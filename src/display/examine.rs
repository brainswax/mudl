//! Builder `@examine` output — categorized, lowercase field names.

use std::collections::HashMap;

use crate::object::{
    is_state_property, is_unlimited_weight, player_carried_weight, Object, ObjectId,
};

use super::{
    format_property_value, location_label, owner_label, short_id, DisplayContext,
};

/// Preferred ordering for common config properties in `@examine`.
const CONFIG_PROPERTY_ORDER: &[&str] = &[
    "weight",
    "volume",
    "capacity",
    "max_weight",
    "max_volume",
    "is_container",
    "is_wearable",
    "is_pocketable",
    "wear_slot",
    "hand_slot",
    "stackable",
    "max_stack",
    "creature",
    "gender",
    "description",
    "exits",
];

fn push_header(lines: &mut Vec<String>, key: &str, value: &str) {
    lines.push(format!("{key}: {value}"));
}

fn push_section(lines: &mut Vec<String>, title: &str, entries: &[String]) {
    if entries.is_empty() {
        lines.push(format!("{title}: (none)"));
    } else {
        lines.push(format!("{title}:"));
        for entry in entries {
            lines.push(format!("  {entry}"));
        }
    }
}

/// Role-aware type label for builder examine (e.g. `container` instead of `item`).
pub fn builder_object_type(obj: &Object) -> &'static str {
    if obj.object_type() == "player" {
        return "player";
    }
    if obj.is_location() {
        return match obj.object_type() {
            "area" => "area",
            "region" => "region",
            "zone" => "zone",
            _ => "room",
        };
    }
    if obj.has_container_role() {
        return "container";
    }
    if obj.is_stackable() {
        return "stackable";
    }
    if obj.has_wearable_role() {
        return "wearable";
    }
    "item"
}

fn sort_config_property_names<'a>(names: Vec<&'a str>) -> Vec<&'a str> {
    let mut ordered: Vec<&'a str> = CONFIG_PROPERTY_ORDER
        .iter()
        .copied()
        .filter(|name| names.contains(name))
        .collect();
    let mut rest: Vec<&'a str> = names
        .into_iter()
        .filter(|name| !CONFIG_PROPERTY_ORDER.contains(name))
        .collect();
    rest.sort_unstable();
    ordered.append(&mut rest);
    ordered
}

fn format_config_properties(obj: &Object, objects: &HashMap<ObjectId, Object>) -> Vec<String> {
    let mut names: Vec<&str> = obj
        .properties
        .keys()
        .map(String::as_str)
        .filter(|name| !is_state_property(name))
        .collect();

    let needs_weight = !obj.is_location()
        && obj.object_type() != "player"
        && !names.contains(&"weight");
    if needs_weight {
        names.push("weight");
    }

    sort_config_property_names(names)
        .into_iter()
        .map(|name| {
            let value = if name == "weight" && !obj.properties.contains_key("weight") {
                obj.unit_weight().to_string()
            } else if let Some(prop) = obj.get_property(name) {
                format_property_value(&prop.value, objects)
            } else {
                obj.unit_weight().to_string()
            };
            format!("{name}: {value}")
        })
        .collect()
}

fn format_object_state_entries(obj: &Object, ctx: &DisplayContext) -> Vec<String> {
    let mut entries = vec![format!(
        "owner: {}",
        owner_label(&obj.owner, &ctx.observer, &ctx.objects)
    )];

    if let Some(loc) = &obj.location {
        entries.push(format!(
            "location: {}",
            location_label(loc, &ctx.objects)
        ));
    }

    if let Some(proto) = &obj.prototype {
        entries.push(format!("prototype: {}", short_id(proto)));
    }

    if !obj.aliases.is_empty() {
        entries.push(format!("aliases: {}", obj.aliases.join(", ")));
    }

    for name in ["contents", "body_slots", "stack_count", "carried_slot"] {
        if let Some(prop) = obj.get_property(name) {
            entries.push(format!(
                "{name}: {}",
                format_property_value(&prop.value, &ctx.objects)
            ));
        } else if name == "contents" && obj.is_container() {
            entries.push("contents: []".to_string());
        }
    }

    entries
}

fn format_contents_weight_status(obj: &Object, objects: &HashMap<ObjectId, Object>) -> String {
    let contents = obj.contents_weight(objects);
    match obj.container_max_weight() {
        Some(max) if is_unlimited_weight(max) => format!("{contents}/unlimited"),
        Some(max) => format!("{contents}/{max}"),
        None => contents.to_string(),
    }
}

fn format_carried_weight_status(obj: &Object, objects: &HashMap<ObjectId, Object>) -> String {
    let carried = player_carried_weight(obj, objects);
    match obj.get_int_property("max_weight") {
        Some(max) if is_unlimited_weight(max) => format!("{carried}/unlimited"),
        Some(max) => format!("{carried}/{max}"),
        None => carried.to_string(),
    }
}

fn format_status_entries(obj: &Object, objects: &HashMap<ObjectId, Object>) -> Vec<String> {
    let mut entries = Vec::new();

    if obj.is_stackable() {
        entries.push(format!("weight: {}", obj.weight()));
    } else if !obj.is_location() && obj.object_type() != "player" {
        entries.push(format!("weight: {}", obj.weight()));
    }

    if obj.is_container() {
        entries.push(format!(
            "contents_weight: {}",
            format_contents_weight_status(obj, objects)
        ));
        let total = obj.total_weight(objects);
        if total != obj.weight() {
            entries.push(format!("total_weight: {total}"));
        }
    }

    if obj.object_type() == "player" {
        entries.push(format!(
            "carried_weight: {}",
            format_carried_weight_status(obj, objects)
        ));
    }

    entries
}

fn format_room_state_entries(obj: &Object, ctx: &DisplayContext) -> Vec<String> {
    let mut entries = vec![format!(
        "owner: {}",
        owner_label(&obj.owner, &ctx.observer, &ctx.objects)
    )];

    let present: Vec<String> = obj
        .contents(&ctx.objects)
        .into_iter()
        .map(|item| item.name.clone())
        .collect();
    if present.is_empty() {
        entries.push("present: (none)".to_string());
    } else {
        entries.push(format!("present: {}", present.join(", ")));
    }

    entries
}

fn format_verbs_section(obj: &Object) -> Vec<String> {
    if obj.verbs.is_empty() {
        return vec!["verbs: (none)".to_string()];
    }
    let mut names: Vec<&str> = obj.verbs.keys().map(String::as_str).collect();
    names.sort_unstable();
    let mut lines = vec!["verbs:".to_string()];
    for name in names {
        let verb = &obj.verbs[name];
        lines.push(format!("  {name}: {}", verb.code));
    }
    lines
}

/// Categorized builder examine for rooms and areas.
pub fn format_builder_examine_room(obj: &Object, ctx: &DisplayContext) -> String {
    let mut lines = Vec::new();
    push_header(&mut lines, "name", &obj.name);
    push_header(&mut lines, "type", builder_object_type(obj));
    push_header(&mut lines, "id", &short_id(&obj.id));

    let config = format_config_properties(obj, &ctx.objects);
    push_section(&mut lines, "properties", &config);

    let state = format_room_state_entries(obj, ctx);
    push_section(&mut lines, "state", &state);

    lines.extend(format_verbs_section(obj));
    lines.join("\n")
}

/// Categorized builder examine for items, players, and other entities.
pub fn format_builder_examine_entity(obj: &Object, ctx: &DisplayContext) -> String {
    let mut lines = Vec::new();
    push_header(&mut lines, "name", &obj.name);
    push_header(&mut lines, "type", builder_object_type(obj));
    push_header(&mut lines, "id", &short_id(&obj.id));

    let config = format_config_properties(obj, &ctx.objects);
    push_section(&mut lines, "properties", &config);

    let state = format_object_state_entries(obj, ctx);
    push_section(&mut lines, "state", &state);

    let status = format_status_entries(obj, &ctx.objects);
    push_section(&mut lines, "status", &status);

    lines.extend(format_verbs_section(obj));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::DisplayMode;
    use crate::object::{ContainerSpec, PermissionFlags, StackableSpec};

    fn bare(id: &str, name: &str) -> Object {
        Object {
            id: ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new("player:admin-001"),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        }
    }

    #[test]
    fn builder_examine_empty_container_format() {
        let owner = ObjectId::new("player:admin-001");
        let mut backpack = bare("item:backpack-001", "backpack");
        backpack.set_property_int("weight", 10);
        backpack.apply_container_role(&ContainerSpec {
            capacity: 20,
            max_weight: Some(100),
            max_volume: None,
            wearable: true,
            wear_slot: Some("torso".to_string()),
        });

        let ctx = DisplayContext::new(owner, DisplayMode::Builder)
            .with_objects(HashMap::from([(backpack.id.clone(), backpack.clone())]));
        let output = format_builder_examine_entity(&backpack, &ctx);

        assert!(output.contains("name: backpack"));
        assert!(output.contains("type: container"));
        assert!(output.contains("id: backpack-001"));
        assert!(output.contains("properties:"));
        assert!(output.contains("weight: 10"));
        assert!(output.contains("state:"));
        assert!(output.contains("owner: you"));
        assert!(output.contains("contents: []"));
        assert!(output.contains("status:"));
        assert!(output.contains("contents_weight: 0/100"));
        assert!(output.contains("verbs: (none)"));
    }

    #[test]
    fn builder_examine_nonempty_container_lists_contents_in_state() {
        let owner = ObjectId::new("player:admin-001");
        let mut purse = bare("item:purse-001", "purse");
        purse.apply_container_role(&ContainerSpec {
            capacity: 3,
            max_weight: Some(10),
            max_volume: None,
            wearable: false,
            wear_slot: None,
        });

        let mut coins = bare("item:coins-001", "coins");
        coins.set_property_int("weight", 1);
        coins.apply_stackable_role(&StackableSpec {
            count: 10,
            max_stack: 99,
        });
        purse.set_property_list("contents", vec![coins.id.clone()]);

        let mut objects = HashMap::new();
        objects.insert(coins.id.clone(), coins);
        objects.insert(purse.id.clone(), purse.clone());

        let ctx = DisplayContext::new(owner, DisplayMode::Builder).with_objects(objects);
        let output = format_builder_examine_entity(&purse, &ctx);

        assert!(output.contains("state:"));
        assert!(output.contains("contents: [coins]"));
        assert!(output.contains("status:"));
        assert!(output.contains("contents_weight: 10/10"));
    }

    #[test]
    fn builder_examine_stackable_shows_unit_weight_in_properties() {
        let owner = ObjectId::new("player:admin-001");
        let mut coins = bare("item:coins-001", "coins");
        coins.set_property_int("weight", 2);
        coins.apply_stackable_role(&StackableSpec {
            count: 10,
            max_stack: 99,
        });

        let ctx = DisplayContext::new(owner, DisplayMode::Builder)
            .with_objects(HashMap::from([(coins.id.clone(), coins.clone())]));
        let output = format_builder_examine_entity(&coins, &ctx);

        assert!(output.contains("type: stackable"));
        assert!(output.contains("properties:"));
        assert!(output.contains("weight: 2"));
        assert!(output.contains("state:"));
        assert!(output.contains("stack_count: 10"));
        assert!(output.contains("status:"));
        assert!(output.contains("weight: 20"));
    }
}