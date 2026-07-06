//! Immersive, MOO-style player-facing messages for command feedback.
//!
//! Technical details (IDs, types, persistence paths) belong in logs via `tracing`,
//! not in strings returned to players or builders. Only `@dump` and similar debug
//! commands should surface raw structures.

use std::collections::HashMap;

use crate::object::{Object, ObjectId, Value};

/// Resolve an object ID to a display name for narrative output.
pub fn object_name(id: &ObjectId, objects: &HashMap<ObjectId, Object>) -> String {
    objects
        .get(id)
        .map(|o| o.name.clone())
        .unwrap_or_else(|| "the unknown".to_string())
}

fn format_value_narrative(value: &Value, objects: &HashMap<ObjectId, Object>) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => crate::object::format_weight_amount(*f),
        Value::Bool(b) => b.to_string(),
        Value::ObjectRef(id) => object_name(id, objects),
        Value::List(items) => format!(
            "[{}]",
            items
                .iter()
                .map(|v| format_value_narrative(v, objects))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Map(map) => {
            let pairs: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", k, format_value_narrative(v, objects)))
                .collect();
            format!("{{{}}}", pairs.join(", "))
        }
    }
}

/// Format a property value for builder `examine` output (names, not raw IDs).
pub fn format_property_value(value: &Value, objects: &HashMap<ObjectId, Object>) -> String {
    format_value_narrative(value, objects)
}

fn location_phrase(location: Option<&Object>) -> String {
    match location {
        Some(loc) if loc.is_location() => format!(" in {}", loc.name),
        Some(loc) => format!(" near {}", loc.name),
        None => String::new(),
    }
}

fn creation_verb(obj_type: &str) -> &'static str {
    match obj_type {
        "player" | "npc" | "creature" => "summon",
        "container" | "backpack" | "chest" => "shape",
        "sword" | "weapon" | "shield" | "armor" => "forge",
        "room" | "area" | "location" => "open",
        _ => "conjure",
    }
}

fn creation_coda(obj_type: &str, has_location: bool) -> &'static str {
    match obj_type {
        "player" | "npc" | "creature" if has_location => " into being",
        "player" | "npc" | "creature" => " from nothing",
        "room" | "area" | "location" => " into the world",
        "container" => " before you",
        "sword" | "weapon" | "shield" => ", and it clatters to the ground",
        _ if has_location => ", and it settles onto the ground",
        _ => " from the ether",
    }
}

/// Player-facing success message for `create`.
pub fn narrate_create(obj: &Object, location: Option<&Object>) -> String {
    let verb = creation_verb(obj.object_type());
    let coda = creation_coda(obj.object_type(), location.is_some());
    let where_phrase = location_phrase(location);
    format!(
        "You {verb} {article} {name}{coda}{where}.",
        article = article_for(&obj.name),
        name = obj.name,
        coda = coda,
        where = where_phrase,
    )
}

/// Builder-facing success message for `create` (contextual, no IDs).
pub fn narrate_create_builder(obj: &Object, location: Option<&Object>) -> String {
    let where_phrase = location_phrase(location);
    format!(
        "You weave the essence of {} into being{}.",
        obj.name, where_phrase
    )
}

fn article_for(name: &str) -> &'static str {
    let first = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_ascii_lowercase());
    match first {
        Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    }
}

/// Player cannot see a resolved target.
pub fn narrate_target_not_found(target: &str) -> String {
    format!("You don't see anything like \"{target}\" here.")
}

/// No current location for a place-oriented command.
pub fn narrate_no_location() -> String {
    "You aren't anywhere in particular.".to_string()
}

/// No current location with a hint for builders.
pub fn narrate_no_location_builder(hint: &str) -> String {
    format!("You aren't anywhere in particular. {hint}")
}

/// Successful movement.
pub fn narrate_go(direction: &str) -> String {
    format!("You head {direction}.")
}

/// Movement while heavily laden (still succeeds).
pub fn narrate_go_encumbered(direction: &str) -> String {
    format!(
        "You are too encumbered to move easily.\n{}",
        narrate_go(direction)
    )
}

/// Movement blocked at or over carry capacity.
pub fn narrate_overloaded() -> String {
    "You are too overloaded to move.".to_string()
}

/// Blocked movement — no exit in that direction.
pub fn narrate_no_exit(direction: &str) -> String {
    format!("You can't go {direction} from here.")
}

/// Builder: field set on an object via `@set`.
pub fn narrate_field_set(obj: &Object, key: &str) -> String {
    format!("You set {key} on {}.", obj.name)
}

/// Builder: field removed from an object via `@unset`.
pub fn narrate_field_unset(obj: &Object, key: &str) -> String {
    format!("You clear {key} from {}.", obj.name)
}

/// Builder: property added to an object.
pub fn narrate_property_added(obj: &Object, prop_name: &str) -> String {
    format!("You inscribe \"{prop_name}\" upon {}.", obj.name)
}

/// Builder: verb added to an object.
pub fn narrate_verb_added(obj: &Object, verb_name: &str) -> String {
    format!("You teach {} the \"{verb_name}\" verb.", obj.name)
}

/// Wizard: soft-delete an object.
pub fn narrate_soft_delete(name: &str) -> String {
    format!("You unravel {name} from the fabric of the world.")
}

/// Wizard: restore a soft-deleted object.
pub fn narrate_restore(name: &str) -> String {
    format!("You restore {name} to the fabric of the world.")
}

/// Wizard: object not found (may need an ID for undelete).
pub fn narrate_wizard_not_found() -> String {
    "No such thing exists — or no longer does.".to_string()
}

/// Builder: object loaded from persistence into session.
pub fn narrate_loaded(name: &str) -> String {
    format!("You draw {name} from memory.")
}

/// Builder: object saved to persistence.
pub fn narrate_saved(name: &str) -> String {
    format!("You commit {name} to the archive.")
}

/// Builder: object not in session cache.
pub fn narrate_not_in_cache() -> String {
    "That isn't in your working memory — try drawing it from the archive first.".to_string()
}

/// Builder: module reloaded.
pub fn narrate_module_reloaded(universe: &str, world: &str) -> String {
    format!("Reality shimmers as \"{universe}\" / \"{world}\" reloads from disk.")
}

/// Builder: module bundled.
pub fn narrate_module_bundled(module_dir: &str, output_dir: &str, file_count: usize) -> String {
    format!("You bundle {file_count} files from \"{module_dir}\" into \"{output_dir}\".")
}

/// Immersive creation failure.
pub fn narrate_create_failed(reason: &str) -> String {
    format!("Your conjuration fizzles: {reason}")
}

/// Resolve an owner ID to a friendly label for builder examine.
pub fn owner_label(
    owner: &ObjectId,
    observer: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> String {
    if owner == observer {
        "you".to_string()
    } else {
        object_name(owner, objects)
    }
}

/// Resolve a location ID to a friendly label for builder examine.
pub fn location_label(location: &ObjectId, objects: &HashMap<ObjectId, Object>) -> String {
    object_name(location, objects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{generate_object_id, PermissionFlags, Property};

    fn void_room() -> Object {
        let mut area = Object {
            id: ObjectId::new("area:the-void-001"),
            name: "The Void".to_string(),
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
        };
        area.add_property(Property {
            name: "description".to_string(),
            value: Value::String("A featureless void.".to_string()),
            permissions: PermissionFlags::EVERYONE,
            behavior: None,
        });
        area
    }

    fn rusty_sword(owner: ObjectId) -> Object {
        Object {
            id: generate_object_id("sword", "rusty-sword", 1),
            name: "Rusty Sword".to_string(),
            aliases: Vec::new(),
            location: Some(ObjectId::new("area:the-void-001")),
            prototype: None,
            owner,
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        }
    }

    #[test]
    fn narrate_create_sword_is_immersive() {
        let owner = ObjectId::new("player:admin-001");
        let sword = rusty_sword(owner);
        let void = void_room();
        let msg = narrate_create(&sword, Some(&void));
        assert!(msg.contains("Rusty Sword"));
        assert!(msg.contains("The Void"));
        assert!(!msg.contains("sword:rusty-sword"));
        assert!(!msg.contains("area:the-void"));
    }

    #[test]
    fn narrate_create_builder_hides_ids() {
        let owner = ObjectId::new("player:admin-001");
        let sword = rusty_sword(owner);
        let void = void_room();
        let msg = narrate_create_builder(&sword, Some(&void));
        assert!(msg.contains("Rusty Sword"));
        assert!(msg.contains("The Void"));
        assert!(!msg.contains(':'));
    }

    #[test]
    fn object_name_resolves_from_map() {
        let owner = ObjectId::new("player:admin-001");
        let sword = rusty_sword(owner);
        let mut objects = HashMap::new();
        objects.insert(sword.id.clone(), sword.clone());
        assert_eq!(object_name(&sword.id, &objects), "Rusty Sword");
    }
}
