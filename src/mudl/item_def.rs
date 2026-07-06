//! Item prototypes and spawn instances from MUDL `items.mudl` / `objects.mudl`.

use crate::mudl::MudlRoleProps;

/// Shared template for identical items (stored as a real object for inheritance).
#[derive(Debug, Clone, PartialEq)]
pub struct ItemPrototypeDef {
    pub base_name: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub aliases: Vec<String>,
    pub props: MudlRoleProps,
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
}

fn strip_comment(line: &str) -> &str {
    line.split(';').next().unwrap_or(line).trim()
}

fn parse_kv_line(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once('=')?;
    Some((key.trim().to_lowercase(), value.trim().to_string()))
}

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
    let refs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    MudlRoleProps::from_pairs(&refs)
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
            });
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
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let blade = instances
            .iter()
            .find(|i| i.base_name == "chest-chipped-blade")
            .unwrap();
        assert_eq!(blade.location, "scene-chest");
    }
}