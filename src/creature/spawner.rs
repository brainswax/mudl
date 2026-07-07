//! Location-attached creature spawners with weighted templates.

use std::collections::HashMap;

use crate::creature::vitality::DEFAULT_MAX_HEALTH;
use crate::creature::{bootstrap_creature_behavior_system, init_creature_vitality, resolve_behavior_templates};
use crate::mudl::PlayerTemplate;
use crate::mudl::{
    AnatomyRegistry, NpcBehaviorDef, SpawnTemplateDef, SpawnerDef, SpawnerEntryDef, SpawnerTrigger,
};
use crate::object::{generate_object_id, Object, ObjectId, PermissionFlags, Property, Value};


/// Result of a spawner tick — optional narrative feedback for the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnResult {
    pub npc_id: ObjectId,
    pub message: Option<String>,
}

/// Whether `obj` is a location spawner (hidden from room listings).
pub fn is_spawner(obj: &Object) -> bool {
    obj.get_bool_property("is_spawner").unwrap_or(false)
}

/// Whether `obj` should be hidden from player room listings.
pub fn is_spawner_infrastructure(obj: &Object) -> bool {
    is_spawner(obj)
}

fn spawner_target_id(obj: &Object) -> Option<ObjectId> {
    obj.get_object_ref_property("spawner_target")
}

/// Creature spawners whose `target` resolves to `target_id`.
pub fn spawners_for_target<'a>(
    target_id: &ObjectId,
    objects: &'a HashMap<ObjectId, Object>,
) -> Vec<&'a Object> {
    objects
        .values()
        .filter(|obj| {
            obj.is_active() && is_spawner(obj) && spawner_target_id(obj).as_ref() == Some(target_id)
        })
        .collect()
}

/// Resolve the room where creatures from this spawner should appear.
pub fn spawner_room_id(spawner: &Object, objects: &HashMap<ObjectId, Object>) -> Option<ObjectId> {
    if let Some(room_id) = spawner.location.clone() {
        return Some(room_id);
    }
    spawner_target_id(spawner).and_then(|target_id| {
        objects
            .get(&target_id)
            .filter(|target| target.is_active())
            .and_then(|target| target.location.clone())
    })
}

/// Spawners active in `room_id` — room-attached and object-attached (via targets in the room).
pub fn spawners_in_room<'a>(
    room_id: &ObjectId,
    objects: &'a HashMap<ObjectId, Object>,
) -> Vec<&'a Object> {
    objects
        .values()
        .filter(|obj| {
            if !obj.is_active() || !is_spawner(obj) {
                return false;
            }
            if obj.location.as_ref() == Some(room_id) {
                return true;
            }
            spawner_target_id(obj).is_some_and(|target_id| {
                objects.get(&target_id).is_some_and(|target| {
                    target.is_active() && target.location.as_ref() == Some(room_id)
                })
            })
        })
        .collect()
}

/// Active NPCs in `room_id` spawned by `spawner_id`.
pub fn count_active_spawns(
    spawner_id: &ObjectId,
    room_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> usize {
    objects
        .values()
        .filter(|obj| {
            obj.is_active()
                && obj.object_type() == "npc"
                && obj.location.as_ref() == Some(room_id)
                && obj
                    .get_property("spawned_by")
                    .and_then(|p| {
                        if let Value::ObjectRef(id) = &p.value {
                            Some(id == spawner_id)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(false)
        })
        .count()
}

fn spawner_trigger(obj: &Object) -> SpawnerTrigger {
    obj.get_property("spawner_trigger")
        .and_then(|p| {
            if let Value::String(s) = &p.value {
                Some(SpawnerTrigger::parse(s))
            } else {
                None
            }
        })
        .unwrap_or(SpawnerTrigger::OnEnter)
}

fn spawner_chance(obj: &Object) -> f64 {
    obj.get_float_property("spawner_chance")
        .unwrap_or(1.0)
        .clamp(0.0, 1.0)
}

fn spawner_max_active(obj: &Object) -> u32 {
    obj.get_int_property("spawner_max_active")
        .unwrap_or(1)
        .max(0) as u32
}

fn spawner_periodic_interval(obj: &Object) -> u32 {
    obj.get_int_property("spawner_periodic_interval")
        .unwrap_or(5)
        .max(1) as u32
}

fn spawner_enter_count(obj: &Object) -> u64 {
    obj.get_int_property("spawner_enter_count")
        .unwrap_or(0)
        .max(0) as u64
}

fn spawner_spawn_count(obj: &Object) -> u32 {
    obj.get_int_property("spawner_spawn_count")
        .unwrap_or(0)
        .max(0) as u32
}

fn set_spawner_enter_count(spawner: &mut Object, count: u64) {
    spawner.set_property_int("spawner_enter_count", count as i64);
}

fn set_spawner_spawn_count(spawner: &mut Object, count: u32) {
    spawner.set_property_int("spawner_spawn_count", count as i64);
}

/// Parse weighted entries stored on a spawner object.
pub fn spawner_entries(obj: &Object) -> Vec<SpawnerEntryDef> {
    obj.get_property("spawner_entries")
        .and_then(|prop| {
            if let Value::List(items) = &prop.value {
                Some(
                    items
                        .iter()
                        .filter_map(|entry| {
                            let Value::Map(map) = entry else {
                                return None;
                            };
                            let template = map.get("template").and_then(|v| {
                                if let Value::String(s) = v {
                                    Some(s.clone())
                                } else {
                                    None
                                }
                            })?;
                            let weight = map
                                .get("weight")
                                .and_then(|v| match v {
                                    Value::Int(n) => Some(*n as u32),
                                    _ => None,
                                })
                                .unwrap_or(1);
                            Some(SpawnerEntryDef { template, weight })
                        })
                        .collect(),
                )
            } else {
                None
            }
        })
        .unwrap_or_default()
}

/// Deterministic weighted pick from `entries` using `seed`.
pub fn pick_weighted_entry(entries: &[SpawnerEntryDef], seed: u64) -> Option<&SpawnerEntryDef> {
    if entries.is_empty() {
        return None;
    }
    let total: u32 = entries.iter().map(|e| e.weight.max(1)).sum();
    if total == 0 {
        return None;
    }
    let roll = (seed % u64::from(total)) as u32;
    let mut cursor = 0u32;
    for entry in entries {
        cursor += entry.weight.max(1);
        if roll < cursor {
            return Some(entry);
        }
    }
    entries.last()
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

fn chance_rolls(seed: u64, chance: f64) -> bool {
    if chance >= 1.0 {
        return true;
    }
    if chance <= 0.0 {
        return false;
    }
    let threshold = (chance * 10_000.0).round() as u64;
    seed % 10_000 < threshold
}

fn trigger_fires(spawner: &Object, enter_count: u64) -> bool {
    match spawner_trigger(spawner) {
        SpawnerTrigger::OnEnter => true,
        SpawnerTrigger::Periodic => {
            let interval = u64::from(spawner_periodic_interval(spawner));
            enter_count.is_multiple_of(interval)
        }
    }
}

/// Build spawner runtime properties from a MUDL definition and template lookup.
pub fn apply_spawner_def(
    spawner: &mut Object,
    def: &SpawnerDef,
    templates: &HashMap<String, SpawnTemplateDef>,
) -> anyhow::Result<()> {
    spawner.set_property_bool("is_spawner", true);
    spawner.set_property_string("spawner_base", &def.base_name);
    spawner.set_property_string("spawner_trigger", def.trigger.as_str());
    spawner.set_property_int(
        "spawner_periodic_interval",
        i64::from(def.periodic_interval),
    );
    spawner.set_property_numeric("spawner_chance", def.chance);
    spawner.set_property_int("spawner_max_active", i64::from(def.max_active));
    spawner.set_property_int("spawner_enter_count", 0);
    spawner.set_property_int("spawner_spawn_count", 0);

    let entry_values: Vec<Value> = def
        .entries
        .iter()
        .map(|entry| {
            if !templates.contains_key(&entry.template) {
                anyhow::bail!(
                    "Spawner '{}' references unknown spawn-template '{}'",
                    def.base_name,
                    entry.template
                );
            }
            Ok(Value::Map(HashMap::from([
                (
                    "template".to_string(),
                    Value::String(entry.template.clone()),
                ),
                (
                    "weight".to_string(),
                    Value::Int(i64::from(entry.weight.max(1))),
                ),
            ])))
        })
        .collect::<anyhow::Result<_>>()?;

    spawner.add_property(Property {
        name: "spawner_entries".to_string(),
        value: Value::List(entry_values),
        permissions: PermissionFlags::EVERYONE,
        behavior: None,
    });
    Ok(())
}

/// Serialize spawn templates for lookup at runtime (stored on spawner owner / world).
pub fn spawn_templates_to_property(templates: &[SpawnTemplateDef]) -> Property {
    let items: Vec<Value> = templates
        .iter()
        .map(|template| {
            let behaviors: Vec<Value> = template
                .behaviors
                .iter()
                .map(|behavior| {
                    Value::Map(HashMap::from([
                        ("event".to_string(), Value::String(behavior.event.clone())),
                        ("action".to_string(), Value::String(behavior.action.clone())),
                        ("text".to_string(), Value::String(behavior.text.clone())),
                    ]))
                })
                .collect();
            let use_behaviors: Vec<Value> = template
                .use_behaviors
                .iter()
                .map(|name| Value::String(name.clone()))
                .collect();
            let triggers: Vec<Value> = template
                .triggers
                .iter()
                .map(|trigger| {
                    Value::Map(HashMap::from([
                        ("event".to_string(), Value::String(trigger.event.clone())),
                        ("code".to_string(), Value::String(trigger.code.clone())),
                    ]))
                })
                .collect();
            Value::Map(HashMap::from([
                (
                    "base_name".to_string(),
                    Value::String(template.base_name.clone()),
                ),
                (
                    "name".to_string(),
                    Value::String(
                        template
                            .name
                            .clone()
                            .unwrap_or_else(|| template.base_name.clone()),
                    ),
                ),
                (
                    "creature".to_string(),
                    Value::String(template.creature.clone()),
                ),
                ("behaviors".to_string(), Value::List(behaviors)),
                ("use_behaviors".to_string(), Value::List(use_behaviors)),
                ("triggers".to_string(), Value::List(triggers)),
            ]))
        })
        .collect();
    Property {
        name: "spawn_templates".to_string(),
        value: Value::List(items),
        permissions: PermissionFlags::EVERYONE,
        behavior: None,
    }
}

/// Look up a spawn template by name on a spawner object.
pub fn resolve_spawn_template(template_name: &str, spawner: &Object) -> Option<SpawnTemplateDef> {
    spawner.get_property("spawn_templates").and_then(|prop| {
        if let Value::List(items) = &prop.value {
            items.iter().find_map(|entry| {
                let Value::Map(map) = entry else {
                    return None;
                };
                let base = map.get("base_name").and_then(|v| {
                    if let Value::String(s) = v {
                        Some(s.as_str())
                    } else {
                        None
                    }
                })?;
                if base != template_name {
                    return None;
                }
                let name = map.get("name").and_then(|v| {
                    if let Value::String(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                });
                let creature = map
                    .get("creature")
                    .and_then(|v| {
                        if let Value::String(s) = v {
                            Some(s.clone())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| "human".to_string());
                let behaviors: Vec<NpcBehaviorDef> = map
                    .get("behaviors")
                    .and_then(|v| {
                        if let Value::List(list) = v {
                            Some(
                                list.iter()
                                    .filter_map(|b| {
                                        let Value::Map(bmap) = b else {
                                            return None;
                                        };
                                        Some(NpcBehaviorDef {
                                            event: bmap.get("event")?.as_string()?,
                                            action: bmap.get("action")?.as_string()?,
                                            text: bmap.get("text")?.as_string()?,
                                            react: bmap.get("react").and_then(|v| {
                                                v.as_string()
                                                    .map(|s| crate::mudl::CreatureReact::parse(&s))
                                            }),
                                        })
                                    })
                                    .collect(),
                            )
                        } else {
                            None
                        }
                    })
                    .unwrap_or_default();
                let use_behaviors: Vec<String> = map
                    .get("use_behaviors")
                    .and_then(|v| {
                        if let Value::List(list) = v {
                            Some(
                                list.iter()
                                    .filter_map(|item| {
                                        if let Value::String(s) = item {
                                            Some(s.clone())
                                        } else {
                                            None
                                        }
                                    })
                                    .collect(),
                            )
                        } else {
                            None
                        }
                    })
                    .unwrap_or_default();
                let triggers: Vec<crate::mudl::TriggerDef> = map
                    .get("triggers")
                    .and_then(|v| {
                        if let Value::List(list) = v {
                            Some(
                                list.iter()
                                    .filter_map(|entry| {
                                        let Value::Map(tmap) = entry else {
                                            return None;
                                        };
                                        Some(crate::mudl::TriggerDef {
                                            event: tmap.get("event")?.as_string()?,
                                            code: tmap.get("code")?.as_string()?,
                                        })
                                    })
                                    .collect(),
                            )
                        } else {
                            None
                        }
                    })
                    .unwrap_or_default();
                Some(SpawnTemplateDef {
                    base_name: base.to_string(),
                    name,
                    creature,
                    behaviors,
                    use_behaviors,
                    triggers,
                })
            })
        } else {
            None
        }
    })
}

trait ValueStringExt {
    fn as_string(&self) -> Option<String>;
}

impl ValueStringExt for Value {
    fn as_string(&self) -> Option<String> {
        if let Value::String(s) = self {
            Some(s.clone())
        } else {
            None
        }
    }
}

/// Create a spawned NPC in `room_id` from a template.
pub fn spawn_creature(
    spawner: &Object,
    template: &SpawnTemplateDef,
    room_id: &ObjectId,
    owner: &ObjectId,
    anatomy: &AnatomyRegistry,
    spawn_index: u32,
) -> Object {
    let base = spawner
        .get_property("spawner_base")
        .and_then(|p| {
            if let Value::String(s) = &p.value {
                Some(s.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "spawn".to_string());
    let id = generate_object_id("npc", &format!("{base}-spawn"), spawn_index.max(1));
    let display_name = template
        .name
        .clone()
        .unwrap_or_else(|| template.base_name.clone());

    let mut npc = Object {
        id,
        name: display_name.clone(),
        aliases: Vec::new(),
        location: Some(room_id.clone()),
        prototype: None,
        owner: owner.clone(),
        permissions: PermissionFlags::EVERYONE,
        properties: HashMap::new(),
        verbs: HashMap::new(),
        event_handlers: HashMap::new(),
        is_deleted: false,
        deleted_at: None,
    };

    let player_template = PlayerTemplate {
        name: template.base_name.clone(),
        creature: template.creature.clone(),
        gender: "neutral".to_string(),
    };
    npc.init_creature_role(&player_template);
    if let Some(creature_def) = anatomy.creature(&template.creature) {
        init_creature_vitality(&mut npc, creature_def);
    } else {
        npc.set_property_int("health", DEFAULT_MAX_HEALTH);
        npc.set_property_int("max_health", DEFAULT_MAX_HEALTH);
    }

    npc.add_property(Property {
        name: "spawned_by".to_string(),
        value: Value::ObjectRef(spawner.id.clone()),
        permissions: PermissionFlags::EVERYONE,
        behavior: None,
    });
    npc.set_property_string("spawn_template", &template.base_name);

    let behavior_templates = resolve_behavior_templates(spawner);
    bootstrap_creature_behavior_system(
        &mut npc,
        &template.behaviors,
        &template.use_behaviors,
        &behavior_templates,
        &template.triggers,
    );

    npc
}

/// Find a spawn template by name on any spawner in the world graph.
pub fn find_spawn_template_in_world(
    template_name: &str,
    objects: &HashMap<ObjectId, Object>,
) -> Option<(SpawnTemplateDef, Object)> {
    for spawner in objects.values().filter(|o| o.is_active() && is_spawner(o)) {
        if let Some(template) = resolve_spawn_template(template_name, spawner) {
            return Some((template, spawner.clone()));
        }
    }
    None
}

/// Spawn an NPC from a named template into `room_id` and insert it into `objects`.
pub fn spawn_creature_from_template(
    template_name: &str,
    room_id: &ObjectId,
    owner: &ObjectId,
    anatomy: &AnatomyRegistry,
    objects: &mut HashMap<ObjectId, Object>,
) -> Option<(Object, Option<String>)> {
    let (template, spawner) = find_spawn_template_in_world(template_name, objects)?;
    let spawn_index = objects
        .values()
        .filter(|o| {
            o.is_active()
                && o.object_type() == "npc"
                && o.get_object_ref_property("spawned_by").as_ref() == Some(&spawner.id)
        })
        .count() as u32
        + 1;
    let npc = spawn_creature(
        &spawner,
        &template,
        room_id,
        owner,
        anatomy,
        spawn_index,
    );
    let message = spawn_message(&template);
    let npc_id = npc.id.clone();
    objects.insert(npc_id, npc.clone());
    Some((npc, Some(message)))
}

fn spawn_message(template: &SpawnTemplateDef) -> String {
    format!(
        "A {} materializes nearby.",
        template
            .name
            .clone()
            .unwrap_or_else(|| template.base_name.clone())
            .to_lowercase()
    )
}

/// Soft-delete NPCs spawned by `spawner_id`.
pub fn despawn_creatures_from_spawner(
    spawner_id: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
) -> Vec<ObjectId> {
    let npc_ids: Vec<ObjectId> = objects
        .values()
        .filter(|obj| {
            obj.is_active()
                && obj.object_type() == "npc"
                && obj.get_object_ref_property("spawned_by").as_ref() == Some(spawner_id)
        })
        .map(|obj| obj.id.clone())
        .collect();
    for npc_id in &npc_ids {
        if let Some(npc) = objects.get_mut(npc_id) {
            npc.soft_delete();
        }
    }
    npc_ids
}

/// Destroy creature spawners attached to `target_id` and despawn their active creatures.
pub fn destroy_spawners_for_target(
    target_id: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
) -> Vec<ObjectId> {
    let spawner_ids: Vec<ObjectId> = spawners_for_target(target_id, objects)
        .into_iter()
        .map(|spawner| spawner.id.clone())
        .collect();
    for spawner_id in &spawner_ids {
        despawn_creatures_from_spawner(spawner_id, objects);
        if let Some(spawner) = objects.get_mut(spawner_id) {
            spawner.soft_delete();
        }
    }
    spawner_ids
}

/// Dispatch creature spawners subscribed to `event_name` on `host_id` (room enter only today).
pub fn dispatch_creature_spawners_for_event(
    event_name: &str,
    host_id: &ObjectId,
    player_id: &ObjectId,
    owner: &ObjectId,
    anatomy: &AnatomyRegistry,
    objects: &mut HashMap<ObjectId, Object>,
) -> crate::world::EventOutcome {
    use crate::world::EventOutcome;

    if event_name != "on_enter" {
        return EventOutcome::default();
    }

    let mut outcome = EventOutcome::default();
    for spawn in run_on_enter_spawners(host_id, player_id, owner, anatomy, objects) {
        outcome.mark_dirty(&spawn.npc_id);
        if let Some(message) = spawn.message {
            outcome.push_line(message);
        }
    }
    outcome
}

/// Run `on_enter` spawners in `room_id`. Only locations with spawner objects spawn creatures.
pub fn run_on_enter_spawners(
    room_id: &ObjectId,
    player_id: &ObjectId,
    owner: &ObjectId,
    anatomy: &AnatomyRegistry,
    objects: &mut HashMap<ObjectId, Object>,
) -> Vec<SpawnResult> {
    let spawner_ids: Vec<ObjectId> = spawners_in_room(room_id, objects)
        .into_iter()
        .map(|s| s.id.clone())
        .collect();

    let mut results = Vec::new();
    for spawner_id in spawner_ids {
        let Some(spawner_snapshot) = objects.get(&spawner_id).cloned() else {
            continue;
        };
        let Some(spawn_room_id) = spawner_room_id(&spawner_snapshot, objects) else {
            continue;
        };
        let enter_count = spawner_enter_count(&spawner_snapshot) + 1;
        if let Some(spawner) = objects.get_mut(&spawner_id) {
            set_spawner_enter_count(spawner, enter_count);
        }

        if !trigger_fires(&spawner_snapshot, enter_count) {
            continue;
        }

        let chance = spawner_chance(&spawner_snapshot);
        let chance_seed = mix_seed(&[
            spawner_id.as_str(),
            player_id.as_str(),
            &enter_count.to_string(),
            "chance",
        ]);
        if !chance_rolls(chance_seed, chance) {
            continue;
        }

        let max_active = spawner_max_active(&spawner_snapshot);
        if count_active_spawns(&spawner_id, &spawn_room_id, objects) >= max_active as usize {
            continue;
        }

        let entries = spawner_entries(&spawner_snapshot);
        let pick_seed = mix_seed(&[
            spawner_id.as_str(),
            player_id.as_str(),
            &enter_count.to_string(),
            "pick",
        ]);
        let Some(entry) = pick_weighted_entry(&entries, pick_seed) else {
            continue;
        };
        let Some(template) = resolve_spawn_template(&entry.template, &spawner_snapshot) else {
            continue;
        };

        let spawn_index = spawner_spawn_count(&spawner_snapshot) + 1;
        let npc = spawn_creature(
            &spawner_snapshot,
            &template,
            &spawn_room_id,
            owner,
            anatomy,
            spawn_index,
        );
        let message = spawn_message(&template);
        let npc_id = npc.id.clone();
        objects.insert(npc_id.clone(), npc);

        if let Some(spawner) = objects.get_mut(&spawner_id) {
            set_spawner_spawn_count(spawner, spawn_index);
        }

        results.push(SpawnResult {
            npc_id,
            message: Some(message),
        });
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mudl::{AnatomyRegistry, NpcBehaviorDef, SpawnTemplateDef, SpawnerDef};

    fn sample_templates() -> HashMap<String, SpawnTemplateDef> {
        HashMap::from([(
            "mist-wisp".to_string(),
            SpawnTemplateDef {
                base_name: "mist-wisp".to_string(),
                name: Some("Mist Wisp".to_string()),
                creature: "human".to_string(),
                behaviors: vec![NpcBehaviorDef {
                    event: "on_enter".to_string(),
                    action: "emote".to_string(),
                    text: "drifts.".to_string(),
                    react: None,
                }],
                use_behaviors: vec![],
                triggers: vec![],
            },
        )])
    }

    fn spawner_object(room: &ObjectId) -> Object {
        let mut spawner = Object {
            id: ObjectId::new("spawner:test-001"),
            name: "test spawner".to_string(),
            aliases: Vec::new(),
            location: Some(room.clone()),
            prototype: None,
            owner: ObjectId::new("player:admin-001"),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        apply_spawner_def(
            &mut spawner,
            &SpawnerDef {
                base_name: "test".to_string(),
                location: "room".to_string(),
                target: String::new(),
                trigger: SpawnerTrigger::OnEnter,
                periodic_interval: 5,
                chance: 1.0,
                max_active: 1,
                entries: vec![SpawnerEntryDef {
                    template: "mist-wisp".to_string(),
                    weight: 1,
                }],
            },
            &sample_templates(),
        )
        .unwrap();
        let templates: Vec<SpawnTemplateDef> = sample_templates().into_values().collect();
        spawner.add_property(spawn_templates_to_property(&templates));
        spawner
    }

    #[test]
    fn weighted_pick_is_deterministic() {
        let entries = vec![
            SpawnerEntryDef {
                template: "a".to_string(),
                weight: 3,
            },
            SpawnerEntryDef {
                template: "b".to_string(),
                weight: 1,
            },
        ];
        let first = pick_weighted_entry(&entries, 42).unwrap().template.as_str();
        let second = pick_weighted_entry(&entries, 42).unwrap().template.as_str();
        assert_eq!(first, second);
    }

    #[test]
    fn on_enter_spawner_creates_npc_only_when_present() {
        let room = ObjectId::new("area:haunted-moon-001");
        let player = ObjectId::new("player:hero-001");
        let owner = ObjectId::new("player:admin-001");
        let anatomy = AnatomyRegistry::default();
        let mut objects = HashMap::from([(
            room.clone(),
            Object {
                id: room.clone(),
                name: "Moonlit Glade".to_string(),
                aliases: Vec::new(),
                location: None,
                prototype: None,
                owner: owner.clone(),
                permissions: PermissionFlags::EVERYONE,
                properties: HashMap::new(),
                verbs: HashMap::new(),
                event_handlers: HashMap::new(),
                is_deleted: false,
                deleted_at: None,
            },
        )]);

        let empty = run_on_enter_spawners(&room, &player, &owner, &anatomy, &mut objects);
        assert!(empty.is_empty());

        let spawner = spawner_object(&room);
        objects.insert(spawner.id.clone(), spawner);
        let spawned = run_on_enter_spawners(&room, &player, &owner, &anatomy, &mut objects);
        assert_eq!(spawned.len(), 1);
        assert!(objects
            .values()
            .any(|o| o.object_type() == "npc" && o.name == "Mist Wisp"));
    }

    #[test]
    fn max_active_prevents_extra_spawns() {
        let room = ObjectId::new("area:haunted-moon-001");
        let player = ObjectId::new("player:hero-001");
        let owner = ObjectId::new("player:admin-001");
        let anatomy = AnatomyRegistry::default();
        let mut spawner = spawner_object(&room);
        spawner.set_property_int("spawner_max_active", 1);
        let mut objects = HashMap::from([
            (
                room.clone(),
                Object {
                    id: room.clone(),
                    name: "room".to_string(),
                    aliases: Vec::new(),
                    location: None,
                    prototype: None,
                    owner: owner.clone(),
                    permissions: PermissionFlags::EVERYONE,
                    properties: HashMap::new(),
                    verbs: HashMap::new(),
                    event_handlers: HashMap::new(),
                    is_deleted: false,
                    deleted_at: None,
                },
            ),
            (spawner.id.clone(), spawner),
        ]);

        let first = run_on_enter_spawners(&room, &player, &owner, &anatomy, &mut objects);
        assert_eq!(first.len(), 1);
        let second = run_on_enter_spawners(&room, &player, &owner, &anatomy, &mut objects);
        assert!(second.is_empty());
    }
}
