//! Core event bus — `@trigger` scripts on places and objects fire through here.

use std::collections::HashMap;

use crate::mudl::{AnatomyRegistry, TriggerDef};
use crate::object::{Behavior, LocationRef, Object, ObjectId, PermissionFlags};
use crate::world::move_manager::MoveResult;

pub use crate::world::event_script::{
    execute_host_event, execute_kill_events, execute_script, format_script_line, parse_script,
    resolve_place_id, ScriptAction,
};

/// Who did what, where — passed to every emitted event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventContext {
    pub actor_id: ObjectId,
    pub host_id: ObjectId,
    pub room_id: Option<ObjectId>,
    pub target_id: Option<ObjectId>,
}

/// Player-facing lines and touched object IDs from a trigger run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventOutcome {
    pub lines: Vec<String>,
    pub dirty: Vec<ObjectId>,
}

impl EventOutcome {
    pub fn push_line(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }

    pub fn mark_dirty(&mut self, id: &ObjectId) {
        if !self.dirty.iter().any(|d| d == id) {
            self.dirty.push(id.clone());
        }
    }

    pub fn append(&mut self, other: EventOutcome) {
        for line in other.lines {
            self.push_line(line);
        }
        for id in other.dirty {
            self.mark_dirty(&id);
        }
    }
}

/// Attach MUDL `@trigger` definitions to a live object.
pub fn attach_triggers(obj: &mut Object, triggers: &[TriggerDef]) {
    for trigger in triggers {
        obj.add_event_handler(
            trigger.event.clone(),
            Behavior {
                code: trigger.code.clone(),
                permissions: PermissionFlags::EVERYONE,
            },
        );
    }
}

/// Format a single trigger script into player-facing text (read-only, no side effects).
pub fn format_trigger_script(host: &Object, code: &str) -> Option<String> {
    format_script_line(host, &parse_script(code))
}

/// Run all handlers on `host` for `event_name` (read-only narrative formatting).
pub fn run_event_handlers_on(host: &Object, event_name: &str) -> Vec<String> {
    host.event_handlers
        .get(event_name)
        .map(|handlers| {
            handlers
                .iter()
                .filter_map(|behavior| format_trigger_script(host, &behavior.code))
                .collect()
        })
        .unwrap_or_default()
}

/// Emit `event_name` on `ctx.host_id` — narrative-only when `objects` is not mutated.
pub fn emit_event(
    event_name: &str,
    ctx: &EventContext,
    objects: &HashMap<ObjectId, Object>,
) -> EventOutcome {
    let Some(host) = objects.get(&ctx.host_id) else {
        return EventOutcome::default();
    };

    let mut outcome = EventOutcome::default();
    if let Some(handlers) = host.event_handlers.get(event_name) {
        for behavior in handlers {
            if let Some(line) = format_trigger_script(host, &behavior.code) {
                outcome.push_line(line);
            }
        }
    }
    let _ = ctx.actor_id.as_str();
    let _ = ctx.room_id.as_ref();
    let _ = ctx.target_id.as_ref();
    outcome
}

/// Execute `event_name` with full script semantics (react, teleport, spawn, stat mods, …).
pub fn execute_event(
    event_name: &str,
    ctx: &EventContext,
    objects: &mut HashMap<ObjectId, Object>,
    anatomy: Option<&AnatomyRegistry>,
) -> EventOutcome {
    execute_host_event(event_name, ctx, objects, anatomy)
}

/// Resolve the room context for a completed move and fire `on_move` on the moved object.
pub fn emit_on_move_event(
    actor_id: &ObjectId,
    move_result: &MoveResult,
    objects: &mut HashMap<ObjectId, Object>,
    anatomy: Option<&AnatomyRegistry>,
) -> EventOutcome {
    let room_id = room_id_for_location(&move_result.destination, objects);
    let ctx = EventContext {
        actor_id: actor_id.clone(),
        host_id: move_result.object_id.clone(),
        room_id,
        target_id: None,
    };
    execute_event(
        crate::mudl::trigger_def::events::ON_MOVE,
        &ctx,
        objects,
        anatomy,
    )
}

fn room_id_for_location(
    location: &LocationRef,
    objects: &HashMap<ObjectId, Object>,
) -> Option<ObjectId> {
    match location {
        LocationRef::Room(id) => Some(id.clone()),
        LocationRef::Inventory(holder) | LocationRef::Container(holder, _)
        | LocationRef::BodySlot(holder, _) => objects.get(holder).and_then(|holder_obj| {
            if holder_obj.is_location() {
                Some(holder.clone())
            } else {
                holder_obj.location.clone()
            }
        }),
        LocationRef::Nowhere => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn sample_gate() -> Object {
        let mut gate = Object {
            id: ObjectId::new("item:door-001"),
            name: "Wooden Door".to_string(),
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
        attach_triggers(
            &mut gate,
            &[
                TriggerDef {
                    event: "on_unlock".to_string(),
                    code: "narrate The lock clicks free.".to_string(),
                },
                TriggerDef {
                    event: "on_open".to_string(),
                    code: "emote swings open".to_string(),
                },
            ],
        );
        gate
    }

    #[test]
    fn handlers_fire_in_registration_order() {
        let gate = sample_gate();
        assert_eq!(
            run_event_handlers_on(&gate, "on_unlock"),
            vec!["The lock clicks free."]
        );
        assert_eq!(
            run_event_handlers_on(&gate, "on_open"),
            vec!["The wooden door swings open"]
        );
        assert!(run_event_handlers_on(&gate, "on_close").is_empty());
    }

    #[test]
    fn execute_react_flee_moves_npc() {
        let player_id = ObjectId::new("player:hero-001");
        let room_a = ObjectId::new("area:room-a-001");
        let room_b = ObjectId::new("area:room-b-001");
        let npc_id = ObjectId::new("npc:lurker-001");

        let mut room_a_obj = Object {
            id: room_a.clone(),
            name: "Room A".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: player_id.clone(),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        room_a_obj.add_exit("north", room_b.clone());

        let room_b_obj = Object {
            id: room_b.clone(),
            name: "Room B".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: player_id.clone(),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };

        let mut npc = Object {
            id: npc_id.clone(),
            name: "Pale Lurker".to_string(),
            aliases: Vec::new(),
            location: Some(room_a.clone()),
            prototype: None,
            owner: player_id.clone(),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        attach_triggers(
            &mut npc,
            &[TriggerDef {
                event: "on_discovered".to_string(),
                code: "react flee".to_string(),
            }],
        );

        let mut objects = HashMap::from([
            (room_a.clone(), room_a_obj),
            (room_b.clone(), room_b_obj),
            (npc_id.clone(), npc),
        ]);

        let outcome = execute_event(
            "on_discovered",
            &EventContext {
                actor_id: player_id,
                host_id: npc_id.clone(),
                room_id: Some(room_a),
                target_id: None,
            },
            &mut objects,
            None,
        );
        assert!(outcome.lines.iter().any(|l| l.contains("bolts away")));
        assert_ne!(
            objects.get(&npc_id).unwrap().location.as_ref(),
            Some(&ObjectId::new("area:room-a-001"))
        );
    }
}