//! Active conditions — timed effects, damage-over-time, and targeted cures.

use std::collections::HashMap;

use crate::creature::vitality::{apply_damage, creature_health, heal};
use crate::mudl::{AnatomyRegistry, EffectDef};
use crate::object::{Object, ObjectId};

use super::effects::{
    active_effects, refresh_effect_derived_properties, remove_effect, EffectModifiers,
};

const CONDITION_TICKS_KEY: &str = "condition_ticks";

/// Outcome of ticking conditions on a creature.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConditionTickOutcome {
    pub lines: Vec<String>,
    pub dirty: bool,
}

fn condition_ticks(creature: &Object) -> HashMap<String, i64> {
    creature.get_int_map(CONDITION_TICKS_KEY)
}

fn set_condition_ticks(creature: &mut Object, ticks: HashMap<String, i64>) {
    if ticks.is_empty() {
        creature.properties.remove(CONDITION_TICKS_KEY);
    } else {
        creature.set_int_map(CONDITION_TICKS_KEY, ticks);
    }
}

fn set_effect_duration(creature: &mut Object, effect_name: &str, ticks: i64) {
    let mut map = condition_ticks(creature);
    if ticks > 0 {
        map.insert(effect_name.to_string(), ticks);
    } else {
        map.remove(effect_name);
    }
    set_condition_ticks(creature, map);
}

/// Whether `creature` has `effect_name` active or granted by equipped gear.
pub fn creature_has_effect(
    creature: &Object,
    effect_name: &str,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> bool {
    if active_effects(creature)
        .iter()
        .any(|e| e == effect_name)
    {
        return true;
    }
    creature_has_equipped_effect(creature, effect_name, objects, anatomy)
}

fn creature_has_equipped_effect(
    creature: &Object,
    effect_name: &str,
    objects: &HashMap<ObjectId, Object>,
    _anatomy: &AnatomyRegistry,
) -> bool {
    for item_id in creature.body_slots().values() {
        if let Some(item) = objects.get(item_id) {
            if item
                .equipment_grant_effects()
                .iter()
                .any(|e| e == effect_name)
            {
                return true;
            }
        }
    }
    false
}

/// Whether any active condition matches `tag` (condition type or cure tag).
pub fn creature_has_condition_tag(
    creature: &Object,
    tag: &str,
    anatomy: &AnatomyRegistry,
) -> bool {
    let tag = tag.trim().to_ascii_lowercase();
    if tag.is_empty() {
        return false;
    }
    active_effects(creature).iter().any(|name| {
        anatomy
            .effect(name)
            .is_some_and(|def| effect_matches_tag(def, &tag))
    })
}

fn effect_matches_tag(def: &EffectDef, tag: &str) -> bool {
    def.condition_type
        .as_ref()
        .is_some_and(|k| k.eq_ignore_ascii_case(tag))
        || def.cure_tags.iter().any(|t| t.eq_ignore_ascii_case(tag))
}

/// Apply an effect, starting or refreshing duration ticks when defined.
pub fn apply_condition(creature: &mut Object, effect_name: &str, anatomy: &AnatomyRegistry) {
    let Some(def) = anatomy.effect(effect_name) else {
        return;
    };
    let mut current = active_effects(creature);
    if !current.iter().any(|e| e == effect_name) {
        current.push(effect_name.to_string());
    }
    creature.set_string_list("active_effects", current);
    if def.duration_ticks > 0 {
        set_effect_duration(creature, effect_name, def.duration_ticks);
    }
    refresh_effect_derived_properties(creature, anatomy);
}

/// Remove an effect and clear any tick counter.
pub fn remove_condition(creature: &mut Object, effect_name: &str, anatomy: &AnatomyRegistry) {
    remove_effect(creature, effect_name, anatomy);
    let mut map = condition_ticks(creature);
    map.remove(effect_name);
    set_condition_ticks(creature, map);
}

/// Remove all active conditions matching `tag`. Returns removed effect names.
pub fn cure_by_tag(
    creature: &mut Object,
    tag: &str,
    anatomy: &AnatomyRegistry,
) -> Vec<String> {
    let tag = tag.trim().to_ascii_lowercase();
    let to_remove: Vec<String> = active_effects(creature)
        .into_iter()
        .filter(|name| {
            anatomy
                .effect(name)
                .is_some_and(|def| effect_matches_tag(def, &tag))
        })
        .collect();
    for name in &to_remove {
        remove_condition(creature, name, anatomy);
    }
    to_remove
}

/// Advance timed conditions and apply DOT for `tick_event` (typically `on_enter`).
pub fn tick_conditions(
    creature: &mut Object,
    anatomy: &AnatomyRegistry,
    tick_event: &str,
) -> ConditionTickOutcome {
    let mut outcome = ConditionTickOutcome::default();
    let tick_event = tick_event.trim().to_ascii_lowercase();
    let names: Vec<String> = active_effects(creature);
    let mut ticks = condition_ticks(creature);
    let mut expired = Vec::new();

    for name in &names {
        let Some(def) = anatomy.effect(name) else {
            continue;
        };
        if !def.tick_on.eq_ignore_ascii_case(&tick_event) {
            continue;
        }

        let label = def
            .condition_type
            .as_deref()
            .unwrap_or(&def.name)
            .to_ascii_lowercase();

        if def.damage_on_tick > 0 && creature_health(creature) > 0 {
            let after = apply_damage(creature, def.damage_on_tick);
            outcome.lines.push(format!(
                "The {label} wracks you for {} damage ({after} health remaining).",
                def.damage_on_tick
            ));
            outcome.dirty = true;
        }

        if def.heal_on_tick > 0 && creature_health(creature) > 0 {
            let after = heal(creature, def.heal_on_tick, Some(anatomy));
            outcome.lines.push(format!(
                "The {label} mends you for {} health ({after} health remaining).",
                def.heal_on_tick
            ));
            outcome.dirty = true;
        }

        if def.duration_ticks > 0 {
            if let Some(remaining) = ticks.get_mut(name) {
                *remaining -= 1;
                if *remaining <= 0 {
                    expired.push(name.clone());
                }
            }
        }
    }

    for name in &expired {
        ticks.remove(name);
    }
    set_condition_ticks(creature, ticks);

    for name in expired {
        remove_condition(creature, &name, anatomy);
        outcome.lines.push(format!("The {name} effect fades."));
        outcome.dirty = true;
    }

    if outcome.dirty {
        refresh_effect_derived_properties(creature, anatomy);
    }

    outcome
}

/// Sum modifiers from active effects (re-exported helper for effects module).
pub fn modifiers_from_def(def: &EffectDef) -> EffectModifiers {
    EffectModifiers {
        max_health_bonus: def.mod_max_health,
        max_weight_bonus: def.mod_max_weight,
        encumbrance_factor: def.mod_encumbrance,
        stat_mods: def.stat_mods.clone(),
        skill_mods: def.skill_mods.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::vitality::init_creature_vitality;
    use crate::mudl::CreatureDef;
    use crate::object::PermissionFlags;

    fn venom_def() -> EffectDef {
        EffectDef {
            name: "swamp_venom".to_string(),
            condition_type: Some("poison".to_string()),
            cure_tags: vec!["poison".to_string()],
            damage_on_tick: 4,
            duration_ticks: 3,
            ..default_effect("swamp_venom".to_string())
        }
    }

    fn default_effect(name: String) -> EffectDef {
        EffectDef {
            name,
            mod_max_health: 0,
            mod_max_weight: 0,
            mod_encumbrance: 1.0,
            stat_mods: HashMap::new(),
            skill_mods: HashMap::new(),
            regen_on_enter: 0,
            condition_type: None,
            cure_tags: Vec::new(),
            damage_on_tick: 0,
            heal_on_tick: 0,
            tick_on: "on_enter".to_string(),
            duration_ticks: 0,
        }
    }

    fn regen_def() -> EffectDef {
        EffectDef {
            name: "shore_regeneration".to_string(),
            condition_type: Some("regeneration".to_string()),
            heal_on_tick: 5,
            duration_ticks: 4,
            ..default_effect("shore_regeneration".to_string())
        }
    }

    fn anatomy_with_venom() -> AnatomyRegistry {
        let mut registry = AnatomyRegistry::default();
        registry
            .effects
            .insert("swamp_venom".to_string(), venom_def());
        registry
    }

    fn bare_creature(id: &str) -> Object {
        Object {
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
        }
    }

    #[test]
    fn apply_and_cure_by_tag() {
        let anatomy = anatomy_with_venom();
        let mut creature = bare_creature("player:hero-001");
        init_creature_vitality(
            &mut creature,
            &CreatureDef {
                name: "human".to_string(),
                slots: vec![],
                max_health: 100,
                base_max_weight: None,
                stats: HashMap::new(),
                skills: HashMap::new(),
            },
        );
        apply_condition(&mut creature, "swamp_venom", &anatomy);
        assert!(creature_has_condition_tag(&creature, "poison", &anatomy));
        let removed = cure_by_tag(&mut creature, "poison", &anatomy);
        assert_eq!(removed, vec!["swamp_venom"]);
        assert!(!creature_has_condition_tag(&creature, "poison", &anatomy));
    }

    #[test]
    fn tick_applies_dot_and_expires() {
        let anatomy = anatomy_with_venom();
        let mut creature = bare_creature("player:hero-001");
        init_creature_vitality(
            &mut creature,
            &CreatureDef {
                name: "human".to_string(),
                slots: vec![],
                max_health: 100,
                base_max_weight: None,
                stats: HashMap::new(),
                skills: HashMap::new(),
            },
        );
        apply_condition(&mut creature, "swamp_venom", &anatomy);
        for _ in 0..3 {
            let out = tick_conditions(&mut creature, &anatomy, "on_enter");
            assert!(out.dirty);
            assert!(!out.lines.is_empty());
        }
        assert!(!creature_has_condition_tag(&creature, "poison", &anatomy));
        assert!(creature_health(&creature) < 100);
    }

    #[test]
    fn tick_applies_hot_and_expires() {
        let mut registry = anatomy_with_venom();
        registry
            .effects
            .insert("shore_regeneration".to_string(), regen_def());
        let anatomy = registry;
        let mut creature = bare_creature("player:hero-001");
        init_creature_vitality(
            &mut creature,
            &CreatureDef {
                name: "human".to_string(),
                slots: vec![],
                max_health: 100,
                base_max_weight: None,
                stats: HashMap::new(),
                skills: HashMap::new(),
            },
        );
        apply_damage(&mut creature, 20);
        apply_condition(&mut creature, "shore_regeneration", &anatomy);
        for _ in 0..4 {
            let out = tick_conditions(&mut creature, &anatomy, "on_enter");
            assert!(out.dirty);
            assert!(out.lines.iter().any(|l| l.contains("mends you")));
        }
        assert_eq!(active_effects(&creature), Vec::<String>::new());
        assert_eq!(creature_health(&creature), 100);
    }
}