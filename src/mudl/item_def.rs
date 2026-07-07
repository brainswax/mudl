//! Item prototypes and spawn instances from MUDL `items.mudl` / `objects.mudl`.

use crate::mudl::trigger_def::parse_trigger_line;
use crate::mudl::{MudlRoleProps, TriggerDef};

/// Shared template for identical items (stored as a real object for inheritance).
#[derive(Debug, Clone, PartialEq)]
pub struct ItemPrototypeDef {
    pub base_name: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub aliases: Vec<String>,
    pub props: MudlRoleProps,
    pub triggers: Vec<TriggerDef>,
}

/// A concrete item placed in the world at bootstrap.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemInstanceDef {
    pub base_name: String,
    pub prototype: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub aliases: Vec<String>,
    pub location: String,
    pub props: MudlRoleProps,
    pub triggers: Vec<TriggerDef>,
}

fn strip_comment(line: &str) -> &str {
    line.split(';').next().unwrap_or(line).trim()
}

fn parse_kv_line(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once('=')?;
    Some((key.trim().to_lowercase(), value.trim().to_string()))
}

#[allow(clippy::too_many_arguments)]
fn apply_kv(
    key: &str,
    value: &str,
    name: &mut Option<String>,
    description: &mut Option<String>,
    aliases: &mut Vec<String>,
    location: &mut Option<String>,
    prototype: &mut Option<String>,
    pairs: &mut Vec<(String, String)>,
) {
    match key {
        "name" => *name = Some(value.to_string()),
        "description" => *description = Some(value.to_string()),
        "location" => *location = Some(value.to_string()),
        "prototype" => *prototype = Some(value.to_string()),
        "alias" => aliases.push(value.to_string()),
        _ => pairs.push((key.to_string(), value.to_string())),
    }
}

fn pairs_to_props(pairs: &[(String, String)]) -> MudlRoleProps {
    let refs: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    MudlRoleProps::from_pairs(&refs)
}

fn parse_mod_stat_line(rest: &str) -> Option<(String, i64)> {
    let rest = rest.trim();
    if let Some((name, value)) = rest.split_once('=') {
        return Some((name.trim().to_string(), value.trim().parse().ok()?));
    }
    let mut parts = rest.split_whitespace();
    let name = parts.next()?.to_string();
    let value = parts.next()?.parse().ok()?;
    Some((name, value))
}

/// Parse `@prototype` and `@item` blocks from MUDL source.
pub fn parse_item_file(content: &str) -> (Vec<ItemPrototypeDef>, Vec<ItemInstanceDef>) {
    let mut prototypes = Vec::new();
    let mut instances = Vec::new();

    let mut current_proto: Option<ItemPrototypeDef> = None;
    let mut current_item: Option<ItemInstanceDef> = None;

    for raw_line in content.lines() {
        let line = strip_comment(raw_line);
        if line.is_empty() {
            continue;
        }

        if line == "@end" {
            if let Some(proto) = current_proto.take() {
                prototypes.push(proto);
            }
            if let Some(item) = current_item.take() {
                instances.push(item);
            }
            continue;
        }

        if let Some(name) = line.strip_prefix("@prototype ") {
            if let Some(proto) = current_proto.take() {
                prototypes.push(proto);
            }
            current_item = None;
            current_proto = Some(ItemPrototypeDef {
                base_name: name.trim().to_string(),
                name: None,
                description: None,
                aliases: Vec::new(),
                props: MudlRoleProps::default(),
                triggers: Vec::new(),
            });
            continue;
        }

        if let Some(name) = line.strip_prefix("@item ") {
            if let Some(item) = current_item.take() {
                instances.push(item);
            }
            current_proto = None;
            current_item = Some(ItemInstanceDef {
                base_name: name.trim().to_string(),
                prototype: None,
                name: None,
                description: None,
                aliases: Vec::new(),
                location: String::new(),
                props: MudlRoleProps::default(),
                triggers: Vec::new(),
            });
            continue;
        }

        if let Some(rest) = line.strip_prefix("@trigger ") {
            if let Some(trigger) = parse_trigger_line(rest) {
                if let Some(proto) = &mut current_proto {
                    proto.triggers.push(trigger);
                } else if let Some(item) = &mut current_item {
                    item.triggers.push(trigger);
                }
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("@mod-stat ") {
            if let Some((name, value)) = parse_mod_stat_line(rest) {
                if let Some(proto) = &mut current_proto {
                    proto.props.stat_mods.insert(name, value);
                } else if let Some(item) = &mut current_item {
                    item.props.stat_mods.insert(name, value);
                }
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("@mod-skill ") {
            if let Some((name, value)) = parse_mod_stat_line(rest) {
                if let Some(proto) = &mut current_proto {
                    proto.props.skill_mods.insert(name, value);
                } else if let Some(item) = &mut current_item {
                    item.props.skill_mods.insert(name, value);
                }
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("@grant-effect ") {
            let effect = rest.trim().to_string();
            if !effect.is_empty() {
                if let Some(proto) = &mut current_proto {
                    if !proto.props.grant_effects.contains(&effect) {
                        proto.props.grant_effects.push(effect);
                    }
                } else if let Some(item) = &mut current_item {
                    if !item.props.grant_effects.contains(&effect) {
                        item.props.grant_effects.push(effect);
                    }
                }
            }
            continue;
        }

        if let Some((key, value)) = parse_kv_line(line) {
            let mut pairs = Vec::new();
            if let Some(proto) = &mut current_proto {
                let mut location = None;
                let mut prototype = None;
                apply_kv(
                    &key,
                    &value,
                    &mut proto.name,
                    &mut proto.description,
                    &mut proto.aliases,
                    &mut location,
                    &mut prototype,
                    &mut pairs,
                );
                if !pairs.is_empty() {
                    let extra = pairs_to_props(&pairs);
                    merge_props(&mut proto.props, &extra);
                }
            } else if let Some(item) = &mut current_item {
                let mut loc = None;
                apply_kv(
                    &key,
                    &value,
                    &mut item.name,
                    &mut item.description,
                    &mut item.aliases,
                    &mut loc,
                    &mut item.prototype,
                    &mut pairs,
                );
                if let Some(loc) = loc {
                    item.location = loc;
                }
                if !pairs.is_empty() {
                    let extra = pairs_to_props(&pairs);
                    merge_props(&mut item.props, &extra);
                }
            }
        }
    }

    if let Some(proto) = current_proto {
        prototypes.push(proto);
    }
    if let Some(item) = current_item {
        instances.push(item);
    }

    (prototypes, instances)
}

fn merge_props(target: &mut MudlRoleProps, extra: &MudlRoleProps) {
    if extra.is_container.is_some() {
        target.is_container = extra.is_container;
    }
    if extra.is_open.is_some() {
        target.is_open = extra.is_open;
    }
    if extra.capacity.is_some() {
        target.capacity = extra.capacity;
    }
    if extra.max_weight.is_some() {
        target.max_weight = extra.max_weight;
    }
    if extra.max_volume.is_some() {
        target.max_volume = extra.max_volume;
    }
    if extra.is_wearable.is_some() {
        target.is_wearable = extra.is_wearable;
    }
    if extra.wear_slot.is_some() {
        target.wear_slot = extra.wear_slot.clone();
    }
    if extra.weight.is_some() {
        target.weight = extra.weight;
    }
    if extra.volume.is_some() {
        target.volume = extra.volume;
    }
    if extra.pocketable.is_some() {
        target.pocketable = extra.pocketable;
    }
    if extra.stackable.is_some() {
        target.stackable = extra.stackable;
    }
    if extra.stack_count.is_some() {
        target.stack_count = extra.stack_count;
    }
    if extra.max_stack.is_some() {
        target.max_stack = extra.max_stack;
    }
    if extra.hand_slot.is_some() {
        target.hand_slot = extra.hand_slot.clone();
    }
    if extra.readable.is_some() {
        target.readable = extra.readable;
    }
    if extra.read_text.is_some() {
        target.read_text = extra.read_text.clone();
    }
    if extra.writable.is_some() {
        target.writable = extra.writable;
    }
    if extra.write_text.is_some() {
        target.write_text = extra.write_text.clone();
    }
    if extra.locked.is_some() {
        target.locked = extra.locked;
    }
    if extra.lock_id.is_some() {
        target.lock_id = extra.lock_id.clone();
    }
    if extra.is_key.is_some() {
        target.is_key = extra.is_key;
    }
    if extra.key_consumable.is_some() {
        target.key_consumable = extra.key_consumable;
    }
    if extra.lock_consumable.is_some() {
        target.lock_consumable = extra.lock_consumable;
    }
    if extra.allowed_types.is_some() {
        target.allowed_types = extra.allowed_types.clone();
    }
    if extra.is_door.is_some() {
        target.is_door = extra.is_door;
    }
    if extra.door_direction.is_some() {
        target.door_direction = extra.door_direction.clone();
    }
    if extra.door_destination.is_some() {
        target.door_destination = extra.door_destination.clone();
    }
    if extra.is_window.is_some() {
        target.is_window = extra.is_window;
    }
    if extra.portal_kind.is_some() {
        target.portal_kind = extra.portal_kind.clone();
    }
    if extra.portal_passable.is_some() {
        target.portal_passable = extra.portal_passable;
    }
    if extra.portal_transparent.is_some() {
        target.portal_transparent = extra.portal_transparent;
    }
    if extra.mod_max_weight.is_some() {
        target.mod_max_weight = extra.mod_max_weight;
    }
    if extra.mod_encumbrance.is_some() {
        target.mod_encumbrance = extra.mod_encumbrance;
    }
    if extra.mod_max_health.is_some() {
        target.mod_max_health = extra.mod_max_health;
    }
    for (stat, value) in &extra.stat_mods {
        target.stat_mods.insert(stat.clone(), *value);
    }
    for (skill, value) in &extra.skill_mods {
        target.skill_mods.insert(skill.clone(), *value);
    }
    for effect in &extra.grant_effects {
        if !target.grant_effects.contains(effect) {
            target.grant_effects.push(effect.clone());
        }
    }
    if extra.breakable.is_some() {
        target.breakable = extra.breakable;
    }
    if extra.break_text.is_some() {
        target.break_text = extra.break_text.clone();
    }
    if extra.harvestable.is_some() {
        target.harvestable = extra.harvestable;
    }
    if extra.hidden_until_discovered.is_some() {
        target.hidden_until_discovered = extra.hidden_until_discovered;
    }
    if extra.discovery_stealth.is_some() {
        target.discovery_stealth = extra.discovery_stealth;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_haunted_forest_consumable_prototypes() {
        let content = include_str!(
            "../../modules/default/worlds/default_world/expansions/haunted_forest/haunted_forest.mudl"
        );
        let (prototypes, _) = parse_item_file(content);
        let charm = prototypes
            .iter()
            .find(|p| p.base_name == "whisper-charm")
            .expect("whisper-charm prototype");
        assert_eq!(charm.props.key_consumable, Some(true));

        let oak = prototypes
            .iter()
            .find(|p| p.base_name == "hollow-oak-portal")
            .expect("hollow-oak-portal prototype");
        assert_eq!(oak.props.lock_consumable, Some(true));

        let pot = prototypes
            .iter()
            .find(|p| p.base_name == "haunted-clay-pot")
            .expect("haunted-clay-pot prototype");
        assert_eq!(pot.triggers.len(), 2);
        assert_eq!(pot.triggers[0].event, "on_break");
        assert_eq!(pot.triggers[0].code, "emote shatters into pale dust.");
        assert_eq!(pot.triggers[1].event, "on_break");
        assert_eq!(pot.triggers[1].code, "grant-effect actor forest_dread");
    }

    #[test]
    fn parse_starting_scene_prototypes_and_instances() {
        let content = include_str!("../../modules/default/worlds/default_world/items.mudl");
        let (prototypes, instances) = parse_item_file(content);

        assert!(prototypes.iter().any(|p| p.base_name == "worn-mailbox"));
        assert!(prototypes.iter().any(|p| p.base_name == "travel-chest"));
        assert!(prototypes.iter().any(|p| p.base_name == "chipped-blade"));

        let mailbox = instances
            .iter()
            .find(|i| i.base_name == "scene-mailbox")
            .unwrap();
        assert_eq!(mailbox.location, "the-void");
        assert_eq!(mailbox.prototype.as_deref(), Some("worn-mailbox"));

        let key = instances
            .iter()
            .find(|i| i.base_name == "mailbox-brass-key")
            .unwrap();
        assert_eq!(key.location, "scene-mailbox");

        let note = instances
            .iter()
            .find(|i| i.base_name == "mailbox-folded-note")
            .unwrap();
        assert_eq!(note.location, "scene-mailbox");

        let blade = instances
            .iter()
            .find(|i| i.base_name == "chest-chipped-blade")
            .unwrap();
        assert_eq!(blade.location, "scene-chest");

        let mailbox_proto = prototypes
            .iter()
            .find(|p| p.base_name == "worn-mailbox")
            .unwrap();
        assert_eq!(mailbox_proto.props.is_container, Some(true));
        assert_eq!(mailbox_proto.props.is_open, Some(false));

        let chest_proto = prototypes
            .iter()
            .find(|p| p.base_name == "travel-chest")
            .unwrap();
        assert_eq!(chest_proto.props.is_open, Some(false));
        assert_eq!(chest_proto.props.locked, Some(true));
        assert_eq!(chest_proto.props.lock_id.as_deref(), Some("chest-lock"));

        let key_proto = prototypes
            .iter()
            .find(|p| p.base_name == "brass-key")
            .unwrap();
        assert_eq!(key_proto.props.is_key, Some(true));
        assert_eq!(key_proto.props.lock_id.as_deref(), Some("chest-lock"));

        let ring_proto = prototypes
            .iter()
            .find(|p| p.base_name == "brass-key-ring")
            .unwrap();
        assert_eq!(ring_proto.props.allowed_types.as_deref(), Some("key"));

        let ring = instances
            .iter()
            .find(|i| i.base_name == "cottage-key-ring")
            .unwrap();
        assert_eq!(ring.location, "cottage-interior");

        let blade_proto = prototypes
            .iter()
            .find(|p| p.base_name == "chipped-blade")
            .unwrap();
        assert_eq!(
            blade_proto.props.stat_mods.get("strength").copied(),
            Some(2)
        );

        let vest_proto = prototypes
            .iter()
            .find(|p| p.base_name == "leather-vest")
            .unwrap();
        assert_eq!(vest_proto.props.mod_max_health, Some(5));
        assert_eq!(
            vest_proto.props.stat_mods.get("constitution").copied(),
            Some(2)
        );
        assert_eq!(
            vest_proto.props.skill_mods.get("survival").copied(),
            Some(1)
        );

        let lantern_proto = prototypes
            .iter()
            .find(|p| p.base_name == "iron-lantern")
            .unwrap();
        assert_eq!(
            lantern_proto.props.grant_effects,
            vec!["iron_lantern_aura".to_string()]
        );

        let note_proto = prototypes
            .iter()
            .find(|p| p.base_name == "folded-note")
            .unwrap();
        assert_eq!(note_proto.props.readable, Some(true));
        assert_eq!(
            note_proto.props.read_text.as_deref(),
            Some("Mind the dark below — take the lantern first.")
        );

        let mailbox_proto = prototypes
            .iter()
            .find(|p| p.base_name == "worn-mailbox")
            .unwrap();
        assert!(mailbox_proto.props.read_text.is_some());
    }
}
