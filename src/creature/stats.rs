//! Core stat/skill conventions and constitution-derived health.

use crate::mudl::CreatureDef;
use crate::object::Object;

use super::vitality::DEFAULT_MAX_HEALTH;

/// Baseline constitution used when deriving health from `max_health`.
pub const CONSTITUTION_HEALTH_BASELINE: i64 = 10;

/// Extra max health per constitution point above the baseline.
pub const HEALTH_PER_CONSTITUTION_POINT: i64 = 5;

/// Preferred display order for common stats (custom MUDL stats follow alphabetically).
pub const CORE_STATS: &[&str] = &[
    "strength",
    "dexterity",
    "constitution",
    "intelligence",
    "wisdom",
    "charisma",
];

/// Preferred display order for common skills.
pub const CORE_SKILLS: &[&str] = &["combat", "stealth", "crafting", "survival"];

/// Compute starting max health from a creature definition and constitution.
pub fn max_health_from_creature_def(def: &CreatureDef) -> i64 {
    let base = if def.max_health > 0 {
        def.max_health
    } else {
        DEFAULT_MAX_HEALTH
    };
    let constitution = def
        .stats
        .get("constitution")
        .copied()
        .unwrap_or(CONSTITUTION_HEALTH_BASELINE);
    max_health_from_constitution(base, constitution)
}

/// Apply constitution scaling to a template `max_health` value.
pub fn max_health_from_constitution(base_max_health: i64, constitution: i64) -> i64 {
    let delta = constitution.saturating_sub(CONSTITUTION_HEALTH_BASELINE);
    (base_max_health + delta * HEALTH_PER_CONSTITUTION_POINT).max(1)
}

fn ordered_keys<'a>(core: &[&str], present: impl Iterator<Item = &'a String>) -> Vec<String> {
    let mut keys: Vec<String> = present.map(|s| s.to_string()).collect();
    keys.sort_by(|a, b| {
        let ai = core.iter().position(|c| c.eq_ignore_ascii_case(a));
        let bi = core.iter().position(|c| c.eq_ignore_ascii_case(b));
        match (ai, bi) {
            (Some(i), Some(j)) => i.cmp(&j),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.cmp(b),
        }
    });
    keys
}

pub(crate) fn capitalize_label(name: &str) -> String {
    name.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_value_line(
    name: &str,
    effective: i64,
    base: i64,
) -> String {
    let label = capitalize_label(name);
    if effective > base {
        format!("{label} {effective} (+{})", effective - base)
    } else if effective < base {
        format!("{label} {effective} ({})", effective - base)
    } else {
        format!("{label} {effective}")
    }
}

/// Format stat values for player examine (comma-separated, core order first).
pub fn format_stat_values(
    creature: &Object,
    effective_for: impl Fn(&str) -> i64,
) -> String {
    let base_stats = creature.get_int_map("stats");
    if base_stats.is_empty() {
        return String::new();
    }
    let names = ordered_keys(CORE_STATS, base_stats.keys());
    names
        .iter()
        .map(|name| {
            let base = base_stats.get(name).copied().unwrap_or(0);
            format_value_line(name, effective_for(name), base)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Format skill values for player examine.
pub fn format_skill_values(
    creature: &Object,
    effective_for: impl Fn(&str) -> i64,
) -> String {
    let base_skills = creature.get_int_map("skills");
    if base_skills.is_empty() {
        return String::new();
    }
    let names = ordered_keys(CORE_SKILLS, base_skills.keys());
    names
        .iter()
        .map(|name| {
            let base = base_skills.get(name).copied().unwrap_or(0);
            format_value_line(name, effective_for(name), base)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Third-person gauge for examining NPCs (stats; skills when present).
pub fn format_creature_gauge(
    creature: &Object,
    stat_effective: impl Fn(&str) -> i64,
    skill_effective: impl Fn(&str) -> i64,
) -> String {
    let stats = format_stat_values(creature, &stat_effective);
    let skills = format_skill_values(creature, &skill_effective);
    match (stats.is_empty(), skills.is_empty()) {
        (true, true) => String::new(),
        (false, true) => stats,
        (true, false) => format!("skills {skills}"),
        (false, false) => format!("{stats}; skills {skills}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::PermissionFlags;
    use std::collections::HashMap;

    fn creature_with_vitals() -> Object {
        let mut obj = Object {
            id: crate::object::ObjectId::new("player:hero-001"),
            name: "Hero".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: crate::object::ObjectId::new("player:hero-001"),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        obj.set_int_map(
            "stats",
            HashMap::from([
                ("strength".to_string(), 10),
                ("constitution".to_string(), 12),
            ]),
        );
        obj.set_int_map(
            "skills",
            HashMap::from([("combat".to_string(), 1), ("survival".to_string(), 0)]),
        );
        obj
    }

    #[test]
    fn constitution_scales_max_health() {
        let def = CreatureDef {
            name: "human".to_string(),
            slots: vec![],
            max_health: 100,
            base_max_weight: Some(90),
            stats: HashMap::from([("constitution".to_string(), 12)]),
            skills: HashMap::new(),
        };
        assert_eq!(max_health_from_creature_def(&def), 110);
        assert_eq!(max_health_from_constitution(100, 10), 100);
    }

    #[test]
    fn format_stat_values_shows_equipment_bonus() {
        let creature = creature_with_vitals();
        let line = format_stat_values(&creature, |name| match name {
            "constitution" => 14,
            _ => creature.get_int_map("stats").get(name).copied().unwrap_or(0),
        });
        assert!(line.contains("Strength 10"));
        assert!(line.contains("Constitution 14 (+2)"));
    }
}