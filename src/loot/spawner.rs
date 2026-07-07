//! Location- and object-attached loot spawners with weighted item templates.

use std::collections::HashMap;

use crate::creature::spawner::pick_weighted_entry;
use crate::mudl::{LootSpawnerDef, LootSpawnerTrigger, LootTemplateDef, SpawnerEntryDef};
use crate::object::{generate_object_id, Object, ObjectId, PermissionFlags, Property, Value};

/// Result of a loot spawner tick — narrative feedback for the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LootSpawnResult {
    pub item_id: ObjectId,
    pub message: Option<String>,
}

/// Whether `obj` is a loot spawner (hidden from room listings).
pub fn is_loot_spawner(obj: &Object) -> bool {
    obj.get_bool_property("is_loot_spawner").unwrap_or(false)
}

/// Whether `obj` should be hidden from player room listings.
pub fn is_loot_spawner_infrastructure(obj: &Object) -> bool {
    is_loot_spawner(obj)
}

fn loot_spawner_target_id(obj: &Object) -> Option<ObjectId> {
    obj.get_object_ref_property("loot_spawner_target")
}

fn loot_spawner_trigger(obj: &Object) -> LootSpawnerTrigger {
    obj.get_property("loot_spawner_trigger")
        .and_then(|p| {
            if let Value::String(s) = &p.value {
                Some(LootSpawnerTrigger::parse(s))
            } else {
                None
            }
        })
        .unwrap_or(LootSpawnerTrigger::OnOpen)
}

fn loot_spawner_chance(obj: &Object) -> f64 {
    obj.get_float_property("loot_spawner_chance")
        .unwrap_or(1.0)
        .clamp(0.0, 1.0)
}

fn loot_spawner_max_active(obj: &Object) -> u32 {
    obj.get_int_property("loot_spawner_max_active")
        .unwrap_or(4)
        .max(0) as u32
}

fn loot_spawner_periodic_interval(obj: &Object) -> u32 {
    obj.get_int_property("loot_spawner_periodic_interval")
        .unwrap_or(5)
        .max(1) as u32
}

fn loot_spawner_trigger_count(obj: &Object) -> u64 {
    obj.get_int_property("loot_spawner_trigger_count")
        .unwrap_or(0)
        .max(0) as u64
}

fn loot_spawner_spawn_count(obj: &Object) -> u32 {
    obj.get_int_property("loot_spawner_spawn_count")
        .unwrap_or(0)
        .max(0) as u32
}

fn loot_spawner_once(obj: &Object) -> bool {
    obj.get_bool_property("loot_spawner_once").unwrap_or(false)
}

fn loot_spawner_fired(obj: &Object) -> bool {
    obj.get_bool_property("loot_spawner_fired").unwrap_or(false)
}

fn set_loot_spawner_trigger_count(spawner: &mut Object, count: u64) {
    spawner.set_property_int("loot_spawner_trigger_count", count as i64);
}

fn set_loot_spawner_spawn_count(spawner: &mut Object, count: u32) {
    spawner.set_property_int("loot_spawner_spawn_count", count as i64);
}

fn set_loot_spawner_fired(spawner: &mut Object, fired: bool) {
    spawner.set_property_bool("loot_spawner_fired", fired);
}

/// Parse weighted entries stored on a loot spawner object.
pub fn loot_spawner_entries(obj: &Object) -> Vec<SpawnerEntryDef> {
    obj.get_property("loot_spawner_entries")
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

/// Loot spawners whose `target` resolves to `target_id`.
pub fn loot_spawners_for_target<'a>(
    target_id: &ObjectId,
    objects: &'a HashMap<ObjectId, Object>,
) -> Vec<&'a Object> {
    objects
        .values()
        .filter(|obj| {
            obj.is_active()
                && is_loot_spawner(obj)
                && loot_spawner_target_id(obj).as_ref() == Some(target_id)
        })
        .collect()
}

/// Loot spawners attached to a room (target is the room itself).
pub fn loot_spawners_in_room<'a>(
    room_id: &ObjectId,
    objects: &'a HashMap<ObjectId, Object>,
) -> Vec<&'a Object> {
    loot_spawners_for_target(room_id, objects)
}

/// Active loot items spawned by `spawner_id` at `target_id`.
pub fn count_active_loot(
    spawner_id: &ObjectId,
    target_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> usize {
    let target_is_room = objects
        .get(target_id)
        .is_some_and(|t| t.is_location() || t.is_room());

    objects
        .values()
        .filter(|obj| {
            if !obj.is_active() || obj.object_type() != "item" {
                return false;
            }
            let spawned_by = obj.get_object_ref_property("looted_by");
            if spawned_by.as_ref() != Some(spawner_id) {
                return false;
            }
            if target_is_room {
                obj.location.as_ref() == Some(target_id)
            } else if let Some(target) = objects.get(target_id) {
                if target.is_container() {
                    target.container_contents().contains(&obj.id)
                } else {
                    obj.location.as_ref() == target.location.as_ref()
                }
            } else {
                false
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

fn trigger_fires(spawner: &Object, trigger: LootSpawnerTrigger, tick: u64) -> bool {
    let actual = loot_spawner_trigger(spawner);
    if actual != trigger {
        return false;
    }
    match actual {
        LootSpawnerTrigger::OnEnter
        | LootSpawnerTrigger::OnOpen
        | LootSpawnerTrigger::OnKill
        | LootSpawnerTrigger::OnBreak => true,
        LootSpawnerTrigger::Timer => {
            let interval = u64::from(loot_spawner_periodic_interval(spawner));
            tick.is_multiple_of(interval)
        }
    }
}

/// Build loot spawner runtime properties from a MUDL definition.
pub fn apply_loot_spawner_def(
    spawner: &mut Object,
    def: &LootSpawnerDef,
    templates: &HashMap<String, LootTemplateDef>,
) -> anyhow::Result<()> {
    spawner.set_property_bool("is_loot_spawner", true);
    spawner.set_property_string("loot_spawner_base", &def.base_name);
    spawner.set_property_string("loot_spawner_trigger", def.trigger.as_str());
    spawner.set_property_int(
        "loot_spawner_periodic_interval",
        i64::from(def.periodic_interval),
    );
    spawner.set_property_numeric("loot_spawner_chance", def.chance);
    spawner.set_property_int("loot_spawner_max_active", i64::from(def.max_active));
    spawner.set_property_bool("loot_spawner_once", def.once);
    spawner.set_property_bool("loot_spawner_fired", false);
    spawner.set_property_int("loot_spawner_trigger_count", 0);
    spawner.set_property_int("loot_spawner_spawn_count", 0);

    let entry_values: Vec<Value> = def
        .entries
        .iter()
        .map(|entry| {
            if !templates.contains_key(&entry.template) {
                anyhow::bail!(
                    "Loot spawner '{}' references unknown loot-template '{}'",
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
        name: "loot_spawner_entries".to_string(),
        value: Value::List(entry_values),
        permissions: PermissionFlags::EVERYONE,
        behavior: None,
    });
    Ok(())
}

/// Serialize loot templates for lookup at runtime (stored on each loot spawner).
pub fn loot_templates_to_property(templates: &[LootTemplateDef]) -> Property {
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
        name: "loot_templates".to_string(),
        value: Value::List(items),
        permissions: PermissionFlags::EVERYONE,
        behavior: None,
    }
}

fn resolve_loot_template(template_name: &str, spawner: &Object) -> Option<LootTemplateDef> {
    spawner.get_property("loot_templates").and_then(|prop| {
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
                Some(LootTemplateDef {
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
    template: &LootTemplateDef,
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
        is_deleted: false,
        deleted_at: None,
    };
    if template.count > 1 {
        item.set_property_int("stack_count", i64::from(template.count));
    }
    item
}

enum LootPlacement {
    Container(ObjectId),
    Room(ObjectId),
}

fn placement_for_target(
    target_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> Option<LootPlacement> {
    let target = objects.get(target_id)?;
    if target.is_container() {
        return Some(LootPlacement::Container(target_id.clone()));
    }
    if target.is_location() || target.is_room() {
        return Some(LootPlacement::Room(target_id.clone()));
    }
    if let Some(room_id) = target.location.clone() {
        return Some(LootPlacement::Room(room_id));
    }
    None
}

fn place_loot_item(
    item: &Object,
    placement: &LootPlacement,
    objects: &mut HashMap<ObjectId, Object>,
) {
    match placement {
        LootPlacement::Container(container_id) => {
            let mut container = objects.get(container_id).unwrap().clone();
            container.add_to_list_property("contents", item.id.clone());
            objects.insert(container_id.clone(), container);
        }
        LootPlacement::Room(_) => {}
    }
}

fn spawn_loot_item(
    spawner: &Object,
    template: &LootTemplateDef,
    placement: &LootPlacement,
    owner: &ObjectId,
    spawn_index: u32,
    objects: &HashMap<ObjectId, Object>,
) -> Option<Object> {
    let proto = find_item_prototype(&template.prototype, objects)?;
    let base = spawner
        .get_property("loot_spawner_base")
        .and_then(|p| {
            if let Value::String(s) = &p.value {
                Some(s.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "loot".to_string());
    let id = generate_object_id("item", &format!("{base}-drop"), spawn_index.max(1));
    let mut item = clone_item_from_prototype(proto, id, owner, template);
    item.add_property(Property {
        name: "looted_by".to_string(),
        value: Value::ObjectRef(spawner.id.clone()),
        permissions: PermissionFlags::EVERYONE,
        behavior: None,
    });
    item.set_property_string("loot_template", &template.base_name);

    match placement {
        LootPlacement::Container(container_id) => {
            item.location = Some(container_id.clone());
        }
        LootPlacement::Room(room_id) => {
            item.location = Some(room_id.clone());
        }
    }
    Some(item)
}

fn spawn_message(template: &LootTemplateDef, placement: &LootPlacement) -> String {
    let label = template.base_name.replace('-', " ").to_lowercase();
    match placement {
        LootPlacement::Container(_) => format!("You find {label} inside."),
        LootPlacement::Room(_) => format!("You notice {label} here."),
    }
}

fn run_loot_spawners(
    spawner_ids: Vec<ObjectId>,
    target_id: &ObjectId,
    trigger: LootSpawnerTrigger,
    actor_id: &ObjectId,
    owner: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
) -> Vec<LootSpawnResult> {
    let placement = match placement_for_target(target_id, objects) {
        Some(p) => p,
        None => return Vec::new(),
    };

    let mut results = Vec::new();
    for spawner_id in spawner_ids {
        let Some(spawner_snapshot) = objects.get(&spawner_id).cloned() else {
            continue;
        };

        if loot_spawner_once(&spawner_snapshot) && loot_spawner_fired(&spawner_snapshot) {
            continue;
        }

        let tick = loot_spawner_trigger_count(&spawner_snapshot) + 1;
        if let Some(spawner) = objects.get_mut(&spawner_id) {
            set_loot_spawner_trigger_count(spawner, tick);
        }

        if !trigger_fires(&spawner_snapshot, trigger, tick) {
            continue;
        }

        let chance = loot_spawner_chance(&spawner_snapshot);
        let chance_seed = mix_seed(&[
            spawner_id.as_str(),
            actor_id.as_str(),
            target_id.as_str(),
            &tick.to_string(),
            "loot-chance",
        ]);
        if !chance_rolls(chance_seed, chance) {
            continue;
        }

        let max_active = loot_spawner_max_active(&spawner_snapshot);
        if count_active_loot(&spawner_id, target_id, objects) >= max_active as usize {
            continue;
        }

        let entries = loot_spawner_entries(&spawner_snapshot);
        let pick_seed = mix_seed(&[
            spawner_id.as_str(),
            actor_id.as_str(),
            target_id.as_str(),
            &tick.to_string(),
            "loot-pick",
        ]);
        let Some(entry) = pick_weighted_entry(&entries, pick_seed) else {
            continue;
        };
        let Some(template) = resolve_loot_template(&entry.template, &spawner_snapshot) else {
            continue;
        };

        let spawn_index = loot_spawner_spawn_count(&spawner_snapshot) + 1;
        let Some(item) = spawn_loot_item(
            &spawner_snapshot,
            &template,
            &placement,
            owner,
            spawn_index,
            objects,
        ) else {
            continue;
        };

        let message = spawn_message(&template, &placement);
        let item_id = item.id.clone();
        place_loot_item(&item, &placement, objects);
        objects.insert(item_id.clone(), item);

        if let Some(spawner) = objects.get_mut(&spawner_id) {
            set_loot_spawner_spawn_count(spawner, spawn_index);
            if loot_spawner_once(spawner) {
                set_loot_spawner_fired(spawner, true);
            }
        }

        results.push(LootSpawnResult {
            item_id,
            message: Some(message),
        });
    }
    results
}

/// Dispatch loot spawners subscribed to `event_name` on `host_id`.
pub fn dispatch_loot_spawners_for_event(
    event_name: &str,
    host_id: &ObjectId,
    actor_id: &ObjectId,
    owner: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
) -> crate::world::EventOutcome {
    use crate::world::EventOutcome;

    let mut outcome = EventOutcome::default();
    let results = match event_name {
        "on_enter" => {
            let mut results = run_on_enter_loot_spawners(host_id, actor_id, owner, objects);
            results.extend(run_timer_loot_spawners(host_id, actor_id, owner, objects));
            results
        }
        "on_open" => run_on_open_loot_spawners(host_id, actor_id, owner, objects),
        "on_break" => run_on_break_loot_spawners(host_id, actor_id, owner, objects),
        "on_kill" => run_on_kill_loot_spawners(host_id, actor_id, owner, objects),
        _ => Vec::new(),
    };

    let spawner_ids: Vec<ObjectId> = loot_spawners_for_target(host_id, objects)
        .into_iter()
        .map(|spawner| spawner.id.clone())
        .collect();

    for loot in results {
        outcome.mark_dirty(&loot.item_id);
        if let Some(message) = loot.message {
            outcome.push_line(message);
        }
    }
    for spawner_id in spawner_ids {
        outcome.mark_dirty(&spawner_id);
    }
    outcome
}

/// Run `on_enter` loot spawners for a room.
pub fn run_on_enter_loot_spawners(
    room_id: &ObjectId,
    player_id: &ObjectId,
    owner: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
) -> Vec<LootSpawnResult> {
    let spawner_ids: Vec<ObjectId> = loot_spawners_in_room(room_id, objects)
        .into_iter()
        .filter(|s| loot_spawner_trigger(s) == LootSpawnerTrigger::OnEnter)
        .map(|s| s.id.clone())
        .collect();
    run_loot_spawners(
        spawner_ids,
        room_id,
        LootSpawnerTrigger::OnEnter,
        player_id,
        owner,
        objects,
    )
}

/// Run `timer` loot spawners for a room (periodic on each qualifying enter tick).
pub fn run_timer_loot_spawners(
    room_id: &ObjectId,
    player_id: &ObjectId,
    owner: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
) -> Vec<LootSpawnResult> {
    let spawner_ids: Vec<ObjectId> = loot_spawners_in_room(room_id, objects)
        .into_iter()
        .filter(|s| loot_spawner_trigger(s) == LootSpawnerTrigger::Timer)
        .map(|s| s.id.clone())
        .collect();
    run_loot_spawners(
        spawner_ids,
        room_id,
        LootSpawnerTrigger::Timer,
        player_id,
        owner,
        objects,
    )
}

/// Run `on_open` loot spawners attached to a container, door, or other gate.
pub fn run_on_open_loot_spawners(
    target_id: &ObjectId,
    player_id: &ObjectId,
    owner: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
) -> Vec<LootSpawnResult> {
    let spawner_ids: Vec<ObjectId> = loot_spawners_for_target(target_id, objects)
        .into_iter()
        .filter(|s| loot_spawner_trigger(s) == LootSpawnerTrigger::OnOpen)
        .map(|s| s.id.clone())
        .collect();
    run_loot_spawners(
        spawner_ids,
        target_id,
        LootSpawnerTrigger::OnOpen,
        player_id,
        owner,
        objects,
    )
}

/// Run `on_break` loot spawners attached to a breakable object.
pub fn run_on_break_loot_spawners(
    target_id: &ObjectId,
    player_id: &ObjectId,
    owner: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
) -> Vec<LootSpawnResult> {
    let spawner_ids: Vec<ObjectId> = loot_spawners_for_target(target_id, objects)
        .into_iter()
        .filter(|s| loot_spawner_trigger(s) == LootSpawnerTrigger::OnBreak)
        .map(|s| s.id.clone())
        .collect();
    run_loot_spawners(
        spawner_ids,
        target_id,
        LootSpawnerTrigger::OnBreak,
        player_id,
        owner,
        objects,
    )
}

/// Run `on_kill` loot spawners attached to a creature (dispatched via `execute_event`).
pub fn run_on_kill_loot_spawners(
    victim_id: &ObjectId,
    killer_id: &ObjectId,
    owner: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
) -> Vec<LootSpawnResult> {
    let spawner_ids: Vec<ObjectId> = loot_spawners_for_target(victim_id, objects)
        .into_iter()
        .filter(|s| loot_spawner_trigger(s) == LootSpawnerTrigger::OnKill)
        .map(|s| s.id.clone())
        .collect();
    run_loot_spawners(
        spawner_ids,
        victim_id,
        LootSpawnerTrigger::OnKill,
        killer_id,
        owner,
        objects,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample_templates() -> HashMap<String, LootTemplateDef> {
        HashMap::from([(
            "bonus-rations".to_string(),
            LootTemplateDef {
                base_name: "bonus-rations".to_string(),
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
            is_deleted: false,
            deleted_at: None,
        };
        proto.set_property_bool("stackable", true);
        proto.set_property_int("stack_count", 3);
        proto
    }

    fn chest_with_spawner() -> (Object, Object) {
        let chest_id = ObjectId::new("item:scene-chest-001");
        let mut chest = Object {
            id: chest_id.clone(),
            name: "Travel Chest".to_string(),
            aliases: Vec::new(),
            location: Some(ObjectId::new("area:void-001")),
            prototype: None,
            owner: ObjectId::new("player:admin-001"),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        chest.apply_container_role(&crate::object::ContainerSpec::default());

        let mut spawner = Object {
            id: ObjectId::new("loot-spawner:chest-bonus-001"),
            name: "chest-bonus loot spawner".to_string(),
            aliases: Vec::new(),
            location: Some(ObjectId::new("area:void-001")),
            prototype: None,
            owner: ObjectId::new("player:admin-001"),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
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
            &sample_templates(),
        )
        .unwrap();
        spawner.set_property_object_ref("loot_spawner_target", chest_id.clone());
        spawner.add_property(loot_templates_to_property(
            &sample_templates().into_values().collect::<Vec<_>>(),
        ));
        (chest, spawner)
    }

    #[test]
    fn on_open_spawner_adds_loot_to_container() {
        let player = ObjectId::new("player:hero-001");
        let owner = ObjectId::new("player:admin-001");
        let (chest, spawner) = chest_with_spawner();
        let proto = proto_rations();
        let mut objects = HashMap::from([
            (chest.id.clone(), chest),
            (spawner.id.clone(), spawner),
            (proto.id.clone(), proto),
        ]);

        let first = run_on_open_loot_spawners(
            &ObjectId::new("item:scene-chest-001"),
            &player,
            &owner,
            &mut objects,
        );
        assert_eq!(first.len(), 1);
        let chest = objects.get(&ObjectId::new("item:scene-chest-001")).unwrap();
        assert_eq!(chest.container_contents().len(), 1);

        let second = run_on_open_loot_spawners(
            &ObjectId::new("item:scene-chest-001"),
            &player,
            &owner,
            &mut objects,
        );
        assert!(second.is_empty(), "once=true prevents repeat drops");
    }
}
