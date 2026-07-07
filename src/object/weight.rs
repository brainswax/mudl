//! Recursive weight calculations with cycle protection.

use std::collections::{HashMap, HashSet};

use crate::object::{LocationRef, Object, ObjectId};

/// Format a weight for display: whole numbers without decimals, fractions to one decimal.
pub fn format_weight_amount(w: f64) -> String {
    if !w.is_finite() {
        return "0".to_string();
    }
    let rounded = (w * 10.0).round() / 10.0;
    if rounded.fract().abs() < 1e-9 {
        format!("{}", rounded.round() as i64)
    } else {
        format!("{:.1}", rounded)
    }
}

/// Default carrying capacity for new players.
pub const DEFAULT_PLAYER_MAX_WEIGHT: i64 = 100;

/// `max_weight` value meaning unlimited capacity.
pub const UNLIMITED_WEIGHT: i64 = -1;

/// Carried-weight fraction of `max_weight` at which movement becomes laborious.
pub const ENCUMBRANCE_SLOW_THRESHOLD: f64 = 0.90;

/// Carried-weight fraction of `max_weight` at which movement is blocked.
pub const ENCUMBRANCE_BLOCK_THRESHOLD: f64 = 1.0;

/// Carry bonuses aggregated from worn equipment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarryModifiers {
    /// Added to base `max_weight` while worn.
    pub max_weight_bonus: i64,
    /// Multiplier on encumbrance ratio (`1.0` = unchanged, lower = lighter feel).
    pub encumbrance_factor: f64,
}

impl Default for CarryModifiers {
    fn default() -> Self {
        Self {
            max_weight_bonus: 0,
            encumbrance_factor: 1.0,
        }
    }
}

/// How encumbrance affects player movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncumbranceLevel {
    /// Below the slow threshold — normal movement.
    Unencumbered,
    /// Near capacity — movement allowed with extra narration.
    Encumbered,
    /// At or over capacity — movement blocked.
    Overloaded,
}

/// Whether a weight limit value denotes unlimited capacity.
pub fn is_unlimited_weight(limit: i64) -> bool {
    limit < 0
}

/// Whether a stored weight limit should be enforced or displayed as a cap.
pub fn weight_limit_applies(limit: Option<i64>) -> bool {
    limit.is_some_and(|l| !is_unlimited_weight(l))
}

/// Weight of `units` being transferred (stackables scale by unit weight).
pub fn transfer_weight(item: &Object, objects: &HashMap<ObjectId, Object>, units: u32) -> f64 {
    if item.is_stackable() {
        item.unit_weight() * f64::from(units)
    } else {
        item.total_weight(objects)
    }
}

/// Player who bears carry weight when an item moves to `loc` (inventory, body slot, or worn container).
pub fn player_weight_bearer(
    loc: &LocationRef,
    objects: &HashMap<ObjectId, Object>,
) -> Option<ObjectId> {
    match loc {
        LocationRef::Inventory(id) | LocationRef::BodySlot(id, _) => Some(id.clone()),
        LocationRef::Container(container_id, _) => owner_player_of_container(container_id, objects),
        _ => None,
    }
}

/// Walk container parent chain to the creature wearing or holding it.
pub fn owner_player_of_container(
    container_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> Option<ObjectId> {
    let mut current = container_id;
    let mut visited = HashSet::new();
    while visited.insert(current.clone()) {
        let obj = objects.get(current)?;
        let parent_id = obj.location.as_ref()?;
        let parent = objects.get(parent_id)?;
        if parent.has_creature_role() {
            return Some(parent_id.clone());
        }
        if parent.is_container() {
            current = parent_id;
            continue;
        }
        return None;
    }
    None
}

/// Base carry limit stored on the player (before worn bonuses).
pub fn player_base_max_weight(player: &Object) -> Option<i64> {
    player.get_int_property("max_weight")
}

/// Sum carry modifiers from worn equipment on `player`.
pub fn collect_worn_carry_modifiers(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
) -> CarryModifiers {
    let mut mods = CarryModifiers::default();
    for item_id in player.carried_body_items() {
        let Some(item) = objects.get(&item_id) else {
            continue;
        };
        if !item.is_active() || !item.is_wearable() {
            continue;
        }
        mods.max_weight_bonus += item.carry_max_weight_bonus();
        mods.encumbrance_factor *= item.carry_encumbrance_factor();
    }
    mods.encumbrance_factor = mods.encumbrance_factor.clamp(0.5, 1.0);
    mods
}

/// Effective carry limit including worn equipment and effect bonuses.
pub fn player_effective_max_weight(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
) -> Option<i64> {
    player_effective_max_weight_with_anatomy(player, objects, None)
}

/// Effective carry limit with optional anatomy for granted equipment effects.
pub fn player_effective_max_weight_with_anatomy(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
    anatomy: Option<&crate::mudl::AnatomyRegistry>,
) -> Option<i64> {
    let base = player_base_max_weight(player)?;
    if is_unlimited_weight(base) {
        return Some(base);
    }
    let mut bonus = crate::creature::effect_max_weight_bonus(player);
    if let Some(anatomy) = anatomy {
        bonus +=
            crate::creature::collect_equipment_modifiers(player, objects, anatomy).max_weight_bonus;
    } else {
        bonus += collect_worn_carry_modifiers(player, objects).max_weight_bonus;
    }
    Some(base.saturating_add(bonus))
}

/// Whether adding `additional` weight would exceed effective carry capacity.
pub fn would_exceed_player_max_weight(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
    additional: f64,
) -> bool {
    let Some(max) = player_effective_max_weight(player, objects) else {
        return false;
    };
    if is_unlimited_weight(max) {
        return false;
    }
    let after = player_carried_weight(player, objects) + additional;
    after > max as f64 + 1e-9
}

impl Object {
    /// Total weight of this object: own weight (including stack scaling) plus nested contents.
    pub fn total_weight(&self, objects: &HashMap<ObjectId, Object>) -> f64 {
        let mut visited = HashSet::new();
        self.total_weight_inner(objects, &mut visited)
    }

    fn total_weight_inner(
        &self,
        objects: &HashMap<ObjectId, Object>,
        visited: &mut HashSet<ObjectId>,
    ) -> f64 {
        if !visited.insert(self.id.clone()) {
            return 0.0;
        }

        let mut sum = self.weight();
        if self.is_container() {
            for id in self.container_contents() {
                if let Some(child) = objects.get(&id) {
                    sum += child.total_weight_inner(objects, visited);
                }
            }
        }
        sum
    }

    /// Sum of total weights of direct contents (recursive through nested containers).
    ///
    /// Used for container capacity checks and "current/max" display. Excludes this
    /// container's own shell weight.
    pub fn contents_weight(&self, objects: &HashMap<ObjectId, Object>) -> f64 {
        if !self.is_container() {
            return 0.0;
        }
        self.container_contents()
            .iter()
            .filter_map(|id| objects.get(id))
            .map(|child| child.total_weight(objects))
            .sum()
    }
}

/// Fraction of effective carry capacity currently used (`None` if unlimited or unset).
pub fn player_carry_fraction(player: &Object, objects: &HashMap<ObjectId, Object>) -> Option<f64> {
    let max = player_effective_max_weight(player, objects)?;
    if is_unlimited_weight(max) {
        return None;
    }
    let max_f = max as f64;
    if max_f <= 0.0 {
        return None;
    }
    Some(player_carried_weight(player, objects) / max_f)
}

/// Encumbrance ratio after worn `mod_encumbrance` multipliers.
pub fn player_encumbrance_fraction(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
) -> Option<f64> {
    player_encumbrance_fraction_with_anatomy(player, objects, None)
}

/// Encumbrance ratio including granted equipment effects when anatomy is provided.
pub fn player_encumbrance_fraction_with_anatomy(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
    anatomy: Option<&crate::mudl::AnatomyRegistry>,
) -> Option<f64> {
    let base = player_carry_fraction(player, objects)?;
    let mut factor = collect_worn_carry_modifiers(player, objects).encumbrance_factor
        * crate::creature::effect_encumbrance_factor(player);
    if let Some(anatomy) = anatomy {
        factor *= crate::creature::equipment_granted_encumbrance_factor(player, objects, anatomy);
    }
    Some(base * factor)
}

/// Encumbrance tier from carried weight vs effective capacity and worn modifiers.
pub fn player_encumbrance_level(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
) -> EncumbranceLevel {
    player_encumbrance_level_with_anatomy(player, objects, None)
}

/// Encumbrance tier with optional anatomy for granted equipment effects.
pub fn player_encumbrance_level_with_anatomy(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
    anatomy: Option<&crate::mudl::AnatomyRegistry>,
) -> EncumbranceLevel {
    match player_encumbrance_fraction_with_anatomy(player, objects, anatomy) {
        None => EncumbranceLevel::Unencumbered,
        Some(ratio) if ratio >= ENCUMBRANCE_BLOCK_THRESHOLD => EncumbranceLevel::Overloaded,
        Some(ratio) if ratio >= ENCUMBRANCE_SLOW_THRESHOLD => EncumbranceLevel::Encumbered,
        Some(_) => EncumbranceLevel::Unencumbered,
    }
}

/// Total weight carried by a player across body slots and nested containers.
pub fn player_carried_weight(player: &Object, objects: &HashMap<ObjectId, Object>) -> f64 {
    let mut total = 0.0;
    let mut seen = HashSet::new();
    for item_id in player.carried_body_items() {
        if !seen.insert(item_id.clone()) {
            continue;
        }
        let Some(item) = objects.get(&item_id) else {
            continue;
        };
        total += item.total_weight(objects);
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{ContainerSpec, PermissionFlags, StackableSpec};

    fn bare(id: &str) -> Object {
        Object {
            id: ObjectId::new(id),
            name: "item".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new("player:hero-001"),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        }
    }

    #[test]
    fn format_weight_amount_one_decimal_for_fractions() {
        assert_eq!(format_weight_amount(2.1), "2.1");
        assert_eq!(format_weight_amount(2.0), "2");
        assert_eq!(format_weight_amount(0.1), "0.1");
        assert_eq!(format_weight_amount(21.0), "21");
    }

    #[test]
    fn stackable_fractional_total_weight() {
        let mut coins = bare("item:coins-001");
        coins.name = "coins".to_string();
        coins.set_property_numeric("weight", 0.1);
        coins.apply_stackable_role(&StackableSpec {
            count: 21,
            max_stack: 99,
        });
        assert!((coins.weight() - 2.1).abs() < f64::EPSILON);
        assert_eq!(format_weight_amount(coins.weight()), "2.1");
    }

    #[test]
    fn weight_limit_applies_excludes_unlimited() {
        assert!(!weight_limit_applies(Some(UNLIMITED_WEIGHT)));
        assert!(weight_limit_applies(Some(10)));
        assert!(!weight_limit_applies(None));
    }

    #[test]
    fn stackable_total_weight_scales_with_count() {
        let mut coins = bare("item:coins-001");
        coins.name = "coins".to_string();
        coins.set_property_int("weight", 2);
        coins.apply_stackable_role(&StackableSpec {
            count: 10,
            max_stack: 99,
        });
        assert!((coins.total_weight(&HashMap::new()) - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn nested_container_weight_is_recursive() {
        let mut purse = bare("item:purse-001");
        purse.name = "purse".to_string();
        purse.set_property_int("weight", 1);
        purse.apply_container_role(&ContainerSpec {
            capacity: 3,
            max_weight: Some(10),
            max_volume: None,
            wearable: true,
            wear_slot: Some("torso".to_string()),
            ..crate::object::ContainerSpec::default()
        });

        let mut pouch = bare("item:pouch-001");
        pouch.name = "pouch".to_string();
        pouch.set_property_int("weight", 1);
        pouch.apply_container_role(&ContainerSpec {
            capacity: 2,
            max_weight: None,
            max_volume: None,
            wearable: false,
            wear_slot: None,
            ..crate::object::ContainerSpec::default()
        });

        let mut coins = bare("item:coins-001");
        coins.name = "coins".to_string();
        coins.set_property_int("weight", 1);
        coins.apply_stackable_role(&StackableSpec {
            count: 5,
            max_stack: 99,
        });

        pouch.set_property_list("contents", vec![coins.id.clone()]);
        purse.set_property_list("contents", vec![pouch.id.clone()]);

        let mut objects = HashMap::new();
        objects.insert(coins.id.clone(), coins);
        objects.insert(pouch.id.clone(), pouch.clone());
        objects.insert(purse.id.clone(), purse.clone());

        assert!((pouch.total_weight(&objects) - 6.0).abs() < f64::EPSILON);
        assert!((purse.contents_weight(&objects) - 6.0).abs() < f64::EPSILON);
        assert!((purse.total_weight(&objects) - 7.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cycle_protection_avoids_infinite_recursion() {
        let mut a = bare("item:a-001");
        a.apply_container_role(&ContainerSpec::default());
        let mut b = bare("item:b-001");
        b.apply_container_role(&ContainerSpec::default());

        a.set_property_list("contents", vec![b.id.clone()]);
        b.set_property_list("contents", vec![a.id.clone()]);
        a.set_property_int("weight", 1);
        b.set_property_int("weight", 1);

        let mut objects = HashMap::new();
        objects.insert(a.id.clone(), a.clone());
        objects.insert(b.id.clone(), b.clone());

        let w = a.total_weight(&objects);
        assert!(w >= 1.0);
        assert!(w < 1000.0);
    }

    #[test]
    fn player_encumbrance_level_thresholds() {
        let mut player = bare("player:hero-001");
        player.set_property_int("max_weight", 100);

        assert_eq!(
            player_encumbrance_level(&player, &HashMap::new()),
            EncumbranceLevel::Unencumbered
        );

        let item_id = ObjectId::new("item:anvil-001");
        let mut heavy = bare("item:anvil-001");
        heavy.set_property_numeric("weight", 89.0);
        heavy.location = Some(player.id.clone());
        player.set_property_map(
            "body_slots",
            HashMap::from([("right_hand".to_string(), item_id.clone())]),
        );
        let objects = HashMap::from([
            (player.id.clone(), player.clone()),
            (item_id.clone(), heavy),
        ]);
        assert_eq!(
            player_encumbrance_level(&player, &objects),
            EncumbranceLevel::Unencumbered
        );

        let mut heavier = objects.get(&item_id).unwrap().clone();
        heavier.set_property_numeric("weight", 92.0);
        let objects = HashMap::from([
            (player.id.clone(), player.clone()),
            (item_id.clone(), heavier),
        ]);
        assert_eq!(
            player_encumbrance_level(&player, &objects),
            EncumbranceLevel::Encumbered
        );

        let mut maxed = objects.get(&item_id).unwrap().clone();
        maxed.set_property_numeric("weight", 100.0);
        let objects = HashMap::from([
            (player.id.clone(), player.clone()),
            (item_id.clone(), maxed),
        ]);
        assert_eq!(
            player_encumbrance_level(&player, &objects),
            EncumbranceLevel::Overloaded
        );
    }

    #[test]
    fn worn_boots_increase_effective_max_weight() {
        let mut player = bare("player:hero-001");
        player.set_property_int("max_weight", 100);
        let mut boots = bare("item:boots-001");
        boots.name = "Boots of Carrying".to_string();
        let mut boot_spec = crate::object::WearableSpec::new("left_foot", 2.0, 2.0);
        boot_spec.mod_max_weight = Some(25);
        boot_spec.mod_encumbrance = Some(0.85);
        boots.apply_wearable_role(&boot_spec);
        boots.location = Some(player.id.clone());
        player.set_property_map(
            "body_slots",
            HashMap::from([("left_foot".to_string(), boots.id.clone())]),
        );
        let objects = HashMap::from([
            (player.id.clone(), player.clone()),
            (boots.id.clone(), boots),
        ]);
        assert_eq!(player_effective_max_weight(&player, &objects), Some(125));
    }

    #[test]
    fn worn_boots_reduce_encumbrance_without_changing_carry_fraction_denominator() {
        let mut player = bare("player:hero-001");
        player.set_property_int("max_weight", 100);
        let item_id = ObjectId::new("item:crate-001");
        let mut heavy = bare("item:crate-001");
        heavy.set_property_numeric("weight", 92.0);
        heavy.location = Some(player.id.clone());
        let mut boots = bare("item:boots-001");
        let mut boot_spec = crate::object::WearableSpec::new("left_foot", 2.0, 2.0);
        boot_spec.mod_max_weight = Some(25);
        boot_spec.mod_encumbrance = Some(0.85);
        boots.apply_wearable_role(&boot_spec);
        boots.location = Some(player.id.clone());
        player.set_property_map(
            "body_slots",
            HashMap::from([
                ("right_hand".to_string(), item_id.clone()),
                ("left_foot".to_string(), boots.id.clone()),
            ]),
        );
        let objects = HashMap::from([
            (player.id.clone(), player.clone()),
            (item_id, heavy),
            (boots.id.clone(), boots),
        ]);

        assert_eq!(
            player_encumbrance_level(&player, &objects),
            EncumbranceLevel::Unencumbered
        );
        // Effective capacity is 125 with +25 from boots; carried is 94 (crate + boots).
        assert!((player_carry_fraction(&player, &objects).unwrap() - 94.0 / 125.0).abs() < 0.01);
    }

    #[test]
    fn player_encumbrance_ignored_when_unlimited() {
        let mut player = bare("player:hero-001");
        player.set_property_int("max_weight", UNLIMITED_WEIGHT);
        let mut heavy = bare("item:anvil-001");
        heavy.set_property_numeric("weight", 500.0);
        heavy.location = Some(player.id.clone());
        player.set_property_map(
            "body_slots",
            HashMap::from([("right_hand".to_string(), heavy.id.clone())]),
        );
        let objects = HashMap::from([
            (player.id.clone(), player.clone()),
            (heavy.id.clone(), heavy),
        ]);
        assert_eq!(
            player_encumbrance_level(&player, &objects),
            EncumbranceLevel::Unencumbered
        );
    }

    #[test]
    fn would_exceed_player_max_weight_detects_overflow() {
        let mut player = bare("player:hero-001");
        player.set_property_int("max_weight", 100);
        let objects = HashMap::new();
        assert!(!would_exceed_player_max_weight(&player, &objects, 50.0));
        assert!(would_exceed_player_max_weight(&player, &objects, 101.0));
    }

    #[test]
    fn unlimited_max_weight_never_exceeds() {
        let mut player = bare("player:hero-001");
        player.set_property_int("max_weight", UNLIMITED_WEIGHT);
        let objects = HashMap::new();
        assert!(!would_exceed_player_max_weight(&player, &objects, 10_000.0));
    }

    #[test]
    fn owner_player_of_worn_container() {
        let player_id = ObjectId::new("player:hero-001");
        let mut player = bare("player:hero-001");
        player.id = player_id.clone();

        let mut backpack = bare("item:backpack-001");
        backpack.name = "backpack".to_string();
        backpack.location = Some(player_id.clone());
        backpack.apply_container_role(&ContainerSpec {
            capacity: 5,
            max_weight: None,
            max_volume: None,
            wearable: true,
            wear_slot: Some("torso".to_string()),
            ..crate::object::ContainerSpec::default()
        });

        let mut objects = HashMap::new();
        objects.insert(player.id.clone(), player);
        objects.insert(backpack.id.clone(), backpack.clone());

        assert_eq!(
            owner_player_of_container(&backpack.id, &objects),
            Some(player_id.clone())
        );
        assert_eq!(
            player_weight_bearer(&LocationRef::Container(backpack.id, None), &objects),
            Some(player_id)
        );
    }

    #[test]
    fn player_carried_weight_sums_nested_possession() {
        let mut player = bare("player:hero-001");
        player.name = "Hero".to_string();

        let mut purse = bare("item:purse-001");
        purse.set_property_int("weight", 1);
        purse.apply_container_role(&ContainerSpec {
            capacity: 3,
            max_weight: Some(10),
            max_volume: None,
            wearable: true,
            wear_slot: Some("torso".to_string()),
            ..crate::object::ContainerSpec::default()
        });

        let mut coins = bare("item:coins-001");
        coins.set_property_int("weight", 1);
        coins.apply_stackable_role(&StackableSpec {
            count: 10,
            max_stack: 99,
        });
        purse.set_property_list("contents", vec![coins.id.clone()]);

        player.set_body_slot("torso", Some(purse.id.clone()));
        player.set_body_slot("right_hand", Some(coins.id.clone()));

        let mut objects = HashMap::new();
        objects.insert(player.id.clone(), player.clone());
        objects.insert(purse.id.clone(), purse);
        objects.insert(coins.id.clone(), coins);

        // Purse on torso (1 shell + 10 coins inside) + 10 coins in hand
        assert!((player_carried_weight(&player, &objects) - 21.0).abs() < f64::EPSILON);
    }
}
