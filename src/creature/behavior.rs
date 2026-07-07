//! MUDL-driven composable creature behaviors — templates, scripts, and reactions.

use std::collections::HashMap;

use crate::creature::tactics::{
    is_creature_aware, is_player_aware, resolve_encounter_awareness_on_enter,
    reset_player_awareness_on_enter, set_creature_aware, set_player_aware,
    uses_awareness_check, SURPRISE_DAMAGE_BONUS,
};
use crate::mudl::AnatomyRegistry;
use crate::creature::vitality::{apply_damage, creature_health};
use crate::mudl::{BehaviorTemplateDef, CreatureReact, NpcBehaviorDef};
use crate::object::{Object, ObjectId, PermissionFlags, Property, Value};

/// A single behavior entry stored on a creature (`creature_behaviors` property).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatureBehaviorEntry {
    pub entry_type: String,
    pub template_name: Option<String>,
    pub react: Option<CreatureReact>,
    pub event: Option<String>,
    pub action: Option<String>,
    pub text: Option<String>,
    pub wander_interval: Option<u32>,
    pub attack_damage: Option<i64>,
    pub awareness_check: Option<bool>,
    pub perception: Option<i64>,
}

/// Outcome of running creature behaviors — narrative lines and touched object ids.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BehaviorOutcome {
    pub lines: Vec<String>,
    pub dirty: Vec<ObjectId>,
}

impl BehaviorOutcome {
    fn push_line(&mut self, line: String) {
        if !line.is_empty() {
            self.lines.push(line);
        }
    }

    fn mark_dirty(&mut self, id: &ObjectId) {
        if !self.dirty.iter().any(|d| d == id) {
            self.dirty.push(id.clone());
        }
    }
}

fn behavior_entry_map(entry: &CreatureBehaviorEntry) -> Value {
    let mut map = HashMap::from([("type".to_string(), Value::String(entry.entry_type.clone()))]);
    if let Some(name) = &entry.template_name {
        map.insert("template".to_string(), Value::String(name.clone()));
    }
    if let Some(react) = entry.react {
        map.insert(
            "react".to_string(),
            Value::String(react.as_str().to_string()),
        );
    }
    if let Some(event) = &entry.event {
        map.insert("event".to_string(), Value::String(event.clone()));
    }
    if let Some(action) = &entry.action {
        map.insert("action".to_string(), Value::String(action.clone()));
    }
    if let Some(text) = &entry.text {
        map.insert("text".to_string(), Value::String(text.clone()));
    }
    if let Some(interval) = entry.wander_interval {
        map.insert(
            "wander_interval".to_string(),
            Value::Int(i64::from(interval)),
        );
    }
    if let Some(damage) = entry.attack_damage {
        map.insert("attack_damage".to_string(), Value::Int(damage));
    }
    if let Some(check) = entry.awareness_check {
        map.insert("awareness_check".to_string(), Value::Bool(check));
    }
    if let Some(perception) = entry.perception {
        map.insert("perception".to_string(), Value::Int(perception));
    }
    Value::Map(map)
}

fn entry_from_map(map: &HashMap<String, Value>) -> Option<CreatureBehaviorEntry> {
    let entry_type = map.get("type").and_then(|v| {
        if let Value::String(s) = v {
            Some(s.clone())
        } else {
            None
        }
    })?;
    let react = map.get("react").and_then(|v| {
        if let Value::String(s) = v {
            Some(CreatureReact::parse(s))
        } else {
            None
        }
    });
    Some(CreatureBehaviorEntry {
        entry_type,
        template_name: map.get("template").and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            _ => None,
        }),
        react,
        event: map.get("event").and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            _ => None,
        }),
        action: map.get("action").and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            _ => None,
        }),
        text: map.get("text").and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            _ => None,
        }),
        wander_interval: map.get("wander_interval").and_then(|v| match v {
            Value::Int(n) => Some((*n).max(1) as u32),
            _ => None,
        }),
        attack_damage: map.get("attack_damage").and_then(|v| match v {
            Value::Int(n) => Some(*n),
            _ => None,
        }),
        awareness_check: map.get("awareness_check").and_then(|v| match v {
            Value::Bool(b) => Some(*b),
            _ => None,
        }),
        perception: map.get("perception").and_then(|v| match v {
            Value::Int(n) => Some(*n),
            _ => None,
        }),
    })
}

/// Expand MUDL scripts and `@use-behavior` references into runtime behavior entries.
pub fn build_creature_behavior_entries(
    scripts: &[NpcBehaviorDef],
    use_behaviors: &[String],
    templates: &HashMap<String, BehaviorTemplateDef>,
) -> Vec<CreatureBehaviorEntry> {
    let mut entries = Vec::new();
    for name in use_behaviors {
        let Some(template) = templates.get(name) else {
            continue;
        };
        entries.push(template_to_entry(template));
    }
    for script in scripts {
        entries.push(script_to_entry(script));
    }
    entries
}

fn template_to_entry(template: &BehaviorTemplateDef) -> CreatureBehaviorEntry {
    CreatureBehaviorEntry {
        entry_type: "template".to_string(),
        template_name: Some(template.base_name.clone()),
        react: Some(template.react),
        event: Some("on_enter".to_string()),
        action: template.on_enter_action.clone(),
        text: template.on_enter_text.clone(),
        wander_interval: Some(template.wander_interval),
        attack_damage: Some(template.attack_damage),
        awareness_check: template.awareness_check,
        perception: template.perception,
    }
}

fn script_to_entry(script: &NpcBehaviorDef) -> CreatureBehaviorEntry {
    CreatureBehaviorEntry {
        entry_type: "script".to_string(),
        template_name: None,
        react: None,
        event: Some(script.event.clone()),
        action: Some(script.action.clone()),
        text: Some(script.text.clone()),
        wander_interval: None,
        attack_damage: None,
        awareness_check: None,
        perception: None,
    }
}

/// Serialize all behavior templates for attachment to spawner objects.
pub fn behavior_templates_to_property(templates: &[BehaviorTemplateDef]) -> Property {
    let items: Vec<Value> = templates
        .iter()
        .map(|template| {
            let mut map = HashMap::from([
                (
                    "base_name".to_string(),
                    Value::String(template.base_name.clone()),
                ),
                (
                    "react".to_string(),
                    Value::String(template.react.as_str().to_string()),
                ),
                (
                    "wander_interval".to_string(),
                    Value::Int(i64::from(template.wander_interval)),
                ),
                (
                    "attack_damage".to_string(),
                    Value::Int(template.attack_damage),
                ),
            ]);
            if let Some(check) = template.awareness_check {
                map.insert("awareness_check".to_string(), Value::Bool(check));
            }
            if let Some(perception) = template.perception {
                map.insert("perception".to_string(), Value::Int(perception));
            }
            if let Some(action) = &template.on_enter_action {
                map.insert("on_enter_action".to_string(), Value::String(action.clone()));
            }
            if let Some(text) = &template.on_enter_text {
                map.insert("on_enter_text".to_string(), Value::String(text.clone()));
            }
            Value::Map(map)
        })
        .collect();
    Property {
        name: "behavior_templates".to_string(),
        value: Value::List(items),
        permissions: PermissionFlags::EVERYONE,
        behavior: None,
    }
}

/// Resolve behavior templates stored on a spawner (or other host object).
pub fn resolve_behavior_templates(host: &Object) -> HashMap<String, BehaviorTemplateDef> {
    host.get_property("behavior_templates")
        .and_then(|prop| {
            if let Value::List(items) = &prop.value {
                Some(
                    items
                        .iter()
                        .filter_map(|entry| {
                            let Value::Map(map) = entry else {
                                return None;
                            };
                            let base = map.get("base_name").and_then(|v| match v {
                                Value::String(s) => Some(s.clone()),
                                _ => None,
                            })?;
                            let react = map
                                .get("react")
                                .and_then(|v| {
                                    if let Value::String(s) = v {
                                        Some(CreatureReact::parse(s))
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or(CreatureReact::Ignore);
                            Some((
                                base.clone(),
                                BehaviorTemplateDef {
                                    base_name: base,
                                    react,
                                    on_enter_action: map.get("on_enter_action").and_then(|v| {
                                        if let Value::String(s) = v {
                                            Some(s.clone())
                                        } else {
                                            None
                                        }
                                    }),
                                    on_enter_text: map.get("on_enter_text").and_then(|v| {
                                        if let Value::String(s) = v {
                                            Some(s.clone())
                                        } else {
                                            None
                                        }
                                    }),
                                    wander_interval: map
                                        .get("wander_interval")
                                        .and_then(|v| match v {
                                            Value::Int(n) => Some((*n).max(1) as u32),
                                            _ => None,
                                        })
                                        .unwrap_or(3),
                                    attack_damage: map
                                        .get("attack_damage")
                                        .and_then(|v| match v {
                                            Value::Int(n) => Some(*n),
                                            _ => None,
                                        })
                                        .unwrap_or(8),
                                    awareness_check: map.get("awareness_check").and_then(|v| {
                                        if let Value::Bool(b) = v {
                                            Some(*b)
                                        } else {
                                            None
                                        }
                                    }),
                                    perception: map.get("perception").and_then(|v| match v {
                                        Value::Int(n) => Some(*n),
                                        _ => None,
                                    }),
                                },
                            ))
                        })
                        .collect(),
                )
            } else {
                None
            }
        })
        .unwrap_or_default()
}

/// Serialize composable behaviors for storage on a creature object.
pub fn creature_behaviors_to_property(entries: &[CreatureBehaviorEntry]) -> Property {
    Property {
        name: "creature_behaviors".to_string(),
        value: Value::List(entries.iter().map(behavior_entry_map).collect()),
        permissions: PermissionFlags::EVERYONE,
        behavior: None,
    }
}

/// Read composable behaviors from a creature, including legacy `npc_behaviors`.
pub fn read_creature_behaviors(obj: &Object) -> Vec<CreatureBehaviorEntry> {
    let mut entries = read_creature_behaviors_property(obj, "creature_behaviors");
    if entries.is_empty() {
        entries = legacy_npc_behaviors(obj);
    }
    entries
}

fn read_creature_behaviors_property(obj: &Object, name: &str) -> Vec<CreatureBehaviorEntry> {
    obj.get_property(name)
        .and_then(|prop| {
            if let Value::List(items) = &prop.value {
                Some(
                    items
                        .iter()
                        .filter_map(|entry| {
                            let Value::Map(map) = entry else {
                                return None;
                            };
                            entry_from_map(map)
                        })
                        .collect(),
                )
            } else {
                None
            }
        })
        .unwrap_or_default()
}

fn legacy_npc_behaviors(obj: &Object) -> Vec<CreatureBehaviorEntry> {
    obj.get_property("npc_behaviors")
        .and_then(|prop| {
            if let Value::List(items) = &prop.value {
                Some(
                    items
                        .iter()
                        .filter_map(|entry| {
                            let Value::Map(map) = entry else {
                                return None;
                            };
                            let event = map.get("event").and_then(|v| match v {
                                Value::String(s) => Some(s.clone()),
                                _ => None,
                            })?;
                            let action = map.get("action").and_then(|v| match v {
                                Value::String(s) => Some(s.clone()),
                                _ => None,
                            })?;
                            let text = map.get("text").and_then(|v| match v {
                                Value::String(s) => Some(s.clone()),
                                _ => None,
                            })?;
                            Some(CreatureBehaviorEntry {
                                entry_type: "script".to_string(),
                                template_name: None,
                                react: None,
                                event: Some(event),
                                action: Some(action),
                                text: Some(text),
                                wander_interval: None,
                                attack_damage: None,
                                awareness_check: None,
                                perception: None,
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

/// Attach a behavior template to a creature at runtime (builder command).
pub fn add_behavior_template(creature: &mut Object, template: &BehaviorTemplateDef) -> bool {
    let mut entries = read_creature_behaviors(creature);
    if entries.iter().any(|e| {
        e.entry_type == "template"
            && e.template_name.as_deref() == Some(template.base_name.as_str())
    }) {
        return false;
    }
    entries.push(template_to_entry(template));
    creature.add_property(creature_behaviors_to_property(&entries));
    true
}

/// Attach a scripted behavior line at runtime.
pub fn add_script_behavior(creature: &mut Object, script: &NpcBehaviorDef) {
    let mut entries = read_creature_behaviors(creature);
    entries.push(script_to_entry(script));
    creature.add_property(creature_behaviors_to_property(&entries));
}

/// List behavior summary lines for builder inspection.
pub fn format_creature_behavior_list(creature: &Object) -> String {
    let entries = read_creature_behaviors(creature);
    if entries.is_empty() {
        return format!("{} has no behaviors.", creature.name);
    }
    let mut lines = vec![format!("{} behaviors:", creature.name)];
    for (idx, entry) in entries.iter().enumerate() {
        match entry.entry_type.as_str() {
            "template" => {
                let react = entry.react.map(|r| r.as_str()).unwrap_or("ignore");
                lines.push(format!(
                    "  {}. template {} (react={react})",
                    idx + 1,
                    entry.template_name.as_deref().unwrap_or("?")
                ));
            }
            "script" => lines.push(format!(
                "  {}. script {} {} {}",
                idx + 1,
                entry.event.as_deref().unwrap_or("?"),
                entry.action.as_deref().unwrap_or("?"),
                entry.text.as_deref().unwrap_or("")
            )),
            other => lines.push(format!("  {}. {other}", idx + 1)),
        }
    }
    lines.join("\n")
}

fn npcs_in_room<'a>(
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
                && obj.has_creature_role()
                && obj.location.as_ref() == Some(room_id)
        })
        .collect()
}

fn format_script_line(npc: &Object, action: &str, text: &str) -> Option<String> {
    match action {
        "say" => Some(format!("{} says, \"{text}\"", npc.name)),
        "say_to" => Some(text.to_string()),
        "emote" => Some(format!("{} {text}", npc.name)),
        _ => None,
    }
}

fn behavior_enter_count(npc: &Object) -> u64 {
    npc.get_int_property("behavior_enter_count")
        .unwrap_or(0)
        .max(0) as u64
}

fn set_behavior_enter_count(npc: &mut Object, count: u64) {
    npc.set_property_int("behavior_enter_count", count as i64);
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

fn flee_npc(
    npc_id: &ObjectId,
    room_id: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
    outcome: &mut BehaviorOutcome,
) {
    let room = match objects.get(room_id) {
        Some(r) => r.clone(),
        None => return,
    };
    let exits: Vec<(String, ObjectId)> = room
        .get_exits()
        .into_iter()
        .filter_map(|(dir, target)| {
            objects.get(&target).and_then(|dest| {
                if dest.is_active() && dest.is_location() {
                    Some((dir, target))
                } else {
                    None
                }
            })
        })
        .collect();
    if exits.is_empty() {
        return;
    }
    let seed = mix_seed(&[npc_id.as_str(), room_id.as_str(), "flee"]);
    let pick = (seed as usize) % exits.len();
    let (_, dest_id) = &exits[pick];
    if let Some(npc) = objects.get_mut(npc_id) {
        npc.location = Some(dest_id.clone());
        outcome.mark_dirty(npc_id);
    }
}

/// Run composable creature behaviors for an event (e.g. `on_enter`).
pub fn run_creature_behaviors(
    event: &str,
    room_id: &ObjectId,
    player_id: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> BehaviorOutcome {
    let mut outcome = BehaviorOutcome::default();
    if event == "on_enter" {
        if let Some(player) = objects.get_mut(player_id) {
            reset_player_awareness_on_enter(player);
            outcome.mark_dirty(player_id);
        }
    }
    let npc_ids: Vec<ObjectId> = npcs_in_room(room_id, player_id, objects)
        .into_iter()
        .map(|npc| npc.id.clone())
        .collect();

    for npc_id in npc_ids {
        let Some(npc_snapshot) = objects.get(&npc_id).cloned() else {
            continue;
        };
        let tick = behavior_enter_count(&npc_snapshot) + 1;
        if let Some(npc) = objects.get_mut(&npc_id) {
            set_behavior_enter_count(npc, tick);
            outcome.mark_dirty(&npc_id);
        }

        let entries: Vec<CreatureBehaviorEntry> = read_creature_behaviors(&npc_snapshot)
            .into_iter()
            .filter(|e| e.event.as_deref() == Some(event))
            .collect();

        let mut aware = is_creature_aware(&npc_snapshot);
        if event == "on_enter" && uses_awareness_check(&npc_snapshot) {
            if let Some(encounter) = resolve_encounter_awareness_on_enter(
                &npc_id,
                player_id,
                room_id,
                tick,
                objects,
                anatomy,
            ) {
                for line in encounter.lines {
                    outcome.push_line(line);
                }
                aware = encounter.creature_aware;
                if let Some(npc) = objects.get_mut(&npc_id) {
                    set_creature_aware(npc, encounter.creature_aware);
                    outcome.mark_dirty(&npc_id);
                }
                if !encounter.player_aware {
                    if let Some(player) = objects.get_mut(player_id) {
                        set_player_aware(player, false);
                        outcome.mark_dirty(player_id);
                    }
                }
            }
        }

        let npc_snapshot = objects.get(&npc_id).cloned().unwrap_or(npc_snapshot);

        for entry in &entries {
            if !aware && uses_awareness_check(&npc_snapshot) {
                continue;
            }
            if entry.entry_type == "script" {
                if let (Some(action), Some(text)) = (&entry.action, &entry.text) {
                    if let Some(line) = format_script_line(&npc_snapshot, action, text) {
                        outcome.push_line(line);
                    }
                }
                continue;
            }
            if let (Some(action), Some(text)) = (&entry.action, &entry.text) {
                if action != "attack" && action != "flee" {
                    if let Some(line) = format_script_line(&npc_snapshot, action, text) {
                        outcome.push_line(line);
                    }
                }
            }
        }

        let reacts: Vec<CreatureReact> = entries.iter().filter_map(|e| e.react).collect();
        if reacts.is_empty() {
            continue;
        }

        if reacts.contains(&CreatureReact::Flee) {
            outcome.push_line(format!(
                "{} {}",
                npc_snapshot.name,
                entries
                    .iter()
                    .find(|e| e.react == Some(CreatureReact::Flee))
                    .and_then(|e| e.text.as_deref())
                    .unwrap_or("flees from your approach.")
            ));
            flee_npc(&npc_id, room_id, objects, &mut outcome);
            continue;
        }

        if reacts.contains(&CreatureReact::Attack) && aware {
            let base_damage = entries
                .iter()
                .filter_map(|e| e.attack_damage)
                .max()
                .unwrap_or(8)
                .max(1);
            let ambush = event == "on_enter"
                && objects
                    .get(player_id)
                    .is_some_and(|player| !is_player_aware(player));
            let damage = if ambush {
                base_damage.saturating_add(SURPRISE_DAMAGE_BONUS)
            } else {
                base_damage
            };
            if let Some(player) = objects.get_mut(player_id) {
                if player.has_creature_role() && creature_health(player) > 0 {
                    let after = apply_damage(player, damage);
                    set_player_aware(player, true);
                    outcome.mark_dirty(player_id);
                    if ambush {
                        outcome.push_line(format!(
                            "{} strikes from hiding for {damage} damage ({after} health remaining).",
                            npc_snapshot.name
                        ));
                    } else {
                        outcome.push_line(format!(
                            "{} attacks you for {damage} damage ({after} health remaining).",
                            npc_snapshot.name
                        ));
                    }
                }
            }
        } else if reacts.contains(&CreatureReact::Warn) && aware {
            let already_spoke = entries.iter().any(|e| {
                e.react == Some(CreatureReact::Warn)
                    && matches!(e.action.as_deref(), Some("say" | "emote" | "say_to"))
                    && e.text.as_deref().is_some_and(|t| !t.is_empty())
            });
            if !already_spoke {
                outcome.push_line(format!("{} eyes you warily.", npc_snapshot.name));
            }
        }

        if reacts.contains(&CreatureReact::Wander) {
            let interval = entries
                .iter()
                .filter_map(|e| e.wander_interval)
                .min()
                .unwrap_or(3)
                .max(1);
            if tick.is_multiple_of(u64::from(interval)) {
                let wander_text = entries
                    .iter()
                    .find(|e| e.react == Some(CreatureReact::Wander))
                    .and_then(|e| e.text.as_deref())
                    .unwrap_or("paces the area restlessly.");
                outcome.push_line(format!("{} {wander_text}", npc_snapshot.name));
            }
        }
    }

    outcome
}

/// Backward-compatible wrapper returning only narrative lines.
pub fn run_on_enter_creature_behaviors(
    room_id: &ObjectId,
    player_id: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> BehaviorOutcome {
    run_creature_behaviors("on_enter", room_id, player_id, objects, anatomy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::tactics::apply_tactics_from_behaviors;
    use crate::mudl::PlayerTemplate;
    use crate::object::PermissionFlags;

    fn template(name: &str, react: CreatureReact) -> BehaviorTemplateDef {
        BehaviorTemplateDef {
            base_name: name.to_string(),
            react,
            ..BehaviorTemplateDef::default()
        }
    }

    fn npc(id: &str, name: &str, room: &ObjectId) -> Object {
        let mut obj = Object {
            id: ObjectId::new(id),
            name: name.to_string(),
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
        obj.init_creature_role(&PlayerTemplate {
            name: "test".to_string(),
            creature: "human".to_string(),
            gender: "neutral".to_string(),
        });
        obj.set_property_int("health", 100);
        obj.set_property_int("max_health", 100);
        obj
    }

    #[test]
    fn composable_guard_and_script_both_fire() {
        let room = ObjectId::new("area:forest-path-001");
        let player_id = ObjectId::new("player:hero-001");
        let mut player = npc("player:hero-001", "Hero", &room);
        player.set_property_int("health", 100);

        let templates = HashMap::from([(
            "guard".to_string(),
            BehaviorTemplateDef {
                base_name: "guard".to_string(),
                react: CreatureReact::Warn,
                on_enter_action: Some("say".to_string()),
                on_enter_text: Some("Halt!".to_string()),
                ..BehaviorTemplateDef::default()
            },
        )]);
        let entries = build_creature_behavior_entries(
            &[NpcBehaviorDef {
                event: "on_enter".to_string(),
                action: "emote".to_string(),
                text: "narrows its eyes.".to_string(),
            }],
            &["guard".to_string()],
            &templates,
        );
        let mut watcher = npc("npc:watcher-001", "Watcher", &room);
        watcher.add_property(creature_behaviors_to_property(&entries));
        apply_tactics_from_behaviors(&mut watcher, &entries, &templates);

        let anatomy = AnatomyRegistry::default();
        let mut objects = HashMap::from([
            (player.id.clone(), player),
            (watcher.id.clone(), watcher),
            (
                room.clone(),
                Object {
                    id: room.clone(),
                    name: "Forest Path".to_string(),
                    aliases: Vec::new(),
                    location: None,
                    prototype: None,
                    owner: ObjectId::new("player:admin-001"),
                    permissions: PermissionFlags::EVERYONE,
                    properties: HashMap::from([(
                        "exits".to_string(),
                        Property {
                            name: "exits".to_string(),
                            value: Value::Map(HashMap::from([(
                                "north".to_string(),
                                Value::ObjectRef(ObjectId::new("area:north-001")),
                            )])),
                            permissions: PermissionFlags::EVERYONE,
                            behavior: None,
                        },
                    )]),
                    verbs: HashMap::new(),
                    event_handlers: HashMap::new(),
                    is_deleted: false,
                    deleted_at: None,
                },
            ),
        ]);

        let outcome =
            run_creature_behaviors("on_enter", &room, &player_id, &mut objects, &anatomy);
        assert!(outcome.lines.iter().any(|l| l.contains("narrows its eyes")));
        assert!(outcome.lines.iter().any(|l| l.contains("Halt")));
    }

    #[test]
    fn aggressive_behavior_damages_player() {
        let room = ObjectId::new("area:haunted-moon-001");
        let player_id = ObjectId::new("player:hero-001");
        let player = npc("player:hero-001", "Hero", &room);

        let templates = HashMap::from([(
            "aggressive".to_string(),
            BehaviorTemplateDef {
                base_name: "aggressive".to_string(),
                react: CreatureReact::Attack,
                attack_damage: 15,
                ..BehaviorTemplateDef::default()
            },
        )]);
        let entries = build_creature_behavior_entries(&[], &["aggressive".to_string()], &templates);
        let mut lurker = npc("npc:lurker-001", "Lurker", &room);
        lurker.add_property(creature_behaviors_to_property(&entries));
        apply_tactics_from_behaviors(&mut lurker, &entries, &templates);
        set_creature_aware(&mut lurker, true);

        let anatomy = AnatomyRegistry::default();
        let mut objects = HashMap::from([(player.id.clone(), player), (lurker.id.clone(), lurker)]);

        let outcome =
            run_creature_behaviors("on_enter", &room, &player_id, &mut objects, &anatomy);
        assert!(outcome.lines.iter().any(|l| l.contains("attacks you")));
        assert_eq!(creature_health(objects.get(&player_id).unwrap()), 85);
    }

    #[test]
    fn unaware_lurker_skips_attack_on_enter() {
        let room = ObjectId::new("area:haunted-moon-001");
        let player_id = ObjectId::new("player:hero-001");
        let mut player = npc("player:hero-001", "Hero", &room);
        player.set_int_map(
            "skills",
            HashMap::from([("stealth".to_string(), 8)]),
        );
        player.set_int_map(
            "stats",
            HashMap::from([("dexterity".to_string(), 14), ("wisdom".to_string(), 12)]),
        );

        let templates = HashMap::from([(
            "lurker".to_string(),
            BehaviorTemplateDef {
                base_name: "lurker".to_string(),
                react: CreatureReact::Attack,
                attack_damage: 12,
                awareness_check: Some(true),
                perception: Some(8),
                ..BehaviorTemplateDef::default()
            },
        )]);
        let entries = build_creature_behavior_entries(&[], &["lurker".to_string()], &templates);
        let mut lurker = npc("npc:lurker-001", "Pale Lurker", &room);
        lurker.add_property(creature_behaviors_to_property(&entries));
        apply_tactics_from_behaviors(&mut lurker, &entries, &templates);

        let anatomy = AnatomyRegistry::default();
        let mut objects = HashMap::from([(player.id.clone(), player), (lurker.id.clone(), lurker)]);

        let outcome =
            run_creature_behaviors("on_enter", &room, &player_id, &mut objects, &anatomy);
        assert!(outcome.lines.iter().any(|l| {
            l.contains("hasn't noticed you") || l.contains("before it sees you")
        }));
        assert!(!outcome.lines.iter().any(|l| l.contains("attacks you")));
        assert!(!outcome.lines.iter().any(|l| l.contains("ambushes you")));
        assert_eq!(creature_health(objects.get(&player_id).unwrap()), 100);
    }

    #[test]
    fn ambush_lurker_surprise_damages_player_on_enter() {
        let room = ObjectId::new("area:haunted-moon-001");
        let player_id = ObjectId::new("player:hero-001");
        let player = npc("player:hero-001", "Hero", &room);

        let templates = HashMap::from([(
            "lurker".to_string(),
            BehaviorTemplateDef {
                base_name: "lurker".to_string(),
                react: CreatureReact::Attack,
                attack_damage: 10,
                awareness_check: Some(true),
                perception: Some(14),
                ..BehaviorTemplateDef::default()
            },
        )]);
        let entries = build_creature_behavior_entries(&[], &["lurker".to_string()], &templates);
        let mut lurker = npc("npc:lurker-001", "Pale Lurker", &room);
        lurker.set_int_map(
            "stats",
            HashMap::from([("dexterity".to_string(), 16), ("wisdom".to_string(), 10)]),
        );
        lurker.set_int_map("skills", HashMap::from([("survival".to_string(), 8)]));
        lurker.add_property(creature_behaviors_to_property(&entries));
        apply_tactics_from_behaviors(&mut lurker, &entries, &templates);

        let anatomy = AnatomyRegistry::default();
        let mut objects = HashMap::from([(player.id.clone(), player), (lurker.id.clone(), lurker)]);

        let outcome =
            run_creature_behaviors("on_enter", &room, &player_id, &mut objects, &anatomy);
        assert!(outcome.lines.iter().any(|l| l.contains("ambushes you")));
        assert!(outcome
            .lines
            .iter()
            .any(|l| l.contains("strikes from hiding")));
        assert_eq!(creature_health(objects.get(&player_id).unwrap()), 87);
        assert!(crate::creature::tactics::is_player_aware(
            objects.get(&player_id).unwrap()
        ));
    }

    #[test]
    fn runtime_add_behavior_template_is_idempotent() {
        let mut creature = npc("npc:test-001", "Test", &ObjectId::new("area:room-001"));
        let t = template("passive", CreatureReact::Ignore);
        assert!(add_behavior_template(&mut creature, &t));
        assert!(!add_behavior_template(&mut creature, &t));
        assert_eq!(read_creature_behaviors(&creature).len(), 1);
    }
}
