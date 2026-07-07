//! Skill progression — experience points and rank advancement.

use crate::object::Object;

/// Skill XP required to advance one rank.
pub const SKILL_XP_PER_RANK: i64 = 5;

/// Award skill experience; returns a narrative line when a rank increases.
pub fn award_skill_xp(creature: &mut Object, skill: &str, xp: i64) -> Option<String> {
    if xp <= 0 || skill.trim().is_empty() {
        return None;
    }
    let key = skill.to_ascii_lowercase();
    let mut xp_map = creature.get_int_map("skill_xp");
    let total = xp_map.get(&key).copied().unwrap_or(0).saturating_add(xp);
    xp_map.insert(key.clone(), total);

    let mut skills = creature.get_int_map("skills");
    let rank = skills.get(&key).copied().unwrap_or(0);
    let threshold = (rank + 1) * SKILL_XP_PER_RANK;
    if total < threshold {
        creature.set_int_map("skill_xp", xp_map);
        return None;
    }

    let next_rank = rank.saturating_add(1);
    skills.insert(key.clone(), next_rank);
    creature.set_int_map("skills", skills);
    xp_map.insert(key, 0);
    creature.set_int_map("skill_xp", xp_map);

    let label = super::stats::capitalize_label(skill);
    Some(format!("Your {label} skill improves to rank {next_rank}."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::PermissionFlags;
    use std::collections::HashMap;

    fn bare_creature() -> Object {
        Object {
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
        }
    }

    #[test]
    fn skill_rank_advances_after_enough_xp() {
        let mut creature = bare_creature();
        creature.set_int_map("skills", HashMap::from([("combat".to_string(), 0)]));
        assert!(award_skill_xp(&mut creature, "combat", 2).is_none());
        let msg = award_skill_xp(&mut creature, "combat", 3).unwrap();
        assert!(msg.contains("Combat"));
        assert_eq!(
            creature.get_int_map("skills").get("combat").copied(),
            Some(1)
        );
    }
}