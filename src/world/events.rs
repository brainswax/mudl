//! Core event bus — `@trigger` scripts on places and objects fire through here.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::mudl::{AnatomyRegistry, TriggerDef};
use crate::object::{Behavior, LocationRef, Object, ObjectId, PermissionFlags};
use crate::world::move_manager::MoveResult;

use crate::mudl::trigger_def::events;

use crate::world::event_subscribers::dispatch_event_subscribers;

pub use crate::world::event_script::{
    execute_host_event, execute_script, format_script_line, parse_script, resolve_place_id,
    ScriptAction,
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
    pub dirty: HashSet<ObjectId>,
    /// When true, remaining handlers and phases for this dispatch are skipped.
    pub cancelled: bool,
    /// Non-fatal script/subscriber failures collected for logging and builder diagnostics.
    pub errors: Vec<String>,
}

impl EventOutcome {
    pub fn push_line(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }

    pub fn mark_dirty(&mut self, id: &ObjectId) {
        self.dirty.insert(id.clone());
    }

    pub fn record_error(&mut self, message: impl Into<String>) {
        self.errors.push(message.into());
    }

    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    pub fn append(&mut self, other: EventOutcome) {
        self.lines.extend(other.lines);
        self.dirty.extend(other.dirty);
        self.errors.extend(other.errors);
        if other.cancelled {
            self.cancelled = true;
        }
    }
}

/// Maximum nested `execute_event` depth (discovery → on_discovered → …).
const MAX_DISPATCH_DEPTH: usize = 32;

struct DispatchFrame {
    host_id: ObjectId,
    event_name: String,
}

thread_local! {
    static DISPATCH_STACK: RefCell<Vec<DispatchFrame>> = RefCell::new(Vec::new());
}

struct DispatchGuard;

impl DispatchGuard {
    fn enter(host_id: &ObjectId, event_name: &str) -> Result<Self, EventOutcome> {
        DISPATCH_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            if stack.len() >= MAX_DISPATCH_DEPTH {
                let mut outcome = EventOutcome::default();
                outcome.record_error(format!(
                    "event '{event_name}' on {host_id}: dispatch depth exceeded ({MAX_DISPATCH_DEPTH})"
                ));
                return Err(outcome);
            }
            if stack.iter().any(|frame| {
                frame.host_id == *host_id && frame.event_name == event_name
            }) {
                let mut outcome = EventOutcome::default();
                outcome.record_error(format!(
                    "event '{event_name}' on {host_id}: cycle detected (already in flight)"
                ));
                return Err(outcome);
            }
            stack.push(DispatchFrame {
                host_id: host_id.clone(),
                event_name: event_name.to_string(),
            });
            Ok(Self)
        })
    }
}

impl Drop for DispatchGuard {
    fn drop(&mut self) {
        DISPATCH_STACK.with(|stack| {
            stack.borrow_mut().pop();
        });
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

/// Preview handler output without side effects (builder dry-run).
///
/// Production code should use [`execute_event`] for gates, rooms, and objects.
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

/// Execute `event_name` with full script semantics (react, teleport, spawn, stat mods, …).
///
/// Dispatch order (production):
/// 1. Subscribers — scheduler tick, due `@schedule` jobs, spawner modules
/// 2. Host `@trigger` scripts — registration order; stops early when [`EventOutcome::cancelled`]
pub fn execute_event(
    event_name: &str,
    ctx: &EventContext,
    objects: &mut HashMap<ObjectId, Object>,
    anatomy: Option<&AnatomyRegistry>,
) -> EventOutcome {
    let _guard = match DispatchGuard::enter(&ctx.host_id, event_name) {
        Ok(guard) => guard,
        Err(outcome) => return outcome,
    };

    if !objects
        .get(&ctx.host_id)
        .is_some_and(|host| host.is_active())
    {
        let mut outcome = EventOutcome::default();
        outcome.record_error(format!(
            "event '{event_name}' skipped: host {} is missing or inactive",
            ctx.host_id
        ));
        return outcome;
    }

    let mut outcome = EventOutcome::default();
    outcome.append(dispatch_event_subscribers(
        event_name, ctx, objects, anatomy,
    ));
    if outcome.is_cancelled() {
        return outcome;
    }
    outcome.append(execute_host_event(
        event_name, ctx, objects, anatomy,
    ));
    outcome
}

/// Convenience: run death/kill triggers and loot spawners when a creature is slain.
pub fn execute_kill_events(
    victim_id: &ObjectId,
    killer_id: &ObjectId,
    room_id: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
    anatomy: Option<&AnatomyRegistry>,
) -> EventOutcome {
    let mut outcome = EventOutcome::default();

    let victim_ctx = EventContext {
        actor_id: killer_id.clone(),
        host_id: victim_id.clone(),
        room_id: Some(room_id.clone()),
        target_id: Some(killer_id.clone()),
    };
    outcome.append(execute_event(
        events::ON_DEATH,
        &victim_ctx,
        objects,
        anatomy,
    ));
    outcome.append(execute_event(
        events::ON_KILL,
        &victim_ctx,
        objects,
        anatomy,
    ));

    if killer_id != victim_id {
        let killer_ctx = EventContext {
            actor_id: victim_id.clone(),
            host_id: killer_id.clone(),
            room_id: Some(room_id.clone()),
            target_id: Some(victim_id.clone()),
        };
        outcome.append(execute_event(
            events::ON_KILL,
            &killer_ctx,
            objects,
            anatomy,
        ));
    }

    outcome
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

    #[test]
    fn execute_event_dispatches_loot_spawner_subscribers() {
        use crate::loot::{apply_loot_spawner_def, loot_templates_to_property};
        use crate::mudl::{LootSpawnerDef, LootSpawnerTrigger, LootTemplateDef, SpawnerEntryDef};
        use crate::object::ContainerSpec;

        let player_id = ObjectId::new("player:hero-001");
        let chest_id = ObjectId::new("item:scene-chest-001");
        let mut chest = Object {
            id: chest_id.clone(),
            name: "Travel Chest".to_string(),
            aliases: Vec::new(),
            location: Some(ObjectId::new("area:void-001")),
            prototype: None,
            owner: player_id.clone(),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        chest.apply_container_role(&ContainerSpec::default());

        let templates = [LootTemplateDef {
            base_name: "bonus-rations".to_string(),
            prototype: "trail-rations".to_string(),
            count: 1,
        }];
        let mut spawner = Object {
            id: ObjectId::new("loot-spawner:chest-bonus-001"),
            name: "chest-bonus loot spawner".to_string(),
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
        let template_map = HashMap::from([(
            "bonus-rations".to_string(),
            templates[0].clone(),
        )]);
        apply_loot_spawner_def(
            &mut spawner,
            &LootSpawnerDef {
                base_name: "chest-bonus".to_string(),
                target: "scene-chest".to_string(),
                trigger: LootSpawnerTrigger::OnOpen,
                periodic_interval: 5,
                chance: 1.0,
                max_active: 2,
                once: true,
                entries: vec![SpawnerEntryDef {
                    template: "bonus-rations".to_string(),
                    weight: 1,
                }],
            },
            &template_map,
        )
        .unwrap();
        spawner.set_property_object_ref("loot_spawner_target", chest_id.clone());
        spawner.add_property(loot_templates_to_property(&templates));

        let mut proto = Object {
            id: ObjectId::new("item:trail-rations-001"),
            name: "Trail Rations".to_string(),
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
        proto.set_property_bool("stackable", true);
        proto.set_property_int("stack_count", 3);

        let mut objects = HashMap::from([
            (chest_id.clone(), chest),
            (spawner.id.clone(), spawner),
            (proto.id.clone(), proto),
        ]);

        let outcome = execute_event(
            events::ON_OPEN,
            &EventContext {
                actor_id: player_id.clone(),
                host_id: chest_id,
                room_id: Some(ObjectId::new("area:void-001")),
                target_id: None,
            },
            &mut objects,
            None,
        );
        assert!(
            outcome.lines.iter().any(|l| l.contains("find")),
            "on_open loot subscriber should narrate drop: {:?}",
            outcome.lines
        );
        assert!(!outcome.dirty.is_empty());
    }

    #[test]
    fn stop_cancels_remaining_host_handlers() {
        let player_id = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("area:test-001");
        let mut room = Object {
            id: room_id.clone(),
            name: "Test Room".to_string(),
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
        attach_triggers(
            &mut room,
            &[
                TriggerDef {
                    event: "on_enter".to_string(),
                    code: "narrate First line.".to_string(),
                },
                TriggerDef {
                    event: "on_enter".to_string(),
                    code: "stop".to_string(),
                },
                TriggerDef {
                    event: "on_enter".to_string(),
                    code: "narrate Third line.".to_string(),
                },
            ],
        );

        let mut objects = HashMap::from([(room_id.clone(), room)]);
        let outcome = execute_event(
            "on_enter",
            &EventContext {
                actor_id: player_id,
                host_id: room_id,
                room_id: None,
                target_id: None,
            },
            &mut objects,
            None,
        );
        assert!(outcome.cancelled);
        assert_eq!(outcome.lines.len(), 1);
        assert_eq!(outcome.lines[0], "First line.");
    }

    #[test]
    fn execute_event_records_inactive_host_error() {
        let player_id = ObjectId::new("player:hero-001");
        let host_id = ObjectId::new("item:gone-001");
        let mut objects = HashMap::new();
        let outcome = execute_event(
            "on_open",
            &EventContext {
                actor_id: player_id,
                host_id,
                room_id: None,
                target_id: None,
            },
            &mut objects,
            None,
        );
        assert_eq!(outcome.errors.len(), 1);
        assert!(outcome.errors[0].contains("inactive"));
    }

    #[test]
    fn dispatch_guard_rejects_reentrant_cycle() {
        let player_id = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("area:test-001");
        let room = Object {
            id: room_id.clone(),
            name: "Test Room".to_string(),
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
        let mut objects = HashMap::from([(room_id.clone(), room)]);

        DISPATCH_STACK.with(|stack| {
            stack.borrow_mut().push(DispatchFrame {
                host_id: room_id.clone(),
                event_name: "on_enter".to_string(),
            });
        });

        let outcome = execute_event(
            "on_enter",
            &EventContext {
                actor_id: player_id,
                host_id: room_id,
                room_id: None,
                target_id: None,
            },
            &mut objects,
            None,
        );
        DISPATCH_STACK.with(|stack| stack.borrow_mut().clear());
        assert_eq!(outcome.errors.len(), 1);
        assert!(outcome.errors[0].contains("cycle"));
    }

    #[test]
    fn dispatch_guard_rejects_excessive_depth() {
        let player_id = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("area:test-001");
        let room = Object {
            id: room_id.clone(),
            name: "Test Room".to_string(),
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
        let mut objects = HashMap::from([(room_id.clone(), room)]);

        DISPATCH_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            for i in 0..MAX_DISPATCH_DEPTH {
                stack.push(DispatchFrame {
                    host_id: ObjectId::new(format!("area:depth-{i:03}")),
                    event_name: "on_enter".to_string(),
                });
            }
        });

        let outcome = execute_event(
            "on_enter",
            &EventContext {
                actor_id: player_id,
                host_id: room_id,
                room_id: None,
                target_id: None,
            },
            &mut objects,
            None,
        );
        DISPATCH_STACK.with(|stack| stack.borrow_mut().clear());
        assert_eq!(outcome.errors.len(), 1);
        assert!(outcome.errors[0].contains("depth exceeded"));
    }

    #[test]
    fn missing_event_handlers_is_silent_success() {
        let player_id = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("area:quiet-001");
        let room = Object {
            id: room_id.clone(),
            name: "Quiet Room".to_string(),
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
        let mut objects = HashMap::from([(room_id.clone(), room)]);
        let outcome = execute_event(
            "on_open",
            &EventContext {
                actor_id: player_id,
                host_id: room_id,
                room_id: None,
                target_id: None,
            },
            &mut objects,
            None,
        );
        assert!(outcome.lines.is_empty());
        assert!(outcome.errors.is_empty());
        assert!(!outcome.cancelled);
    }

    #[test]
    fn execute_event_runs_mutating_gate_scripts() {
        use crate::object::ContainerSpec;

        let player_id = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("area:void-001");
        let chest_id = ObjectId::new("item:scene-chest-001");

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
            is_deleted: false,
            deleted_at: None,
        };
        player.init_creature_role(&crate::mudl::PlayerTemplate {
            name: "hero".to_string(),
            creature: "human".to_string(),
            gender: "neutral".to_string(),
        });
        player.set_property_int("health", 80);
        player.set_property_int("max_health", 100);

        let mut chest = Object {
            id: chest_id.clone(),
            name: "Travel Chest".to_string(),
            aliases: Vec::new(),
            location: Some(room_id.clone()),
            prototype: None,
            owner: player_id.clone(),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        chest.apply_container_role(&ContainerSpec::default());
        attach_triggers(
            &mut chest,
            &[TriggerDef {
                event: events::ON_OPEN.to_string(),
                code: "heal 5".to_string(),
            }],
        );

        let mut objects = HashMap::from([
            (player_id.clone(), player),
            (chest_id.clone(), chest),
        ]);

        let outcome = execute_event(
            events::ON_OPEN,
            &EventContext {
                actor_id: player_id.clone(),
                host_id: chest_id,
                room_id: Some(room_id),
                target_id: None,
            },
            &mut objects,
            None,
        );
        assert!(outcome.lines.iter().any(|l| l.contains("recover 5 health")));
        assert_eq!(
            crate::creature::creature_health(objects.get(&player_id).unwrap()),
            85
        );
    }
}