//! Location- and object-attached resource spawners with weighted item templates.

use std::collections::HashMap;

use crate::creature::spawner::pick_weighted_entry;
use crate::mudl::{
    ResourceSpawnerDef, ResourceSpawnerTrigger, ResourceTemplateDef, SpawnerEntryDef,
};
use crate::object::{generate_object_id, Object, ObjectId, PermissionFlags, Property, Value};
use crate::world::scheduler::periodic_fires;

/// Result of a resource spawner tick — narrative feedback for the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceSpawnResult {
    pub item_id: ObjectId,
    pub message: Option<String>,
}

/// Whether `obj` is a resource spawner (hidden from room listings).
pub fn is_resource_spawner(obj: &Object) -> bool {
    obj.get_bool_property("is_resource_spawner")
        .unwrap_or(false)
}

/// Whether `obj` should be hidden from player room listings.
pub fn is_resource_spawner_infrastructure(obj: &Object) -> bool {
    is_resource_spawner(obj)
}

fn resource_spawner_target_id(obj: &Object) -> Option<ObjectId> {
    obj.get_object_ref_property("resource_spawner_target")
}

fn resource_spawner_trigger(obj: &Object) -> ResourceSpawnerTrigger {
    obj.get_property("resource_spawner_trigger")
        .and_then(|p| {
            if let Value::String(s) = &p.value {
                Some(ResourceSpawnerTrigger::parse(s))
            } else {
                None
            }
        })
        .unwrap_or(ResourceSpawnerTrigger::OnEnter)
}

fn resource_spawner_chance(obj: &Object) -> f64 {
    obj.get_float_property("resource_spawner_chance")
        .unwrap_or(1.0)
        .clamp(0.0, 1.0)
}

fn resource_spawner_max_active(obj: &Object) -> u32 {
    obj.get_int_property("resource_spawner_max_active")
        .unwrap_or(4)
        .max(0) as u32
}

fn resource_spawner_periodic_interval(obj: &Object) -> u32 {
    obj.get_int_property("resource_spawner_periodic_interval")
        .unwrap_or(5)
        .max(1) as u32
}

fn resource_spawner_spawn_count(obj: &Object) -> u32 {
    obj.get_int_property("resource_spawner_spawn_count")
        .unwrap_or(0)
        .max(0) as u32
}

fn resource_spawner_once(obj: &Object) -> bool {
    obj.get_bool_property("resource_spawner_once")
        .unwrap_or(false)
}

fn resource_spawner_fired(obj: &Object) -> bool {
    obj.get_bool_property("resource_spawner_fired")
        .unwrap_or(false)
}

fn set_resource_spawner_spawn_count(spawner: &mut Object, count: u32) {
    spawner.set_property_int("resource_spawner_spawn_count", count as i64);
}

fn set_resource_spawner_fired(spawner: &mut Object, fired: bool) {
    spawner.set_property_bool("resource_spawner_fired", fired);
}

/// Parse weighted entries stored on a resource spawner object.
pub fn resource_spawner_entries(obj: &Object) -> Vec<SpawnerEntryDef> {
    obj.get_property("resource_spawner_entries")
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

/// Resource spawners whose `target` resolves to `target_id`.
pub fn resource_spawners_for_target<'a>(
    target_id: &ObjectId,
    objects: &'a HashMap<ObjectId, Object>,
) -> Vec<&'a Object> {
    objects
        .values()
        .filter(|obj| {
            obj.is_active()
                && is_resource_spawner(obj)
                && resource_spawner_target_id(obj).as_ref() == Some(target_id)
        })
        .collect()
}

/// Resource spawners attached to a room (target is the room itself).
fn resource_spawners_in_room<'a>(
    room_id: &ObjectId,
    objects: &'a HashMap<ObjectId, Object>,
) -> Vec<&'a Object> {
    resource_spawners_for_target(room_id, objects)
}

/// Active resource items spawned by `spawner_id` at `target_id`.
pub fn count_active_resources(
    spawner_id: &ObjectId,
    target_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> usize {
    let room_id = room_for_target(target_id, objects);

    objects
        .values()
        .filter(|obj| {
            if !obj.is_active() || obj.object_type() != "item" {
                return false;
            }
            let spawned_by = obj.get_object_ref_property("resource_spawned_by");
            if spawned_by.as_ref() != Some(spawner_id) {
                return false;
            }
            match room_id.as_ref() {
                Some(room) => obj.location.as_ref() == Some(room),
                None => false,
            }
        })
        .count()
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

fn trigger_fires(
    spawner: &Object,
    trigger: ResourceSpawnerTrigger,
    tick: u64,
) -> bool {
    let actual = resource_spawner_trigger(spawner);
    if actual != trigger {
        return false;
    }
    match actual {
        ResourceSpawnerTrigger::OnEnter | ResourceSpawnerTrigger::OnHarvest => true,
        ResourceSpawnerTrigger::Timer => {
            let interval = resource_spawner_periodic_interval(spawner);
            periodic_fires(tick, interval)
        }
    }
}

/// Build resource spawner runtime properties from a MUDL definition.
pub fn apply_resource_spawner_def(
    spawner: &mut Object,
    def: &ResourceSpawnerDef,
    templates: &HashMap<String, ResourceTemplateDef>,
) -> anyhow::Result<()> {
    spawner.set_property_bool("is_resource_spawner", true);
    spawner.set_property_string("resource_spawner_base", &def.base_name);
    spawner.set_property_string("resource_spawner_trigger", def.trigger.as_str());
    spawner.set_property_int(
        "resource_spawner_periodic_interval",
        i64::from(def.periodic_interval),
    );
    spawner.set_property_numeric("resource_spawner_chance", def.chance);
    spawner.set_property_int("resource_spawner_max_active", i64::from(def.max_active));
    spawner.set_property_bool("resource_spawner_once", def.once);
    spawner.set_property_bool("resource_spawner_fired", false);
    spawner.set_property_int("resource_spawner_spawn_count", 0);

    let entry_values: Vec<Value> = def
        .entries
        .iter()
        .map(|entry| {
            if !templates.contains_key(&entry.template) {
                anyhow::bail!(
                    "Resource spawner '{}' references unknown resource-template '{}'",
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
        name: "resource_spawner_entries".to_string(),
        value: Value::List(entry_values),
        permissions: PermissionFlags::EVERYONE,
        behavior: None,
    });
    Ok(())
}

/// Serialize resource templates for lookup at runtime (stored on each resource spawner).
pub fn resource_templates_to_property(templates: &[ResourceTemplateDef]) -> Property {
    let items: Vec<Value> = templates
        .iter()
        .map(|template| {
            Value::Map(HashMap::from([
                (
                    "base_name".to_string(),
                    Value::String(template.base_name.clone()),
                ),
                (
                    "prototype".to_string(),
                    Value::String(template.prototype.clone()),
                ),
                (
                    "count".to_string(),
                    Value::Int(i64::from(template.count.max(1))),
                ),
            ]))
        })
        .collect();
    Property {
        name: "resource_templates".to_string(),
        value: Value::List(items),
        permissions: PermissionFlags::EVERYONE,
        behavior: None,
    }
}

fn resolve_resource_template(template_name: &str, spawner: &Object) -> Option<ResourceTemplateDef> {
    spawner.get_property("resource_templates").and_then(|prop| {
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
                let prototype = map.get("prototype").and_then(|v| {
                    if let Value::String(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })?;
                let count = map
                    .get("count")
                    .and_then(|v| match v {
                        Value::Int(n) => Some(*n as u32),
                        _ => None,
                    })
                    .unwrap_or(1);
                Some(ResourceTemplateDef {
                    base_name: base.to_string(),
                    prototype,
                    count: count.max(1),
                })
            })
        } else {
            None
        }
    })
}

fn find_item_prototype<'a>(
    prototype_base: &str,
    objects: &'a HashMap<ObjectId, Object>,
) -> Option<&'a Object> {
    let expected = ObjectId::new(format!("item:{prototype_base}-001"));
    if let Some(obj) = objects.get(&expected) {
        if obj.is_active() {
            return Some(obj);
        }
    }
    objects.values().find(|obj| {
        obj.is_active()
            && obj.object_type() == "item"
            && obj.prototype.is_none()
            && obj.id.as_str().starts_with("item:")
            && obj.id.as_str().contains(prototype_base)
    })
}

fn clone_item_from_prototype(
    proto: &Object,
    new_id: ObjectId,
    owner: &ObjectId,
    template: &ResourceTemplateDef,
) -> Object {
    let mut item = Object {
        id: new_id,
        name: proto.name.clone(),
        aliases: proto.aliases.clone(),
        location: None,
        prototype: Some(proto.id.clone()),
        owner: owner.clone(),
        permissions: proto.permissions,
        properties: proto.properties.clone(),
        verbs: proto.verbs.clone(),
        event_handlers: proto.event_handlers.clone(),
        revision: 0,
        updated_at: None,
        is_deleted: false,
        deleted_at: None,
    };
    if template.count > 1 {
        item.set_property_int("stack_count", i64::from(template.count));
    }
    item
}

fn room_for_target(
    target_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> Option<ObjectId> {
    let target = objects.get(target_id)?;
    if target.is_location() || target.is_room() {
        return Some(target_id.clone());
    }
    target.location.clone()
}

fn spawn_resource_item(
    spawner: &Object,
    template: &ResourceTemplateDef,
    room_id: &ObjectId,
    owner: &ObjectId,
    spawn_index: u32,
    objects: &HashMap<ObjectId, Object>,
) -> Option<Object> {
    let proto = find_item_prototype(&template.prototype, objects)?;
    let base = spawner
        .get_property("resource_spawner_base")
        .and_then(|p| {
            if let Value::String(s) = &p.value {
                Some(s.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "resource".to_string());
    let id = generate_object_id("item", &format!("{base}-resource"), spawn_index.max(1));
    let mut item = clone_item_from_prototype(proto, id, owner, template);
    item.add_property(Property {
        name: "resource_spawned_by".to_string(),
        value: Value::ObjectRef(spawner.id.clone()),
        permissions: PermissionFlags::EVERYONE,
        behavior: None,
    });
    item.set_property_string("resource_template", &template.base_name);
    item.location = Some(room_id.clone());
    Some(item)
}

fn spawn_message(template: &ResourceTemplateDef) -> String {
    let label = template.base_name.replace('-', " ").to_lowercase();
    format!("You notice {label} here.")
}

fn tick_for_trigger(trigger: ResourceSpawnerTrigger, scheduler_tick: Option<u64>) -> u64 {
    match trigger {
        ResourceSpawnerTrigger::Timer => scheduler_tick.unwrap_or(1),
        ResourceSpawnerTrigger::OnEnter | ResourceSpawnerTrigger::OnHarvest => 1,
    }
}

fn run_resource_spawners(
    spawner_ids: Vec<ObjectId>,
    target_id: &ObjectId,
    trigger: ResourceSpawnerTrigger,
    actor_id: &ObjectId,
    owner: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
    scheduler_tick: Option<u64>,
) -> Vec<ResourceSpawnResult> {
    let room_id = match room_for_target(target_id, objects) {
        Some(room) => room,
        None => return Vec::new(),
    };

    let tick = tick_for_trigger(trigger, scheduler_tick);
    let mut results = Vec::new();
    for spawner_id in spawner_ids {
        let Some(spawner_snapshot) = objects.get(&spawner_id).cloned() else {
            continue;
        };

        if resource_spawner_once(&spawner_snapshot) && resource_spawner_fired(&spawner_snapshot) {
            continue;
        }

        if !trigger_fires(&spawner_snapshot, trigger, tick) {
            continue;
        }

        let chance = resource_spawner_chance(&spawner_snapshot);
        let chance_seed = mix_seed(&[
            spawner_id.as_str(),
            actor_id.as_str(),
            target_id.as_str(),
            &tick.to_string(),
            "resource-chance",
        ]);
        if !chance_rolls(chance_seed, chance) {
            continue;
        }

        let max_active = resource_spawner_max_active(&spawner_snapshot);
        if count_active_resources(&spawner_id, target_id, objects) >= max_active as usize {
            continue;
        }

        let entries = resource_spawner_entries(&spawner_snapshot);
        let pick_seed = mix_seed(&[
            spawner_id.as_str(),
            actor_id.as_str(),
            target_id.as_str(),
            &tick.to_string(),
            "resource-pick",
        ]);
        let Some(entry) = pick_weighted_entry(&entries, pick_seed) else {
            continue;
        };
        let Some(template) = resolve_resource_template(&entry.template, &spawner_snapshot) else {
            continue;
        };

        let spawn_index = resource_spawner_spawn_count(&spawner_snapshot) + 1;
        let Some(item) = spawn_resource_item(
            &spawner_snapshot,
            &template,
            &room_id,
            owner,
            spawn_index,
            objects,
        ) else {
            continue;
        };

        let message = spawn_message(&template);
        let item_id = item.id.clone();
        objects.insert(item_id.clone(), item);

        if let Some(spawner) = objects.get_mut(&spawner_id) {
            set_resource_spawner_spawn_count(spawner, spawn_index);
            if resource_spawner_once(spawner) {
                set_resource_spawner_fired(spawner, true);
            }
        }

        results.push(ResourceSpawnResult {
            item_id,
            message: Some(message),
        });
    }
    results
}

/// Dispatch resource spawners subscribed to `event_name` on `host_id`.
pub fn dispatch_resource_spawners_for_event(
    event_name: &str,
    host_id: &ObjectId,
    actor_id: &ObjectId,
    owner: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
    scheduler_tick: Option<u64>,
) -> crate::world::EventOutcome {
    use crate::world::EventOutcome;

    let mut outcome = EventOutcome::default();
    let results = match event_name {
        "on_enter" => {
            let mut results = run_on_enter_resource_spawners(
                host_id,
                actor_id,
                owner,
                objects,
                scheduler_tick,
            );
            results.extend(run_timer_resource_spawners(
                host_id,
                actor_id,
                owner,
                objects,
                scheduler_tick,
            ));
            results
        }
        crate::mudl::trigger_def::events::ON_HARVEST => {
            run_on_harvest_resource_spawners(host_id, actor_id, owner, objects)
        }
        _ => Vec::new(),
    };

    let spawner_ids: Vec<ObjectId> = resource_spawners_for_target(host_id, objects)
        .into_iter()
        .map(|spawner| spawner.id.clone())
        .collect();

    for resource in results {
        outcome.mark_dirty(&resource.item_id);
        if let Some(message) = resource.message {
            outcome.push_line(message);
        }
    }
    for spawner_id in spawner_ids {
        outcome.mark_dirty(&spawner_id);
    }
    outcome
}

/// Run `on_enter` resource spawners for a room.
pub fn run_on_enter_resource_spawners(
    room_id: &ObjectId,
    player_id: &ObjectId,
    owner: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
    scheduler_tick: Option<u64>,
) -> Vec<ResourceSpawnResult> {
    let spawner_ids: Vec<ObjectId> = resource_spawners_in_room(room_id, objects)
        .into_iter()
        .filter(|s| resource_spawner_trigger(s) == ResourceSpawnerTrigger::OnEnter)
        .map(|s| s.id.clone())
        .collect();
    run_resource_spawners(
        spawner_ids,
        room_id,
        ResourceSpawnerTrigger::OnEnter,
        player_id,
        owner,
        objects,
        scheduler_tick,
    )
}

/// Run `timer` resource spawners for a room (periodic on scheduler ticks).
pub fn run_timer_resource_spawners(
    room_id: &ObjectId,
    player_id: &ObjectId,
    owner: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
    scheduler_tick: Option<u64>,
) -> Vec<ResourceSpawnResult> {
    let spawner_ids: Vec<ObjectId> = resource_spawners_in_room(room_id, objects)
        .into_iter()
        .filter(|s| resource_spawner_trigger(s) == ResourceSpawnerTrigger::Timer)
        .map(|s| s.id.clone())
        .collect();
    run_resource_spawners(
        spawner_ids,
        room_id,
        ResourceSpawnerTrigger::Timer,
        player_id,
        owner,
        objects,
        scheduler_tick,
    )
}

/// Run `on_harvest` resource spawners attached to a harvestable object.
pub fn run_on_harvest_resource_spawners(
    target_id: &ObjectId,
    player_id: &ObjectId,
    owner: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
) -> Vec<ResourceSpawnResult> {
    let spawner_ids: Vec<ObjectId> = resource_spawners_for_target(target_id, objects)
        .into_iter()
        .filter(|s| resource_spawner_trigger(s) == ResourceSpawnerTrigger::OnHarvest)
        .map(|s| s.id.clone())
        .collect();
    run_resource_spawners(
        spawner_ids,
        target_id,
        ResourceSpawnerTrigger::OnHarvest,
        player_id,
        owner,
        objects,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_templates() -> HashMap<String, ResourceTemplateDef> {
        HashMap::from([(
            "moon-moss".to_string(),
            ResourceTemplateDef {
                base_name: "moon-moss".to_string(),
                prototype: "trail-rations".to_string(),
                count: 2,
            },
        )])
    }

    fn proto_rations() -> Object {
        let mut proto = Object {
            id: ObjectId::new("item:trail-rations-001"),
            name: "Trail Rations".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new("player:admin-001"),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        proto.set_property_bool("stackable", true);
        proto.set_property_int("stack_count", 3);
        proto
    }

    fn moss_patch_with_spawner() -> (Object, Object) {
        let patch_id = ObjectId::new("item:moss-patch-001");
        let room_id = ObjectId::new("area:void-001");
        let patch = Object {
            id: patch_id.clone(),
            name: "Moss Patch".to_string(),
            aliases: Vec::new(),
            location: Some(room_id.clone()),
            prototype: None,
            owner: ObjectId::new("player:admin-001"),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };

        let mut spawner = Object {
            id: ObjectId::new("resource-spawner:moss-harvest-001"),
            name: "moss-harvest resource spawner".to_string(),
            aliases: Vec::new(),
            location: Some(room_id),
            prototype: None,
            owner: ObjectId::new("player:admin-001"),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        apply_resource_spawner_def(
            &mut spawner,
            &ResourceSpawnerDef {
                base_name: "moss-harvest".to_string(),
                target: "moss-patch".to_string(),
                trigger: ResourceSpawnerTrigger::OnHarvest,
                periodic_interval: 5,
                chance: 1.0,
                max_active: 2,
                once: true,
                entries: vec![SpawnerEntryDef {
                    template: "moon-moss".to_string(),
                    weight: 1,
                }],
            },
            &sample_templates(),
        )
        .unwrap();
        spawner.set_property_object_ref("resource_spawner_target", patch_id.clone());
        spawner.add_property(resource_templates_to_property(
            &sample_templates().into_values().collect::<Vec<_>>(),
        ));
        (patch, spawner)
    }

    #[test]
    fn on_harvest_spawner_adds_resource_to_room() {
        let player = ObjectId::new("player:hero-001");
        let owner = ObjectId::new("player:admin-001");
        let (patch, spawner) = moss_patch_with_spawner();
        let proto = proto_rations();
        let room_id = ObjectId::new("area:void-001");
        let mut objects = HashMap::from([
            (room_id.clone(), Object {
                id: room_id.clone(),
                name: "Void".to_string(),
                aliases: Vec::new(),
                location: None,
                prototype: None,
                owner: owner.clone(),
                permissions: PermissionFlags::EVERYONE,
                properties: HashMap::new(),
                verbs: HashMap::new(),
                event_handlers: HashMap::new(),
                revision: 0,
                updated_at: None,
                is_deleted: false,
                deleted_at: None,
            }),
            (patch.id.clone(), patch),
            (spawner.id.clone(), spawner),
            (proto.id.clone(), proto),
        ]);

        let first = run_on_harvest_resource_spawners(
            &ObjectId::new("item:moss-patch-001"),
            &player,
            &owner,
            &mut objects,
        );
        assert_eq!(first.len(), 1);
        let spawned = objects.get(&first[0].item_id).unwrap();
        assert_eq!(spawned.location.as_ref(), Some(&room_id));

        let second = run_on_harvest_resource_spawners(
            &ObjectId::new("item:moss-patch-001"),
            &player,
            &owner,
            &mut objects,
        );
        assert!(second.is_empty(), "once=true prevents repeat drops");
    }

    #[test]
    fn timer_spawner_uses_scheduler_tick() {
        let player = ObjectId::new("player:hero-001");
        let owner = ObjectId::new("player:admin-001");
        let room_id = ObjectId::new("area:grove-001");
        let proto = proto_rations();

        let mut spawner = Object {
            id: ObjectId::new("resource-spawner:grove-timer-001"),
            name: "grove-timer resource spawner".to_string(),
            aliases: Vec::new(),
            location: Some(room_id.clone()),
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        apply_resource_spawner_def(
            &mut spawner,
            &ResourceSpawnerDef {
                base_name: "grove-timer".to_string(),
                target: "grove".to_string(),
                trigger: ResourceSpawnerTrigger::Timer,
                periodic_interval: 3,
                chance: 1.0,
                max_active: 4,
                once: false,
                entries: vec![SpawnerEntryDef {
                    template: "moon-moss".to_string(),
                    weight: 1,
                }],
            },
            &sample_templates(),
        )
        .unwrap();
        spawner.set_property_object_ref("resource_spawner_target", room_id.clone());
        spawner.add_property(resource_templates_to_property(
            &sample_templates().into_values().collect::<Vec<_>>(),
        ));

        let room = Object {
            id: room_id.clone(),
            name: "Grove".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };

        let mut objects = HashMap::from([
            (room_id.clone(), room),
            (spawner.id.clone(), spawner),
            (proto.id.clone(), proto),
        ]);

        let tick2 = run_timer_resource_spawners(
            &room_id,
            &player,
            &owner,
            &mut objects,
            Some(2),
        );
        assert!(tick2.is_empty(), "interval=3 should not fire on tick 2");

        let tick3 = run_timer_resource_spawners(
            &room_id,
            &player,
            &owner,
            &mut objects,
            Some(3),
        );
        assert_eq!(tick3.len(), 1);
    }
}