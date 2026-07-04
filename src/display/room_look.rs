//! In-character room/area look output — description, exits, visible items.

use std::collections::HashMap;

use crate::object::{Object, ObjectId};

use super::container::format_stackable_label;
use super::grammar::{indefinite_article, join_natural_list};
use super::DisplayContext;

fn format_exits(exits: &HashMap<String, ObjectId>) -> String {
    if exits.is_empty() {
        return String::new();
    }
    let mut dirs: Vec<&str> = exits.keys().map(String::as_str).collect();
    dirs.sort_unstable();
    format!("Obvious exits: {}", dirs.join(", "))
}

/// Phrase for one visible object in a room listing.
fn room_item_phrase(item: &Object) -> String {
    let label = format_stackable_label(item);
    match item.object_type() {
        "player" => label,
        "item" | "thing" => {
            if item.is_stackable() && item.stack_count() > 1 {
                label
            } else {
                format!("{} {}", indefinite_article(&label), label)
            }
        }
        _ => label,
    }
}

fn visible_content_phrases(room: &Object, ctx: &DisplayContext) -> Vec<String> {
    let mut items: Vec<&Object> = room
        .contents(&ctx.objects)
        .into_iter()
        .filter(|item| item.id != ctx.observer)
        .collect();
    items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    items.iter().map(|item| room_item_phrase(item)).collect()
}

fn format_you_see_line(phrases: &[String]) -> String {
    if phrases.is_empty() {
        return String::new();
    }
    format!("You see {} here.", join_natural_list(phrases))
}

/// Player `look` / `examine` for rooms and areas (no leading room name).
pub fn format_room_look_player(room: &Object, ctx: &DisplayContext) -> String {
    let mut lines = Vec::new();

    if ctx.flags.contains(super::DisplayFlags::DARK) {
        lines.push("It is pitch black.".to_string());
    } else if let Some(desc) = room.get_description() {
        lines.push(desc);
    }

    let exits = format_exits(&room.get_exits());
    if !exits.is_empty() {
        lines.push(exits);
    }

    if !ctx.flags.contains(super::DisplayFlags::DARK) {
        let you_see = format_you_see_line(&visible_content_phrases(room, ctx));
        if !you_see.is_empty() {
            lines.push(you_see);
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::DisplayMode;
    use crate::object::{PermissionFlags, Property, StackableSpec, Value};

    fn bare_room(id: &str, name: &str, desc: &str) -> Object {
        let mut room = Object {
            id: ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new("player:hero-001"),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        room.add_property(Property {
            name: "description".to_string(),
            value: Value::String(desc.to_string()),
            permissions: PermissionFlags::EVERYONE,
            behavior: None,
        });
        room
    }

    fn bare_item(id: &str, name: &str, room_id: &ObjectId) -> Object {
        Object {
            id: ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: Some(room_id.clone()),
            prototype: None,
            owner: ObjectId::new("player:hero-001"),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        }
    }

    #[test]
    fn room_look_natural_visible_items_no_leading_name() {
        let room_id = ObjectId::new("area:the-void-001");
        let room = bare_room(
            "area:the-void-001",
            "The Void",
            "You are in a featureless void. This is the starting point for new players.",
        );
        let anvil = bare_item("item:anvil-001", "anvil", &room_id);
        let boulder = bare_item("item:boulder-001", "boulder", &room_id);

        let mut objects = HashMap::new();
        objects.insert(room.id.clone(), room.clone());
        objects.insert(anvil.id.clone(), anvil);
        objects.insert(boulder.id.clone(), boulder);

        let ctx = DisplayContext::new(ObjectId::new("player:hero-001"), DisplayMode::Player)
            .with_objects(objects);
        let output = format_room_look_player(&room, &ctx);

        assert_eq!(
            output,
            "You are in a featureless void. This is the starting point for new players.\n\
             You see an anvil and a boulder here."
        );
        assert!(!output.starts_with("The Void"));
        assert!(!output.contains("You see:"));
    }

    #[test]
    fn room_look_stackable_shows_quantity_without_article() {
        let room_id = ObjectId::new("room:void-001");
        let room = bare_room("room:void-001", "The Void", "Empty.");
        let mut coins = bare_item("item:coins-001", "coins", &room_id);
        coins.apply_stackable_role(&StackableSpec {
            count: 20,
            max_stack: 99,
        });

        let mut objects = HashMap::new();
        objects.insert(room.id.clone(), room.clone());
        objects.insert(coins.id.clone(), coins);

        let ctx = DisplayContext::new(ObjectId::new("player:hero-001"), DisplayMode::Player)
            .with_objects(objects);
        let output = format_room_look_player(&room, &ctx);

        assert!(output.contains("You see 20 coins here."));
    }

    #[test]
    fn room_look_includes_exits_and_hides_observer() {
        let room_id = ObjectId::new("room:garden-001");
        let mut room = bare_room("room:garden-001", "South Garden", "A peaceful garden.");
        room.add_exit("north", ObjectId::new("room:hub-001"));

        let observer = ObjectId::new("player:hero-001");
        let mut player = bare_item("player:hero-001", "Hero", &room_id);
        player.id = observer.clone();

        let mut objects = HashMap::new();
        objects.insert(room.id.clone(), room.clone());
        objects.insert(player.id.clone(), player);

        let ctx = DisplayContext::new(observer, DisplayMode::Player).with_objects(objects);
        let output = format_room_look_player(&room, &ctx);

        assert!(output.contains("Obvious exits: north"));
        assert!(!output.contains("Hero"));
        assert!(!output.contains("South Garden"));
    }
}