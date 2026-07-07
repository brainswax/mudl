//! Basic NPC behaviors triggered by simple room events.

use std::collections::HashMap;

use crate::mudl::NpcBehaviorDef;
use crate::object::{Object, ObjectId};

/// Parsed behavior action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NpcAction {
    Say(String),
    SayTo(String),
    Emote(String),
}

/// A behavior attached to an NPC for a specific event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpcBehavior {
    pub event: String,
    pub action: NpcAction,
}

impl NpcBehavior {
    pub fn from_def(def: &NpcBehaviorDef) -> Option<Self> {
        let action = match def.action.as_str() {
            "say" => NpcAction::Say(def.text.clone()),
            "say_to" => NpcAction::SayTo(def.text.clone()),
            "emote" => NpcAction::Emote(def.text.clone()),
            _ => return None,
        };
        Some(Self {
            event: def.event.clone(),
            action,
        })
    }
}

/// Read scripted behaviors stored on an NPC object (`npc_behaviors` property).
pub fn npc_behaviors(npc: &Object) -> Vec<NpcBehavior> {
    npc.get_property("npc_behaviors")
        .and_then(|prop| {
            if let crate::object::Value::List(items) = &prop.value {
                Some(
                    items
                        .iter()
                        .filter_map(|entry| {
                            let crate::object::Value::Map(map) = entry else {
                                return None;
                            };
                            let event = map.get("event").and_then(|v| {
                                if let crate::object::Value::String(s) = v {
                                    Some(s.clone())
                                } else {
                                    None
                                }
                            })?;
                            let action = map.get("action").and_then(|v| {
                                if let crate::object::Value::String(s) = v {
                                    Some(s.clone())
                                } else {
                                    None
                                }
                            })?;
                            let text = map.get("text").and_then(|v| {
                                if let crate::object::Value::String(s) = v {
                                    Some(s.clone())
                                } else {
                                    None
                                }
                            })?;
                            NpcBehavior::from_def(&NpcBehaviorDef {
                                event,
                                action,
                                text,
                                react: map.get("react").and_then(|v| {
                                    if let crate::object::Value::String(s) = v {
                                        Some(crate::mudl::CreatureReact::parse(s))
                                    } else {
                                        None
                                    }
                                }),
                            })
                        })
                        .collect(),
                )
            } else {
                None
            }
        })
        .unwrap_or_default()
}

/// NPCs present in a room (excluding the entering player).
pub fn npcs_in_room<'a>(
    room_id: &ObjectId,
    player_id: &ObjectId,
    objects: &'a HashMap<ObjectId, Object>,
) -> Vec<&'a Object> {
    objects
        .values()
        .filter(|obj| {
            obj.is_active()
                && obj.id != *player_id
                && obj.object_type() == "npc"
                && obj.location.as_ref() == Some(room_id)
        })
        .collect()
}

/// Run `on_enter` behaviors for NPCs in `room_id` and return player-facing lines.
///
/// Prefer [`crate::creature::run_on_enter_creature_behaviors`] when mutation (attack/flee) is needed.
pub fn run_on_enter_behaviors(
    room_id: &ObjectId,
    player_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &crate::mudl::AnatomyRegistry,
) -> Vec<String> {
    let mut objects = objects.clone();
    crate::creature::run_on_enter_creature_behaviors(room_id, player_id, &mut objects, anatomy)
        .lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{PermissionFlags, Property, Value};

    fn npc_with_behaviors() -> Object {
        let mut npc = Object {
            id: ObjectId::new("npc:watcher-001"),
            name: "Path Watcher".to_string(),
            aliases: Vec::new(),
            location: Some(ObjectId::new("area:forest-path-001")),
            prototype: None,
            owner: ObjectId::new("player:admin-001"),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        npc.init_creature_role(&crate::mudl::PlayerTemplate {
            name: "watcher".to_string(),
            creature: "human".to_string(),
            gender: "neutral".to_string(),
        });
        npc.add_property(Property {
            name: "npc_behaviors".to_string(),
            value: Value::List(vec![Value::Map(HashMap::from([
                ("event".to_string(), Value::String("on_enter".to_string())),
                ("action".to_string(), Value::String("say".to_string())),
                (
                    "text".to_string(),
                    Value::String("The trees seem to lean closer.".to_string()),
                ),
            ]))]),
            permissions: PermissionFlags::EVERYONE,
            behavior: None,
        });
        npc
    }

    #[test]
    fn on_enter_behavior_formats_speech() {
        let room = ObjectId::new("area:forest-path-001");
        let player = ObjectId::new("player:hero-001");
        let npc = npc_with_behaviors();
        let mut objects = HashMap::from([(npc.id.clone(), npc)]);
        let lines = run_on_enter_behaviors(&room, &player, &objects, &crate::mudl::AnatomyRegistry::default());
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("Path Watcher"));
        assert!(lines[0].contains("trees seem to lean closer"));

        let player_obj = Object {
            id: player.clone(),
            name: "Hero".to_string(),
            aliases: Vec::new(),
            location: Some(room.clone()),
            prototype: None,
            owner: player.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        let player_id = player_obj.id.clone();
        objects.insert(player_id.clone(), player_obj);
        assert_eq!(
            run_on_enter_behaviors(&room, &player_id, &objects, &crate::mudl::AnatomyRegistry::default()).len(),
            1
        );
    }
}
