//! Perception discovery for hidden creatures and objects — fires `on_discovered` via the event bus.

use std::collections::HashMap;

use crate::creature::behavior::run_perception_discovery_on_look;
use crate::creature::tactics::player_perception_score;
use crate::mudl::AnatomyRegistry;
use crate::mudl::trigger_def::events;
use crate::object::{Object, ObjectId};

use super::dispatch_guard::DispatchStack;
use super::events::{execute_event, EventContext, EventOutcome};

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

fn perception_look_count(player: &Object) -> u64 {
    player
        .get_int_property("perception_look_count")
        .unwrap_or(0)
        .max(0) as u64
}

/// Whether `obj` is hidden from the player until discovered on `look`.
pub fn is_object_hidden_from_player(obj: &Object) -> bool {
    obj.is_active()
        && obj.get_bool_property("hidden_until_discovered")
            .unwrap_or(false)
        && !obj.get_bool_property("player_discovered").unwrap_or(false)
}

/// Whether a room object should appear in listings and targeting.
pub fn object_visible_to_player(obj: &Object) -> bool {
    !is_object_hidden_from_player(obj)
}

/// Whether a creature or object should appear in room listings and targeting.
pub fn entity_visible_to_player(obj: &Object) -> bool {
    crate::creature::creature_visible_to_player(obj) && object_visible_to_player(obj)
}

fn player_notices_object(
    player: &Object,
    obj: &Object,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
    seed: u64,
) -> bool {
    let stealth = obj
        .get_int_property("discovery_stealth")
        .unwrap_or(8)
        .max(0);
    let perception =
        player_perception_score(player, objects, anatomy) + i64::from((seed % 5) as u32);
    perception > stealth
}

fn hidden_objects_in_room<'a>(
    room_id: &ObjectId,
    player_id: &ObjectId,
    objects: &'a HashMap<ObjectId, Object>,
) -> Vec<&'a Object> {
    objects
        .values()
        .filter(|obj| {
            obj.is_active()
                && obj.id != *player_id
                && obj.location.as_ref() == Some(room_id)
                && is_object_hidden_from_player(obj)
        })
        .collect()
}

/// Reveal hidden objects in `room_id` and fire `on_discovered` triggers on each.
pub fn run_object_discovery_on_look(
    dispatch: &mut DispatchStack,
    room_id: &ObjectId,
    player_id: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> EventOutcome {
    let hidden: Vec<ObjectId> = hidden_objects_in_room(room_id, player_id, objects)
        .into_iter()
        .map(|obj| obj.id.clone())
        .collect();
    if hidden.is_empty() {
        return EventOutcome::default();
    }

    let mut outcome = EventOutcome::default();
    let look_tick = {
        let tick = objects
            .get(player_id)
            .map(perception_look_count)
            .unwrap_or(0)
            + 1;
        if let Some(player) = objects.get_mut(player_id) {
            player.set_property_int("perception_look_count", tick as i64);
            outcome.mark_dirty(player_id);
        }
        tick
    };
    let player_snapshot = match objects.get(player_id) {
        Some(player) => player.clone(),
        None => return outcome,
    };

    for obj_id in hidden {
        let obj = match objects.get(&obj_id) {
            Some(obj) => obj.clone(),
            None => continue,
        };
        let seed = mix_seed(&[
            player_id.as_str(),
            obj_id.as_str(),
            room_id.as_str(),
            "obj-look",
            &look_tick.to_string(),
        ]);
        if !player_notices_object(&player_snapshot, &obj, objects, anatomy, seed) {
            continue;
        }
        if let Some(obj_mut) = objects.get_mut(&obj_id) {
            obj_mut.set_property_bool("player_discovered", true);
            outcome.mark_dirty(&obj_id);
        }
        let display = obj.name.to_lowercase();
        outcome.push_line(format!("You notice {display} here."));
        outcome.append(execute_event(
            dispatch,
            events::ON_DISCOVERED,
            &EventContext {
                actor_id: player_id.clone(),
                host_id: obj_id.clone(),
                room_id: Some(room_id.clone()),
                target_id: None,
            },
            objects,
            Some(anatomy),
        ));
    }

    outcome
}

/// Unified perception pass for creatures and hidden objects when the player looks around.
pub fn run_discovery_on_look(
    dispatch: &mut DispatchStack,
    room_id: &ObjectId,
    player_id: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> EventOutcome {
    let mut outcome = EventOutcome::default();
    let creature = run_perception_discovery_on_look(room_id, player_id, objects, anatomy);
    for line in creature.lines {
        outcome.push_line(line);
    }
    for id in creature.dirty {
        outcome.mark_dirty(&id);
    }
    outcome.append(run_object_discovery_on_look(
        dispatch, room_id, player_id, objects, anatomy,
    ));
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mudl::trigger_def::TriggerDef;
    use crate::object::PermissionFlags;
    use crate::world::events::attach_triggers;

    #[test]
    fn hidden_object_fires_on_discovered_trigger() {
        let room_id = ObjectId::new("area:moss-001");
        let player_id = ObjectId::new("player:hero-001");
        let cache_id = ObjectId::new("item:cache-001");

        let mut cache = Object {
            id: cache_id.clone(),
            name: "Supply Cache".to_string(),
            aliases: Vec::new(),
            location: Some(room_id.clone()),
            prototype: None,
            owner: player_id.clone(),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        cache.set_property_bool("hidden_until_discovered", true);
        cache.set_property_int("discovery_stealth", 0);
        attach_triggers(
            &mut cache,
            &[TriggerDef {
                event: events::ON_DISCOVERED.to_string(),
                code: "narrate You kneel to inspect the bundle.".to_string(),
            }],
        );

        let mut player = Object {
            id: player_id.clone(),
            name: "Hero".to_string(),
            aliases: Vec::new(),
            location: Some(room_id.clone()),
            prototype: None,
            owner: player_id.clone(),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        player.set_property_int("survival", 10);

        let room = Object {
            id: room_id.clone(),
            name: "Moss Choke".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: player_id.clone(),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };

        let anatomy = AnatomyRegistry::default();
        let mut objects = HashMap::from([
            (room_id.clone(), room),
            (player_id.clone(), player),
            (cache_id.clone(), cache),
        ]);

        assert!(!object_visible_to_player(objects.get(&cache_id).unwrap()));
        let mut dispatch = DispatchStack::default();
        let outcome = run_object_discovery_on_look(
            &mut dispatch,
            &room_id,
            &player_id,
            &mut objects,
            &anatomy,
        );
        assert!(outcome.lines.iter().any(|l| l.contains("supply cache")));
        assert!(outcome.lines.iter().any(|l| l.contains("kneel")));
        assert!(objects
            .get(&cache_id)
            .unwrap()
            .get_bool_property("player_discovered")
            .unwrap_or(false));
        assert!(object_visible_to_player(objects.get(&cache_id).unwrap()));
    }
}