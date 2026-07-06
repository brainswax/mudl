//! In-character room/area look output — description, exits, visible items.

use std::collections::HashMap;

use crate::object::{Object, ObjectId};
use crate::world::portal::{
    portal_for_direction, portal_kind_label, portal_passage_block, portals_in_room, PortalBlock,
};

use super::container::format_stackable_label;
use super::grammar::{indefinite_article, join_natural_list};
use super::DisplayContext;

fn passable_portal_exit_suffix(portal: &Object) -> Option<String> {
    if !portal.portal_passable() {
        return None;
    }
    let kind = portal_kind_label(portal);
    match portal_passage_block(portal) {
        Some(PortalBlock::Locked) => Some(format!("locked {kind}")),
        Some(PortalBlock::Closed) => Some(format!("closed {kind}")),
        None => None,
    }
}

fn exit_label(direction: &str, room: &Object, objects: &HashMap<ObjectId, Object>) -> String {
    let Some(portal) = portal_for_direction(&room.id, direction, objects) else {
        return direction.to_string();
    };
    if let Some(suffix) = passable_portal_exit_suffix(portal) {
        return format!("{direction} ({suffix})");
    }
    direction.to_string()
}

fn format_exits(room: &Object, objects: &HashMap<ObjectId, Object>) -> String {
    let exits = room.get_exits();
    if exits.is_empty() {
        return String::new();
    }
    let mut dirs: Vec<String> = exits
        .keys()
        .map(|dir| exit_label(dir, room, objects))
        .collect();
    dirs.sort_unstable();
    format!("Obvious exits: {}", dirs.join(", "))
}

fn portal_view_description(portal: &Object, objects: &HashMap<ObjectId, Object>) -> Option<String> {
    if !portal.portal_allows_view() {
        return None;
    }
    let dest_id = portal.portal_destination()?;
    let dest = objects.get(&dest_id)?;
    let text = dest
        .get_description()
        .map(|s| s.to_string())
        .unwrap_or_else(|| dest.name.clone())
        .trim()
        .to_string();
    if text.is_empty() {
        return None;
    }
    Some(text)
}

fn format_portal_view_line(portal: &Object, objects: &HashMap<ObjectId, Object>) -> Option<String> {
    let description = portal_view_description(portal, objects)?;
    let direction = portal
        .portal_direction()
        .unwrap_or_else(|| "that".to_string());
    let kind = portal_kind_label(portal);
    Some(format!(
        "Through the {direction} {kind} you see: {description}"
    ))
}

fn format_portal_views(room: &Object, objects: &HashMap<ObjectId, Object>) -> Vec<String> {
    portals_in_room(&room.id, objects)
        .into_iter()
        .filter_map(|portal| format_portal_view_line(portal, objects))
        .collect()
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

    let exits = format_exits(room, &ctx.objects);
    if !exits.is_empty() {
        lines.push(exits);
    }

    if !ctx.flags.contains(super::DisplayFlags::DARK) {
        for view in format_portal_views(room, &ctx.objects) {
            lines.push(view);
        }

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
    use crate::object::{DoorSpec, PermissionFlags, PortalKind, PortalSpec, Property, StackableSpec, Value};

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
    fn room_look_annotates_closed_and_locked_doors() {
        let room_id = ObjectId::new("area:cottage-front-001");
        let dest_id = ObjectId::new("area:cottage-interior-001");
        let mut room = bare_room("area:cottage-front-001", "Cottage Front", "A cottage.");
        room.add_exit("in", dest_id.clone());

        let mut locked_door = bare_item("item:door-001", "Wooden Door", &room_id);
        locked_door.apply_door_role(&DoorSpec {
            direction: "in".to_string(),
            destination: "cottage-interior".to_string(),
            open: false,
            lock_id: Some("cottage-door".to_string()),
            locked: true,
        });
        locked_door.set_portal_destination(dest_id);

        let mut objects = HashMap::new();
        objects.insert(room.id.clone(), room.clone());
        objects.insert(locked_door.id.clone(), locked_door);

        let ctx = DisplayContext::new(ObjectId::new("player:hero-001"), DisplayMode::Player)
            .with_objects(objects);
        let output = format_room_look_player(&room, &ctx);
        assert!(output.contains("Obvious exits: in (locked door)"));
    }

    #[test]
    fn room_look_shows_view_through_transparent_window() {
        let room_id = ObjectId::new("area:cottage-interior-001");
        let dest_id = ObjectId::new("area:cottage-rear-001");
        let room = bare_room(
            "area:cottage-interior-001",
            "Cottage Interior",
            "A warm room.",
        );
        let mut rear = bare_room(
            "area:cottage-rear-001",
            "Behind the Cottage",
            "Clutter and stacked firewood behind the cottage.",
        );
        rear.id = dest_id.clone();

        let mut window = bare_item("item:window-001", "Small Window", &room_id);
        window.apply_portal_role(&PortalSpec {
            kind: PortalKind::Window,
            direction: "east".to_string(),
            destination: "cottage-rear".to_string(),
            open: false,
            lock_id: None,
            locked: false,
            passable: None,
            transparent: None,
        });
        window.set_portal_destination(dest_id);

        let mut objects = HashMap::new();
        objects.insert(room.id.clone(), room.clone());
        objects.insert(rear.id.clone(), rear);
        objects.insert(window.id.clone(), window);

        let ctx = DisplayContext::new(ObjectId::new("player:hero-001"), DisplayMode::Player)
            .with_objects(objects);
        let output = format_room_look_player(&room, &ctx);
        assert!(output.contains("Through the east window you see:"));
        assert!(output.contains("stacked firewood"));
    }

    #[test]
    fn room_look_hides_view_through_locked_window() {
        let room_id = ObjectId::new("area:cottage-interior-001");
        let dest_id = ObjectId::new("area:cottage-rear-001");
        let room = bare_room("area:cottage-interior-001", "Cottage Interior", "A warm room.");
        let rear = bare_room(
            "area:cottage-rear-001",
            "Behind the Cottage",
            "Clutter behind the cottage.",
        );

        let mut window = bare_item("item:window-001", "Small Window", &room_id);
        window.apply_portal_role(&PortalSpec {
            kind: PortalKind::Window,
            direction: "east".to_string(),
            destination: "cottage-rear".to_string(),
            open: false,
            lock_id: Some("shutters".to_string()),
            locked: true,
            passable: None,
            transparent: None,
        });
        window.set_portal_destination(dest_id);

        let mut objects = HashMap::new();
        objects.insert(room.id.clone(), room.clone());
        objects.insert(rear.id.clone(), rear);
        objects.insert(window.id.clone(), window);

        let ctx = DisplayContext::new(ObjectId::new("player:hero-001"), DisplayMode::Player)
            .with_objects(objects);
        let output = format_room_look_player(&room, &ctx);
        assert!(!output.contains("Through the east window"));
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