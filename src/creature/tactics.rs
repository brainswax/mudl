//! Awareness, stealth, surprise, and initiative for tactical combat.

use std::collections::HashMap;

use crate::creature::equipment::{creature_effective_skill, creature_effective_stat};
use crate::creature::vitality::creature_skill;
use crate::mudl::{AnatomyRegistry, BehaviorTemplateDef, CreatureReact};
use crate::object::{Object, ObjectId};

use super::behavior::CreatureBehaviorEntry;

/// Bonus damage when striking an unaware target.
pub const SURPRISE_DAMAGE_BONUS: i64 = 3;

/// Who strikes first in a combat exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrikeOrder {
    ActorFirst,
    TargetFirst,
}

/// Whether `creature` has noticed threats (player) in the current encounter.
pub fn is_creature_aware(creature: &Object) -> bool {
    creature
        .get_bool_property("creature_aware")
        .unwrap_or(true)
}

pub fn set_creature_aware(creature: &mut Object, aware: bool) {
    creature.set_property_bool("creature_aware", aware);
}

pub fn uses_awareness_check(creature: &Object) -> bool {
    creature
        .get_bool_property("uses_awareness_check")
        .unwrap_or(false)
}

pub fn perception_bonus(creature: &Object) -> i64 {
    creature
        .get_int_property("perception_bonus")
        .unwrap_or(0)
        .max(0)
}

fn mix_seed(parts: &[&str]) -> u64 {
    let mut hash = 0u64;
    for part in parts {
        for byte in part.as_bytes() {
            hash = hash.wrapping_mul(31).wrapping_add(u64::from(*byte));
        }
        hash = hash.wrapping_mul(31).wrapping_add(255);
    }
    hash
}

/// Player stealth score for awareness contests.
pub fn player_stealth_score(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> i64 {
    let stealth = creature_effective_skill(player, "stealth", objects, anatomy);
    let dexterity = creature_effective_stat(player, "dexterity", objects, anatomy);
    let wisdom = creature_effective_stat(player, "wisdom", objects, anatomy);
    stealth
        .saturating_mul(2)
        .saturating_add(dexterity)
        .saturating_add(wisdom / 4)
}

/// NPC perception score for awareness contests.
pub fn creature_perception_score(
    creature: &Object,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> i64 {
    let dexterity = creature_effective_stat(creature, "dexterity", objects, anatomy);
    let wisdom = creature_effective_stat(creature, "wisdom", objects, anatomy);
    let alertness = creature_skill(creature, "survival");
    dexterity
        .saturating_add(wisdom / 2)
        .saturating_add(alertness)
        .saturating_add(perception_bonus(creature))
}

/// Returns true when the player slips past the creature unnoticed.
pub fn stealth_check_succeeds(
    player: &Object,
    creature: &Object,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
    seed: u64,
) -> bool {
    let stealth = player_stealth_score(player, objects, anatomy) + i64::from((seed % 5) as u32);
    let perception =
        creature_perception_score(creature, objects, anatomy) + i64::from(((seed / 5) % 5) as u32);
    stealth > perception
}

/// Initiative score — higher acts first (dexterity + optional speed stat).
pub fn initiative_score(
    creature: &Object,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> i64 {
    let dexterity = creature_effective_stat(creature, "dexterity", objects, anatomy);
    let speed = creature_effective_stat(creature, "speed", objects, anatomy);
    let combat = creature_effective_skill(creature, "combat", objects, anatomy);
    dexterity
        .saturating_add(if speed > 0 { speed } else { dexterity / 3 })
        .saturating_add(combat / 4)
}

/// Resolve who strikes first between actor and target.
pub fn resolve_strike_order(
    actor: &Object,
    target: &Object,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> StrikeOrder {
    let actor_init = initiative_score(actor, objects, anatomy);
    let target_init = initiative_score(target, objects, anatomy);
    if target_init > actor_init {
        StrikeOrder::TargetFirst
    } else {
        StrikeOrder::ActorFirst
    }
}

pub fn default_awareness_check(react: CreatureReact) -> bool {
    matches!(react, CreatureReact::Attack)
}

/// Configure awareness/tactics properties from behavior entries and templates.
pub fn apply_tactics_from_behaviors(
    creature: &mut Object,
    entries: &[CreatureBehaviorEntry],
    templates: &HashMap<String, BehaviorTemplateDef>,
) {
    let mut uses_check = false;
    let mut perception = 0i64;

    for entry in entries {
        let Some(react) = entry.react else {
            continue;
        };
        let template = entry
            .template_name
            .as_deref()
            .and_then(|name| templates.get(name));
        let check = entry
            .awareness_check
            .or_else(|| template.and_then(|t| t.awareness_check))
            .unwrap_or_else(|| default_awareness_check(react));
        if check && matches!(react, CreatureReact::Attack | CreatureReact::Warn) {
            uses_check = true;
        }
        let bonus = entry
            .perception
            .or_else(|| template.and_then(|t| t.perception))
            .unwrap_or(0);
        perception = perception.max(bonus);
    }

    creature.set_property_bool("uses_awareness_check", uses_check);
    if perception > 0 {
        creature.set_property_int("perception_bonus", perception);
    } else {
        creature.properties.remove("perception_bonus");
    }
    set_creature_aware(creature, !uses_check);
}

/// Run an awareness contest when the player enters the creature's room.
/// Returns `(aware, narrative line)` when the creature uses awareness checks.
pub fn roll_awareness_on_enter(
    npc_id: &ObjectId,
    player_id: &ObjectId,
    room_id: &ObjectId,
    enter_count: u64,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> Option<(bool, String)> {
    let npc = objects.get(npc_id)?;
    if !uses_awareness_check(npc) {
        return None;
    }
    let player = objects.get(player_id)?;
    let seed = mix_seed(&[
        player_id.as_str(),
        npc_id.as_str(),
        room_id.as_str(),
        &enter_count.to_string(),
    ]);
    let unnoticed = stealth_check_succeeds(player, npc, objects, anatomy, seed);
    let display = npc.name.to_lowercase();
    if unnoticed {
        Some((
            false,
            format!("The {display} hasn't noticed you."),
        ))
    } else {
        Some((
            true,
            format!("The {display} spots you!"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::vitality::init_creature_vitality;
    use crate::mudl::{CreatureDef, PlayerTemplate};
    use crate::object::PermissionFlags;

    fn human_def() -> CreatureDef {
        CreatureDef {
            name: "human".to_string(),
            slots: vec![],
            max_health: 100,
            base_max_weight: Some(90),
            stats: HashMap::from([
                ("dexterity".to_string(), 10),
                ("wisdom".to_string(), 10),
            ]),
            skills: HashMap::from([("stealth".to_string(), 5), ("survival".to_string(), 2)]),
        }
    }

    fn creature(id: &str, name: &str) -> Object {
        let mut obj = Object {
            id: ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new("player:admin-001"),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        obj.init_creature_role(&PlayerTemplate {
            name: "test".to_string(),
            creature: "human".to_string(),
            gender: "neutral".to_string(),
        });
        init_creature_vitality(&mut obj, &human_def());
        obj
    }

    #[test]
    fn high_stealth_can_go_unnoticed() {
        let player = creature("player:hero-001", "Hero");
        let mut lurker = creature("npc:lurker-001", "Pale Lurker");
        lurker.set_property_bool("uses_awareness_check", true);
        lurker.set_property_int("perception_bonus", 4);
        lurker.set_property_bool("creature_aware", false);
        let objects = HashMap::from([
            (player.id.clone(), player),
            (lurker.id.clone(), lurker),
        ]);
        let anatomy = AnatomyRegistry::default();
        let unnoticed = stealth_check_succeeds(
            objects.get(&ObjectId::new("player:hero-001")).unwrap(),
            objects.get(&ObjectId::new("npc:lurker-001")).unwrap(),
            &objects,
            &anatomy,
            0,
        );
        assert!(unnoticed);
    }

    #[test]
    fn faster_creature_wins_initiative() {
        let player = creature("player:hero-001", "Hero");
        let mut lurker = creature("npc:lurker-001", "Pale Lurker");
        lurker.set_int_map(
            "stats",
            HashMap::from([("dexterity".to_string(), 18), ("wisdom".to_string(), 8)]),
        );
        let objects = HashMap::from([
            (player.id.clone(), player.clone()),
            (lurker.id.clone(), lurker.clone()),
        ]);
        let anatomy = AnatomyRegistry::default();
        assert_eq!(
            resolve_strike_order(&player, &lurker, &objects, &anatomy),
            StrikeOrder::TargetFirst
        );
    }
}