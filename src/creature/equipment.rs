//! Equipment modifiers — stats, skills, carry bonuses, and granted effects from worn/wielded gear.

use std::collections::HashMap;

use crate::mudl::{AnatomyRegistry, EffectDef};
use crate::object::{Object, ObjectId};

/// Aggregated modifiers from all equipped items (wear slots and grasp slots).
#[derive(Debug, Clone, PartialEq)]
pub struct EquipmentModifiers {
    pub max_health_bonus: i64,
    pub max_weight_bonus: i64,
    pub encumbrance_factor: f64,
    pub stat_mods: HashMap<String, i64>,
    pub skill_mods: HashMap<String, i64>,
    pub regen_on_enter: i64,
}

impl Default for EquipmentModifiers {
    fn default() -> Self {
        Self {
            max_health_bonus: 0,
            max_weight_bonus: 0,
            encumbrance_factor: 1.0,
            stat_mods: HashMap::new(),
            skill_mods: HashMap::new(),
            regen_on_enter: 0,
        }
    }
}

impl EquipmentModifiers {
    pub fn merge(&mut self, other: &EquipmentModifiers) {
        self.max_health_bonus += other.max_health_bonus;
        self.max_weight_bonus += other.max_weight_bonus;
        self.encumbrance_factor *= other.encumbrance_factor;
        self.regen_on_enter += other.regen_on_enter;
        for (stat, bonus) in &other.stat_mods {
            *self.stat_mods.entry(stat.clone()).or_insert(0) += bonus;
        }
        for (skill, bonus) in &other.skill_mods {
            *self.skill_mods.entry(skill.clone()).or_insert(0) += bonus;
        }
        self.encumbrance_factor = self.encumbrance_factor.clamp(0.5, 2.0);
    }
}

fn modifiers_from_item(item: &Object) -> EquipmentModifiers {
    let encumbrance_factor = if item.has_carry_modifiers() {
        let factor = item.carry_encumbrance_factor();
        if factor <= 0.0 || !factor.is_finite() {
            1.0
        } else {
            factor
        }
    } else {
        1.0
    };
    EquipmentModifiers {
        max_health_bonus: item.equipment_max_health_bonus(),
        max_weight_bonus: item.carry_max_weight_bonus(),
        encumbrance_factor,
        stat_mods: item.equipment_stat_mods(),
        skill_mods: item.equipment_skill_mods(),
        regen_on_enter: 0,
    }
}

fn modifiers_from_effect(def: &EffectDef) -> EquipmentModifiers {
    let encumbrance_factor = if (def.mod_encumbrance - 1.0).abs() > 1e-9
        && def.mod_encumbrance > 0.0
        && def.mod_encumbrance.is_finite()
    {
        def.mod_encumbrance
    } else {
        1.0
    };
    EquipmentModifiers {
        max_health_bonus: def.mod_max_health,
        max_weight_bonus: def.mod_max_weight,
        encumbrance_factor,
        stat_mods: def.stat_mods.clone(),
        skill_mods: def.skill_mods.clone(),
        regen_on_enter: def.regen_on_enter,
    }
}

/// Whether `item` contributes any equipment modifiers.
pub fn item_has_equipment_modifiers(item: &Object) -> bool {
    item.equipment_max_health_bonus() != 0
        || item.carry_max_weight_bonus() != 0
        || (item.carry_encumbrance_factor() - 1.0).abs() > 1e-9
        || !item.equipment_stat_mods().is_empty()
        || !item.equipment_skill_mods().is_empty()
        || !item.equipment_grant_effects().is_empty()
}

/// Sum modifiers from all items in the creature's body slots (worn and wielded).
pub fn collect_equipment_modifiers(
    creature: &Object,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> EquipmentModifiers {
    let mut total = EquipmentModifiers::default();
    for item_id in creature.carried_body_items() {
        let Some(item) = objects.get(&item_id) else {
            continue;
        };
        if !item.is_active() {
            continue;
        }
        if item_has_equipment_modifiers(item) {
            total.merge(&modifiers_from_item(item));
        }
        for effect_name in item.equipment_grant_effects() {
            if let Some(def) = anatomy.effect(&effect_name) {
                total.merge(&modifiers_from_effect(def));
            }
        }
    }
    total
}

/// Effective stat including base, active effects, and equipment.
pub fn creature_effective_stat(
    creature: &Object,
    name: &str,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> i64 {
    let base = crate::creature::vitality::creature_stat(creature, name);
    let equipment = collect_equipment_modifiers(creature, objects, anatomy);
    let bonus = equipment.stat_mods.get(name).copied().unwrap_or(0);
    base.saturating_add(bonus)
}

/// Effective skill including base and equipment bonuses.
pub fn creature_effective_skill(
    creature: &Object,
    name: &str,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> i64 {
    let base = crate::creature::vitality::creature_skill(creature, name);
    let equipment = collect_equipment_modifiers(creature, objects, anatomy);
    let bonus = equipment.skill_mods.get(name).copied().unwrap_or(0);
    base.saturating_add(bonus)
}

/// Effective max health including active effects and equipment.
pub fn creature_effective_max_health(
    creature: &Object,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> i64 {
    let base = crate::creature::vitality::creature_max_health(creature, Some(anatomy));
    let equipment = collect_equipment_modifiers(creature, objects, anatomy);
    (base + equipment.max_health_bonus).max(1)
}

/// Encumbrance multiplier from equipment-granted effects only (not direct item props).
pub fn equipment_granted_encumbrance_factor(
    creature: &Object,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> f64 {
    let mut factor = 1.0;
    for item_id in creature.carried_body_items() {
        let Some(item) = objects.get(&item_id) else {
            continue;
        };
        if !item.is_active() {
            continue;
        }
        for effect_name in item.equipment_grant_effects() {
            if let Some(def) = anatomy.effect(&effect_name) {
                factor *= def.mod_encumbrance;
            }
        }
    }
    factor.clamp(0.5, 2.0)
}

/// Apply regeneration from equipped gear when entering a room; returns narrative line.
pub fn apply_equipment_regen_on_enter(
    creature: &mut Object,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> Option<String> {
    if !creature.has_creature_role() {
        return None;
    }
    let regen = collect_equipment_modifiers(creature, objects, anatomy).regen_on_enter;
    if regen <= 0 {
        return None;
    }
    let before = crate::creature::vitality::creature_health(creature);
    let max = creature_effective_max_health(creature, objects, anatomy);
    let next = (before + regen).min(max);
    if next <= before {
        return None;
    }
    creature.set_property_int("health", next);
    Some(format!("You feel renewed ({before} → {next} health)."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::vitality::init_creature_vitality;
    use crate::mudl::{CreatureDef, EffectDef, PlayerTemplate};
    use crate::object::{PermissionFlags, WearableSpec};

    fn anatomy_with_regen() -> AnatomyRegistry {
        let mut registry = AnatomyRegistry::default();
        registry.effects.insert(
            "regeneration".to_string(),
            EffectDef {
                name: "regeneration".to_string(),
                mod_max_health: 0,
                mod_max_weight: 0,
                mod_encumbrance: 1.0,
                stat_mods: HashMap::new(),
                skill_mods: HashMap::new(),
                regen_on_enter: 3,
            },
        );
        registry.creatures.insert(
            "human".to_string(),
            CreatureDef {
                name: "human".to_string(),
                slots: vec![],
                max_health: 100,
                base_max_weight: Some(90),
                stats: HashMap::from([("strength".to_string(), 10)]),
                skills: HashMap::from([("survival".to_string(), 0)]),
            },
        );
        registry
    }

    fn player(id: &str) -> Object {
        let mut obj = Object {
            id: ObjectId::new(id),
            name: "Hero".to_string(),
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
        };
        obj.init_creature_role(&PlayerTemplate {
            name: "default".to_string(),
            creature: "human".to_string(),
            gender: "neutral".to_string(),
        });
        obj
    }

    #[test]
    fn stacked_wearables_merge_stat_and_carry_modifiers() {
        let anatomy = anatomy_with_regen();
        let mut hero = player("player:hero-001");
        init_creature_vitality(&mut hero, anatomy.creature("human").unwrap());

        let mut boots = Object {
            id: ObjectId::new("item:boots-001"),
            name: "Boots".to_string(),
            aliases: Vec::new(),
            location: Some(hero.id.clone()),
            prototype: None,
            owner: hero.id.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        let mut boot_spec = WearableSpec::new("left_foot", 2.0, 2.0);
        boot_spec.mod_max_weight = Some(25);
        boot_spec.mod_encumbrance = Some(0.85);
        boots.apply_wearable_role(&boot_spec);
        boots.apply_equipment_mods(
            Some(0),
            HashMap::from([("strength".to_string(), 1)]),
            HashMap::new(),
            Vec::new(),
        );

        let mut vest = Object {
            id: ObjectId::new("item:vest-001"),
            name: "Vest".to_string(),
            aliases: Vec::new(),
            location: Some(hero.id.clone()),
            prototype: None,
            owner: hero.id.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        vest.apply_wearable_role(&WearableSpec::new("torso", 3.0, 3.0));
        vest.apply_equipment_mods(
            Some(5),
            HashMap::from([("strength".to_string(), 2)]),
            HashMap::from([("survival".to_string(), 1)]),
            vec!["regeneration".to_string()],
        );

        hero.set_property_map(
            "body_slots",
            HashMap::from([
                ("left_foot".to_string(), boots.id.clone()),
                ("torso".to_string(), vest.id.clone()),
            ]),
        );

        let objects = HashMap::from([
            (hero.id.clone(), hero.clone()),
            (boots.id.clone(), boots),
            (vest.id.clone(), vest),
        ]);
        let boots_obj = objects.get(&ObjectId::new("item:boots-001")).unwrap();
        assert!(boots_obj.has_carry_modifiers());
        assert!((boots_obj.carry_encumbrance_factor() - 0.85).abs() < f64::EPSILON);
        assert_eq!(hero.carried_body_items().len(), 2);

        let mods = collect_equipment_modifiers(&hero, &objects, &anatomy);
        assert_eq!(mods.max_weight_bonus, 25);
        assert_eq!(mods.max_health_bonus, 5);
        assert_eq!(mods.stat_mods.get("strength").copied(), Some(3));
        assert_eq!(mods.skill_mods.get("survival").copied(), Some(1));
        assert_eq!(mods.regen_on_enter, 3);
        assert!((mods.encumbrance_factor - 0.85).abs() < f64::EPSILON);

        assert_eq!(
            creature_effective_stat(&hero, "strength", &objects, &anatomy),
            13
        );
        assert_eq!(
            creature_effective_skill(&hero, "survival", &objects, &anatomy),
            1
        );
        assert_eq!(
            creature_effective_max_health(&hero, &objects, &anatomy),
            105
        );
    }

    #[test]
    fn wielded_weapon_stat_bonus_counts_in_grasp_slot() {
        let anatomy = anatomy_with_regen();
        let mut hero = player("player:hero-001");
        init_creature_vitality(&mut hero, anatomy.creature("human").unwrap());

        let mut blade = Object {
            id: ObjectId::new("item:blade-001"),
            name: "Blade".to_string(),
            aliases: Vec::new(),
            location: Some(hero.id.clone()),
            prototype: None,
            owner: hero.id.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        blade.set_property_string("hand_slot", "right");
        blade.apply_equipment_mods(
            None,
            HashMap::from([("strength".to_string(), 2)]),
            HashMap::new(),
            Vec::new(),
        );
        hero.set_property_map(
            "body_slots",
            HashMap::from([("right_hand".to_string(), blade.id.clone())]),
        );

        let objects = HashMap::from([(hero.id.clone(), hero.clone()), (blade.id.clone(), blade)]);
        assert_eq!(
            creature_effective_stat(&hero, "strength", &objects, &anatomy),
            12
        );
    }

    #[test]
    fn equipment_regen_heals_on_enter() {
        let anatomy = anatomy_with_regen();
        let mut hero = player("player:hero-001");
        init_creature_vitality(&mut hero, anatomy.creature("human").unwrap());
        hero.set_property_int("health", 80);

        let mut band = Object {
            id: ObjectId::new("item:band-001"),
            name: "Band".to_string(),
            aliases: Vec::new(),
            location: Some(hero.id.clone()),
            prototype: None,
            owner: hero.id.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        band.apply_equipment_mods(
            None,
            HashMap::new(),
            HashMap::new(),
            vec!["regeneration".to_string()],
        );
        hero.set_property_map(
            "body_slots",
            HashMap::from([("torso".to_string(), band.id.clone())]),
        );

        let hero_id = ObjectId::new("player:hero-001");
        let objects = HashMap::from([(hero_id.clone(), hero), (band.id.clone(), band)]);
        let mut player = objects.get(&hero_id).unwrap().clone();
        let msg = apply_equipment_regen_on_enter(&mut player, &objects, &anatomy).unwrap();
        assert!(msg.contains("renewed"));
        assert_eq!(crate::creature::vitality::creature_health(&player), 83);
    }
}
