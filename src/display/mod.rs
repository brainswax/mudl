use bitflags::bitflags;
use std::collections::HashMap;

use crate::mudl::AnatomyRegistry;
use crate::object::{Object, ObjectId};

pub mod narrative;
pub use narrative::{
    format_property_value, location_label, narrate_create, narrate_create_builder,
    narrate_go, narrate_module_bundled, narrate_module_reloaded, narrate_no_exit,
    narrate_loaded, narrate_no_location, narrate_no_location_builder, narrate_not_in_cache,
    narrate_property_added, narrate_restore, narrate_saved, narrate_soft_delete,
    narrate_target_not_found,
    narrate_verb_added, narrate_wizard_not_found, object_name, owner_label,
};

/// How an object should be rendered for a given command/audience.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayMode {
    /// Clean, immersive output for normal play.
    Player,
    /// Builder/wizard mode: shows ownership, properties, etc.
    Builder,
    /// Full internal dump (for debugging/coding).
    Debug,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct DisplayFlags: u32 {
        const DARK = 1 << 0;
        const BRIEF = 1 << 1;
    }
}

/// Context passed to [`Describable`] implementations for rendering.
#[derive(Debug, Clone)]
pub struct DisplayContext {
    /// Rendering mode.
    pub mode: DisplayMode,
    /// Who is observing (for permission checks, personalization).
    pub observer: ObjectId,
    /// Recursion/detail level.
    pub depth: u8,
    /// Additional flags (dark room, etc.).
    pub flags: DisplayFlags,
    /// Known objects for resolving contents, exits, and name lookups.
    pub objects: HashMap<ObjectId, Object>,
    /// Loaded anatomy definitions for body-slot descriptions.
    pub anatomy: AnatomyRegistry,
}

impl DisplayContext {
    pub fn new(observer: ObjectId, mode: DisplayMode) -> Self {
        Self {
            mode,
            observer,
            depth: 0,
            flags: DisplayFlags::empty(),
            objects: HashMap::new(),
            anatomy: AnatomyRegistry::default(),
        }
    }

    pub fn with_objects(mut self, objects: HashMap<ObjectId, Object>) -> Self {
        self.objects = objects;
        self
    }

    pub fn with_anatomy(mut self, anatomy: AnatomyRegistry) -> Self {
        self.anatomy = anatomy;
        self
    }

    pub fn lookup(&self, id: &ObjectId) -> Option<&Object> {
        self.objects.get(id)
    }
}

/// Presentation layer for objects — separates player-facing output from builder/debug views.
pub trait Describable {
    /// Basic description suitable for "look".
    fn describe(&self, ctx: &DisplayContext) -> String;

    /// Detailed view (exits, contents, properties).
    fn describe_detailed(&self, ctx: &DisplayContext) -> String;

    /// Full internal representation (Debug mode).
    fn dump(&self) -> String;
}

/// Resolve a player-facing target name to an object ID.
///
/// Supports full IDs, friendly names, aliases, and the special target `here`.
pub fn resolve_target(
    name: &str,
    current_location: Option<&ObjectId>,
    observer: Option<&ObjectId>,
    objects: &HashMap<ObjectId, Object>,
) -> Option<ObjectId> {
    let needle = name.to_lowercase();

    if needle == "here" {
        return current_location.cloned();
    }

    if needle == "self" || needle == "me" {
        return observer.cloned();
    }

    let id = ObjectId::new(name);
    if objects.contains_key(&id) {
        return Some(id);
    }

    for (obj_id, obj) in objects {
        if !obj.is_active() {
            continue;
        }
        if obj.name.to_lowercase() == needle {
            return Some(obj_id.clone());
        }
        for alias in &obj.aliases {
            if alias.to_lowercase() == needle {
                return Some(obj_id.clone());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mudl::load_module;
    use crate::object::{
        generate_object_id, ObjectFactory, PermissionFlags, Property, Value, Verb,
    };
    use crate::persistence::SqlitePersistence;

    async fn test_factory() -> ObjectFactory<SqlitePersistence> {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        ObjectFactory::new(persistence)
    }

    fn make_room(id: &str, name: &str, desc: &str, owner: ObjectId) -> Object {
        let mut room = Object {
            id: ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: owner.clone(),
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

    #[tokio::test]
    async fn describe_area_lists_ground_items() {
        let factory = test_factory().await;
        let owner = ObjectId::new("player:admin-001");
        let area_id = ObjectId::new("area:the-void-001");
        let mut area = make_room(
            "area:the-void-001",
            "The Void",
            "A featureless void.",
            owner.clone(),
        );
        area.add_exit("north", ObjectId::new("area:passage-001"));

        let mut boots = factory
            .create("item", "boots", owner.clone())
            .await
            .unwrap();
        boots.name = "Boots".to_string();
        boots.location = Some(area_id.clone());

        let mut objects = HashMap::new();
        objects.insert(area.id.clone(), area.clone());
        objects.insert(boots.id.clone(), boots);

        let ctx = DisplayContext::new(owner, DisplayMode::Player).with_objects(objects);
        let output = area.describe(&ctx);

        assert!(output.contains("The Void"));
        assert!(output.contains("featureless void"));
        assert!(output.contains("Obvious exits: north"));
        assert!(output.contains("You see: Boots"));
    }

    #[tokio::test]
    async fn describe_room_player_mode() {
        let factory = test_factory().await;
        let owner = ObjectId::new("player:admin-001");
        let room_id = ObjectId::new("room:garden-001");
        let mut room = make_room(
            "room:garden-001",
            "South Garden",
            "A peaceful garden full of flowers.",
            owner.clone(),
        );
        room.add_exit("north", ObjectId::new("room:hub-001"));

        let mut daisy = factory
            .create("item", "daisy", owner.clone())
            .await
            .unwrap();
        daisy.name = "Daisy".to_string();
        daisy.location = Some(room_id.clone());

        let mut objects = HashMap::new();
        objects.insert(room.id.clone(), room.clone());
        objects.insert(daisy.id.clone(), daisy);

        let ctx = DisplayContext::new(owner, DisplayMode::Player).with_objects(objects);
        let output = room.describe(&ctx);

        assert!(output.contains("South Garden"));
        assert!(output.contains("peaceful garden"));
        assert!(output.contains("Obvious exits: north"));
        assert!(output.contains("Daisy"));
        assert!(!output.contains("room:garden-001"));
        assert!(!output.contains("player:admin-001"));
    }

    #[tokio::test]
    async fn describe_room_builder_mode() {
        let owner = ObjectId::new("player:admin-001");
        let room = make_room(
            "room:garden-001",
            "South Garden",
            "A peaceful garden.",
            owner.clone(),
        );

        let mut objects = HashMap::new();
        objects.insert(room.id.clone(), room.clone());
        let ctx = DisplayContext::new(owner.clone(), DisplayMode::Builder).with_objects(objects);
        let output = room.describe_detailed(&ctx);

        assert!(output.contains("South Garden"));
        assert!(output.contains("Owner: you"));
        assert!(!output.contains("room:garden-001"));
        assert!(!output.contains("player:admin-001"));
    }

    #[tokio::test]
    async fn describe_item_debug_mode() {
        let factory = test_factory().await;
        let owner = ObjectId::new("player:admin-001");
        let item = factory.create("item", "sword", owner).await.unwrap();
        let output = item.dump();

        assert!(output.contains("\"id\""));
        assert!(output.contains("item:sword-001"));
    }

    #[tokio::test]
    async fn resolve_target_by_name_and_here() {
        let owner = ObjectId::new("player:admin-001");
        let room_id = ObjectId::new("room:garden-001");
        let room = make_room("room:garden-001", "South Garden", "Flowers.", owner);

        let mut objects = HashMap::new();
        objects.insert(room_id.clone(), room);

        assert_eq!(
            resolve_target("here", Some(&room_id), Some(&room_id), &objects),
            Some(room_id.clone())
        );
        assert_eq!(
            resolve_target("South Garden", None, None, &objects),
            Some(room_id.clone())
        );
        assert_eq!(resolve_target("missing", None, None, &objects), None);
        let player_id = ObjectId::new("player:admin-001");
        assert_eq!(
            resolve_target("self", None, Some(&player_id), &objects),
            Some(player_id)
        );
    }

    #[test]
    fn player_describe_hides_internal_ids() {
        let owner = ObjectId::new("player:admin-001");
        let mut player = Object {
            id: generate_object_id("player", "admin", 1),
            name: "Admin".to_string(),
            aliases: vec!["brains".to_string()],
            location: Some(ObjectId::new("room:void-001")),
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        player.add_property(Property {
            name: "description".to_string(),
            value: Value::String("A weary adventurer.".to_string()),
            permissions: PermissionFlags::EVERYONE,
            behavior: None,
        });

        let anatomy = load_module("modules/default")
            .unwrap()
            .active_world()
            .unwrap()
            .anatomy
            .clone();
        player.init_body(anatomy.player_template("default").unwrap());

        let ctx = DisplayContext::new(owner.clone(), DisplayMode::Player).with_anatomy(anatomy);
        let output = player.describe(&ctx);

        assert!(output.contains("Admin"));
        assert!(output.contains("weary adventurer"));
        assert!(output.contains("completely naked and empty-handed"));
        assert!(!output.contains("player:admin-001"));
    }

    #[test]
    fn player_describe_detailed_shows_builder_info() {
        let owner = ObjectId::new("player:admin-001");
        let mut player = Object {
            id: generate_object_id("player", "admin", 1),
            name: "Admin".to_string(),
            aliases: Vec::new(),
            location: Some(ObjectId::new("room:void-001")),
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        player.add_verb(Verb {
            name: "wave".to_string(),
            code: "say('You wave.')".to_string(),
            permissions: PermissionFlags::EVERYONE,
        });

        let mut objects = HashMap::new();
        objects.insert(player.id.clone(), player.clone());
        let ctx = DisplayContext::new(owner.clone(), DisplayMode::Builder).with_objects(objects);
        let output = player.describe_detailed(&ctx);

        assert!(output.contains("Admin"));
        assert!(output.contains("Owner: you"));
        assert!(output.contains("wave"));
        assert!(!output.contains("player:admin-001"));
    }
}
