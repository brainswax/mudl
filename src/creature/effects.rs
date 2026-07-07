//! Temporary conditions and body-plan effects that modify creature capabilities.

use std::collections::HashMap;

use crate::mudl::AnatomyRegistry;
use crate::object::{Object, PermissionFlags, Property, Value};

use super::conditions::{apply_condition, modifiers_from_def};

/// Aggregated modifiers from active effects.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectModifiers {
    pub max_health_bonus: i64,
    pub max_weight_bonus: i64,
    pub encumbrance_factor: f64,
    pub stat_mods: HashMap<String, i64>,
    pub skill_mods: HashMap<String, i64>,
}

impl Default for EffectModifiers {
    fn default() -> Self {
        Self {
            max_health_bonus: 0,
            max_weight_bonus: 0,
            encumbrance_factor: 1.0,
            stat_mods: HashMap::new(),
            skill_mods: HashMap::new(),
        }
    }
}

impl EffectModifiers {
    pub fn merge(&mut self, other: &EffectModifiers) {
        self.max_health_bonus += other.max_health_bonus;
        self.max_weight_bonus += other.max_weight_bonus;
        self.encumbrance_factor *= other.encumbrance_factor;
        for (stat, bonus) in &other.stat_mods {
            *self.stat_mods.entry(stat.clone()).or_insert(0) += bonus;
        }
        for (skill, bonus) in &other.skill_mods {
            *self.skill_mods.entry(skill.clone()).or_insert(0) += bonus;
        }
        self.encumbrance_factor = self.encumbrance_factor.clamp(0.5, 2.0);
    }
}

/// Active effect ids on a creature (`active_effects` property).
pub fn active_effects(creature: &Object) -> Vec<String> {
    creature.get_string_list("active_effects")
}

fn set_active_effects(creature: &mut Object, effects: Vec<String>) {
    creature.set_string_list("active_effects", effects);
}

/// Sum modifiers from all active effects on a creature.
pub fn collect_active_effect_modifiers(
    creature: &Object,
    anatomy: &AnatomyRegistry,
) -> EffectModifiers {
    let mut mods = EffectModifiers::default();
    for name in active_effects(creature) {
        if let Some(def) = anatomy.effect(&name) {
            mods.merge(&modifiers_from_def(def));
        }
    }
    mods
}

/// Apply an effect by name (idempotent) and refresh derived properties on the creature.
pub fn apply_effect(creature: &mut Object, effect_name: &str, anatomy: &AnatomyRegistry) {
    apply_condition(creature, effect_name, anatomy);
}

/// Remove an effect and refresh derived properties.
pub fn remove_effect(creature: &mut Object, effect_name: &str, anatomy: &AnatomyRegistry) {
    let mut current = active_effects(creature);
    current.retain(|e| e != effect_name);
    if current.is_empty() {
        creature.properties.remove("active_effects");
    } else {
        set_active_effects(creature, current);
    }
    refresh_effect_derived_properties(creature, anatomy);
}

/// Recompute cached modifier properties from active effects.
pub fn refresh_effect_derived_properties(creature: &mut Object, anatomy: &AnatomyRegistry) {
    let mods = collect_active_effect_modifiers(creature, anatomy);
    if mods.stat_mods.is_empty() {
        creature.properties.remove("stat_mods");
    } else {
        creature.set_int_map("stat_mods", mods.stat_mods.clone());
    }
    if mods.skill_mods.is_empty() {
        creature.properties.remove("skill_mods");
    } else {
        creature.set_int_map("skill_mods", mods.skill_mods.clone());
    }
    creature.add_property(Property {
        name: "effect_mod_encumbrance".to_string(),
        value: Value::Float(mods.encumbrance_factor),
        permissions: PermissionFlags::OWNER,
        behavior: None,
    });
    creature.set_property_int("effect_mod_max_weight", mods.max_weight_bonus);
}

/// Encumbrance multiplier from active effects (1.0 when none).
pub fn effect_encumbrance_factor(creature: &Object) -> f64 {
    creature
        .get_float_property("effect_mod_encumbrance")
        .unwrap_or(1.0)
        .clamp(0.5, 2.0)
}

/// Bonus max carry weight from cached effect modifiers.
pub fn effect_max_weight_bonus(creature: &Object) -> i64 {
    creature
        .get_int_property("effect_mod_max_weight")
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::vitality::init_creature_vitality;
    use crate::mudl::CreatureDef;
    use crate::mudl::EffectDef;
    use crate::object::PermissionFlags;

    fn weary_effect() -> EffectDef {
        EffectDef {
            name: "weary".to_string(),
            mod_max_health: 0,
            mod_max_weight: -5,
            mod_encumbrance: 1.1,
            stat_mods: HashMap::from([("dexterity".to_string(), -2)]),
            skill_mods: HashMap::from([("stealth".to_string(), -1)]),
            regen_on_enter: 0,
            condition_type: None,
            cure_tags: Vec::new(),
            damage_on_tick: 0,
            heal_on_tick: 0,
            tick_on: "on_enter".to_string(),
            duration_ticks: 0,
        }
    }

    fn anatomy_with_weary() -> AnatomyRegistry {
        let mut registry = AnatomyRegistry::default();
        registry.effects.insert("weary".to_string(), weary_effect());
        registry
    }

    fn bare_creature(id: &str) -> Object {
        Object {
            id: crate::object::ObjectId::new(id),
            name: "Hero".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: crate::object::ObjectId::new("player:hero-001"),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        }
    }

    #[test]
    fn apply_effect_tracks_active_list_and_stat_mods() {
        let anatomy = anatomy_with_weary();
        let mut creature = bare_creature("player:hero-001");
        init_creature_vitality(
            &mut creature,
            &CreatureDef {
                name: "human".to_string(),
                slots: vec![],
                max_health: 100,
                base_max_weight: Some(100),
                stats: HashMap::from([("dexterity".to_string(), 10)]),
                skills: HashMap::new(),
            },
        );
        apply_effect(&mut creature, "weary", &anatomy);
        assert_eq!(active_effects(&creature), vec!["weary"]);
        assert_eq!(effect_encumbrance_factor(&creature), 1.1);
        assert_eq!(
            creature.get_int_map("stat_mods").get("dexterity").copied(),
            Some(-2)
        );
        assert_eq!(
            creature.get_int_map("skill_mods").get("stealth").copied(),
            Some(-1)
        );
        assert_eq!(crate::creature::vitality::creature_skill(&creature, "stealth"), -1);
        remove_effect(&mut creature, "weary", &anatomy);
        assert!(active_effects(&creature).is_empty());
    }
}
