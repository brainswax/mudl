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

/// Whether adding `additional` weight would exceed the player's `max_weight` (skips unlimited).
pub fn would_exceed_player_max_weight(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
    additional: f64,
) -> bool {
    let Some(max) = player.get_int_property("max_weight") else {
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