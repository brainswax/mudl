//! Health, stats, and skills for creatures (players and NPCs).

use std::collections::HashMap;

use crate::mudl::{AnatomyRegistry, CreatureDef};
use crate::object::{Object, ObjectId, PermissionFlags, Property, Value};

use super::stats::{format_skill_values, format_stat_values, max_health_from_creature_def};

/// Default health when a creature definition omits `max_health`.
pub const DEFAULT_MAX_HEALTH: i64 = 100;

/// Apply creature-definition defaults to a player or NPC object.
pub fn init_creature_vitality(creature: &mut Object, def: &CreatureDef) {
    let max_health = max_health_from_creature_def(def);
    creature.set_property_int("health", max_health);
    creature.set_property_int("max_health", max_health);

    if let Some(base) = def.base_max_weight {
        let strength = def.stats.get("strength").copied().unwrap_or(0);
        creature.set_property_int("max_weight", base.saturating_add(strength));
    }

    if !def.stats.is_empty() {
        creature.set_int_map("stats", def.stats.clone());
    }
    if !def.skills.is_empty() {
        creature.set_int_map("skills", def.skills.clone());
    }
    if creature.get_property("active_effects").is_none() {
        creature.add_property(Property {
            name: "active_effects".to_string(),
            value: Value::List(Vec::new()),
            permissions: PermissionFlags::OWNER,
            behavior: None,
        });
    }
}

/// Current health (clamped to zero).
pub fn creature_health(creature: &Object) -> i64 {
    creature.get_int_property("health").unwrap_or(0).max(0)
}

/// Base maximum health stored on the creature.
pub fn creature_base_max_health(creature: &Object) -> i64 {
    creature
        .get_int_property("max_health")
        .unwrap_or(DEFAULT_MAX_HEALTH)
        .max(1)
}

/// Effective max health including temporary effect bonuses.
pub fn creature_max_health(creature: &Object, anatomy: Option<&AnatomyRegistry>) -> i64 {
    let base = creature_base_max_health(creature);
    let bonus = anatomy
        .map(|registry| {
            crate::creature::effects::collect_active_effect_modifiers(creature, registry)
                .max_health_bonus
        })
        .unwrap_or(0);
    (base + bonus).max(1)
}

/// Named stat value (base + temporary effect bonuses stored in `stat_mods`).
pub fn creature_stat(creature: &Object, name: &str) -> i64 {
    let base = creature
        .get_int_map("stats")
        .get(name)
        .copied()
        .unwrap_or(0);
    let bonus = creature
        .get_int_map("stat_mods")
        .get(name)
        .copied()
        .unwrap_or(0);
    base.saturating_add(bonus)
}

/// Named skill rank (base + temporary effect bonuses stored in `skill_mods`).
pub fn creature_skill(creature: &Object, name: &str) -> i64 {
    let base = creature
        .get_int_map("skills")
        .get(name)
        .copied()
        .unwrap_or(0);
    let bonus = creature
        .get_int_map("skill_mods")
        .get(name)
        .copied()
        .unwrap_or(0);
    base.saturating_add(bonus)
}

/// Whether a creature has been reduced to zero health.
pub fn creature_is_defeated(creature: &Object) -> bool {
    creature_health(creature) == 0
}

/// Compact player-facing summary of core stats and skills (base values only).
pub fn format_creature_stats_summary(creature: &Object) -> String {
    format_creature_vitals_summary(creature, None, None)
}

/// Stats/skills summary including equipment when `objects` and `anatomy` are provided.
pub fn format_creature_stats_summary_with_equipment(
    creature: &Object,
    objects: Option<&HashMap<ObjectId, Object>>,
    anatomy: Option<&AnatomyRegistry>,
) -> String {
    format_creature_vitals_summary(creature, objects, anatomy)
}

/// Player-facing vitals: stats sentence and optional skills sentence.
pub fn format_creature_vitals_summary(
    creature: &Object,
    objects: Option<&HashMap<ObjectId, Object>>,
    anatomy: Option<&AnatomyRegistry>,
) -> String {
    let use_equipment = objects.is_some() && anatomy.is_some();
    let stat_effective = |name: &str| {
        if use_equipment {
            crate::creature::creature_effective_stat(
                creature,
                name,
                objects.unwrap(),
                anatomy.unwrap(),
            )
        } else {
            creature_stat(creature, name)
        }
    };
    let skill_effective = |name: &str| {
        if use_equipment {
            crate::creature::creature_effective_skill(
                creature,
                name,
                objects.unwrap(),
                anatomy.unwrap(),
            )
        } else {
            creature_skill(creature, name)
        }
    };

    let stats = format_stat_values(creature, stat_effective);
    let skills = format_skill_values(creature, skill_effective);
    match (stats.is_empty(), skills.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!("You are {stats}"),
        (true, false) => format!("Your skills are {skills}"),
        (false, false) => format!("You are {stats}. Your skills are {skills}"),
    }
}

/// Third-person health phrase for examining an NPC.
pub fn format_npc_health_clause(creature: &Object, anatomy: Option<&AnatomyRegistry>) -> String {
    let display = creature.name.to_lowercase();
    let health = creature_health(creature);
    let max = creature_max_health(creature, anatomy);
    if health == 0 {
        return format!("The {display} is down.");
    }
    if health >= max {
        return format!("The {display} looks fit.");
    }
    if max > 0 && health * 100 / max < 25 {
        return format!("The {display} looks badly hurt ({health}/{max} health).");
    }
    format!("The {display} looks wounded ({health}/{max} health).")
}

/// Apply damage; returns new health.
pub fn apply_damage(creature: &mut Object, amount: i64) -> i64 {
    let current = creature_health(creature);
    let next = (current - amount.max(0)).max(0);
    creature.set_property_int("health", next);
    next
}

/// Heal up to effective max health; returns new health.
pub fn heal(creature: &mut Object, amount: i64, anatomy: Option<&AnatomyRegistry>) -> i64 {
    let current = creature_health(creature);
    let max = creature_max_health(creature, anatomy);
    let next = (current + amount.max(0)).min(max);
    creature.set_property_int("health", next);
    next
}

/// Player-facing health phrase for `examine self`.
pub fn format_health_clause(creature: &Object, anatomy: Option<&AnatomyRegistry>) -> String {
    let health = creature_health(creature);
    let max = creature_max_health(creature, anatomy);
    if health >= max {
        if health == max {
            "You feel fit.".to_string()
        } else {
            format!("You feel vigorous ({health}/{max} health).")
        }
    } else if health == 0 {
        "You are barely conscious.".to_string()
    } else if max > 0 && health * 100 / max < 25 {
        format!("You are badly hurt ({health}/{max} health).")
    } else {
        format!("You are wounded ({health}/{max} health).")
    }
}

/// Resolve a creature definition for an object (player template or explicit creature property).
pub fn creature_def_for<'a>(obj: &Object, anatomy: &'a AnatomyRegistry) -> Option<&'a CreatureDef> {
    let name = obj.creature_name()?;
    anatomy.creature(&name)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::mudl::{BodySlotDef, PlayerTemplate, SlotType};
    use crate::object::PermissionFlags;

    fn human_def() -> CreatureDef {
        CreatureDef {
            name: "human".to_string(),
            slots: vec![BodySlotDef {
                name: "right_hand".to_string(),
                capacity: 1,
                slot_type: SlotType::Grasp,
                hands: 1,
                effect: None,
            }],
            max_health: 100,
            base_max_weight: Some(90),
            stats: HashMap::from([("strength".to_string(), 10), ("dexterity".to_string(), 12)]),
            skills: HashMap::from([("survival".to_string(), 1)]),
        }
    }

    fn bare_creature(id: &str) -> Object {
        let mut obj = Object {
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
        };
        obj.init_creature_role(&PlayerTemplate {
            name: "default".to_string(),
            creature: "human".to_string(),
            gender: "neutral".to_string(),
        });
        obj
    }

    #[test]
    fn init_creature_vitality_sets_health_stats_and_carry_limit() {
        let mut creature = bare_creature("player:hero-001");
        init_creature_vitality(&mut creature, &human_def());
        assert_eq!(creature_health(&creature), 100);
        assert_eq!(creature_max_health(&creature, None), 100);
        assert_eq!(creature_stat(&creature, "strength"), 10);
        assert_eq!(creature_skill(&creature, "survival"), 1);
        assert_eq!(creature.get_int_property("max_weight"), Some(100));
    }

    #[test]
    fn damage_and_heal_clamp_correctly() {
        let mut creature = bare_creature("player:hero-001");
        init_creature_vitality(&mut creature, &human_def());
        assert_eq!(apply_damage(&mut creature, 30), 70);
        assert_eq!(heal(&mut creature, 10, None), 80);
        assert_eq!(apply_damage(&mut creature, 200), 0);
        assert_eq!(heal(&mut creature, 500, None), 100);
    }

    #[test]
    fn health_clause_reflects_condition() {
        let mut creature = bare_creature("player:hero-001");
        init_creature_vitality(&mut creature, &human_def());
        assert_eq!(format_health_clause(&creature, None), "You feel fit.");
        apply_damage(&mut creature, 85);
        assert!(format_health_clause(&creature, None).contains("badly hurt"));
    }

    #[test]
    fn stats_summary_lists_stats_and_skills() {
        let mut creature = bare_creature("player:hero-001");
        init_creature_vitality(&mut creature, &human_def());
        let summary = format_creature_stats_summary(&creature);
        assert!(summary.contains("You are Strength 10"));
        assert!(summary.contains("Dexterity 12"));
        assert!(summary.contains("Your skills are Survival 1"));
    }

    #[test]
    fn defeated_creature_reports_zero_health() {
        let mut creature = bare_creature("player:hero-001");
        init_creature_vitality(&mut creature, &human_def());
        apply_damage(&mut creature, 500);
        assert!(creature_is_defeated(&creature));
    }
}
