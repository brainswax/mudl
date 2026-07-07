use bitflags::bitflags;
use std::collections::HashMap;

use crate::mudl::AnatomyRegistry;
use crate::object::{Object, ObjectId};

pub mod body_plan;
pub mod carried;
pub mod container;
pub mod creature;
pub mod equipment;
pub mod examine;
pub mod examine_target;
pub mod grammar;
pub mod narrative;
pub mod object_look;
pub mod readable;
pub mod resolve;
pub mod room_look;
pub mod self_examine;
pub mod stackable;
pub mod weight;
pub use body_plan::{creature_definition, format_body_detail_player};
pub use carried::format_look_self_summary;
pub use container::{
    container_content_labels, format_container_contents_builder, format_examine_container_player,
    format_inside_container, format_look_container_player, format_open_container_message,
};
pub use examine::{
    builder_object_type, format_builder_examine_entity, format_builder_examine_room,
    format_prototype_examine_builder, format_prototype_examine_player,
};
pub use examine_target::{
    format_examine_output, format_no_parent_message, parse_examine_request,
    resolve_examine_request, ExamineError, ExamineRequest, ExamineResolution, ExamineTarget,
};
pub use narrative::{
    format_property_value, location_label, narrate_create, narrate_create_builder, narrate_dig,
    narrate_field_set, narrate_field_unset, narrate_go, narrate_go_encumbered, narrate_link,
    narrate_loaded, narrate_module_bundled, narrate_module_reloaded, narrate_no_exit,
    narrate_no_location, narrate_no_location_builder, narrate_not_in_cache, narrate_overloaded,
    narrate_property_added, narrate_restore, narrate_saved, narrate_scatter_exit,
    narrate_soft_delete, narrate_target_not_found, narrate_verb_added, narrate_wizard_not_found,
    object_name, owner_label,
};
pub use object_look::{format_look_item_player, format_look_object_player};
pub use readable::{effective_read_text, format_read_message};
pub use resolve::{
    format_disambiguation, is_in_player_possession, name_matches, resolve_object, short_id,
    ResolveScope, ResolvedMatch, TargetResolution,
};
pub use room_look::format_room_look_player;
pub use self_examine::format_examine_self;
pub use stackable::{
    display_name_for_single_unit, format_examine_stack_weight, format_examine_stackable_fallback,
    format_look_stackable_sentence, format_stack_transfer_message, format_stackable_label,
    item_lookup_variants, name_looks_plural, pluralize_item_name, singularize_item_name,
    stack_quantity_phrase, StackRemainderLocation,
};
pub use weight::format_examine_item_player;
pub use weight::{format_weight_examine_builder, format_weight_examine_player};

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

    pub fn with_flags(mut self, flags: DisplayFlags) -> Self {
        self.flags = flags;
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
/// Uses possession-first search with room and global fallback. Returns `None` when
/// the target is missing or ambiguous (use [`resolve_object`] for disambiguation text).
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

    if let Some(observer) = observer {
        match resolve_object(
            name,
            observer,
            current_location,
            objects,
            ResolveScope::General,
        ) {
            TargetResolution::Found(id) => Some(id),
            TargetResolution::Ambiguous(_) | TargetResolution::NotFound => None,
        }
    } else {
        for (obj_id, obj) in objects {
            if !obj.is_active() {
                continue;
            }
            if name_matches(&needle, obj) {
                return Some(obj_id.clone());
            }
        }
        None
    }
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
            revision: 0,
            updated_at: None,
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

        assert!(!output.starts_with("The Void"));
        assert!(output.contains("featureless void"));
        assert!(output.contains("Obvious exits: north"));
        assert!(output.contains("You see a Boots here."));
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

        assert!(!output.starts_with("South Garden"));
        assert!(output.contains("peaceful garden"));
        assert!(output.contains("Obvious exits: north"));
        assert!(output.contains("You see a Daisy here."));
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

        assert!(output.contains("name: South Garden"));
        assert!(output.contains("type: room"));
        assert!(output.contains("state:"));
        assert!(output.contains("owner: you"));
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
            revision: 0,
            updated_at: None,
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
        player.init_creature_role(anatomy.player_template("default").unwrap());

        let ctx = DisplayContext::new(owner.clone(), DisplayMode::Player)
            .with_anatomy(anatomy)
            .with_flags(DisplayFlags::BRIEF);
        let output = player.describe(&ctx);

        assert!(!output.contains("Admin"));
        assert!(!output.contains("weary adventurer"));
        assert!(output.contains("aren't holding or wearing anything"));
        assert!(!output.contains("player:admin-001"));
    }

    #[test]
    fn in_game_examine_hides_internal_fields() {
        let owner = ObjectId::new("player:admin-001");
        let mut item = Object {
            id: generate_object_id("item", "coins", 1),
            name: "coins".to_string(),
            aliases: Vec::new(),
            location: Some(ObjectId::new("room:void-001")),
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        item.apply_stackable_role(&crate::object::StackableSpec {
            count: 20,
            max_stack: 99,
        });
        item.add_property(Property {
            name: "description".to_string(),
            value: Value::String("Gold coins glint in the light.".to_string()),
            permissions: PermissionFlags::EVERYONE,
            behavior: None,
        });
        item.add_verb(Verb {
            name: "flip".to_string(),
            code: "say('tails')".to_string(),
            permissions: PermissionFlags::EVERYONE,
        });

        let mut objects = HashMap::new();
        objects.insert(item.id.clone(), item.clone());
        let ctx = DisplayContext::new(owner, DisplayMode::Player).with_objects(objects);
        let output = item.describe(&ctx);

        assert!(output.contains("Gold coins glint"));
        assert!(output.contains("weighs 20 in total"));
        assert!(!output.contains("id:"));
        assert!(!output.contains("properties:"));
        assert!(!output.contains("flip"));
    }

    #[test]
    fn look_self_shows_holding_and_wearing_summary() {
        use crate::mudl::load_module;

        let anatomy = load_module("modules/default")
            .unwrap()
            .active_world()
            .unwrap()
            .anatomy
            .clone();
        let owner = ObjectId::new("player:admin-001");
        let mut player = Object {
            id: owner.clone(),
            name: "Admin".to_string(),
            aliases: Vec::new(),
            location: Some(ObjectId::new("room:void-001")),
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        player.init_creature_role(anatomy.player_template("default").unwrap());

        let mut purse = Object {
            id: ObjectId::new("item:purse-001"),
            name: "purse".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        purse.apply_container_role(&crate::object::ContainerSpec {
            capacity: 3,
            max_weight: Some(10),
            max_volume: None,
            wearable: false,
            wear_slot: None,
            ..crate::object::ContainerSpec::default()
        });

        let mut backpack = Object {
            id: ObjectId::new("item:backpack-001"),
            name: "backpack".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        backpack.apply_container_role(&crate::object::ContainerSpec {
            capacity: 5,
            max_weight: None,
            max_volume: None,
            wearable: true,
            wear_slot: Some("torso".to_string()),
            ..crate::object::ContainerSpec::default()
        });

        player.set_body_slot("right_hand", Some(purse.id.clone()));
        player.set_body_slot("torso", Some(backpack.id.clone()));

        let mut objects = HashMap::new();
        objects.insert(purse.id.clone(), purse);
        objects.insert(backpack.id.clone(), backpack);
        objects.insert(player.id.clone(), player.clone());

        let ctx = DisplayContext::new(owner.clone(), DisplayMode::Player)
            .with_objects(objects)
            .with_anatomy(anatomy)
            .with_flags(DisplayFlags::BRIEF);
        let output = player.describe(&ctx);

        assert!(output.contains("You are holding a purse and wearing a backpack."));
        assert!(!output.starts_with("Admin"));
        assert!(!output.contains("right hand"));
        assert!(!output.contains("weighs"));
    }

    #[test]
    fn look_empty_container_shows_empty_message() {
        let owner = ObjectId::new("player:admin-001");
        let mut backpack = Object {
            id: ObjectId::new("item:backpack-001"),
            name: "backpack".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        backpack.apply_container_role(&crate::object::ContainerSpec {
            capacity: 20,
            max_weight: Some(100),
            max_volume: None,
            wearable: true,
            wear_slot: Some("torso".to_string()),
            ..crate::object::ContainerSpec::default()
        });

        let mut objects = HashMap::new();
        objects.insert(backpack.id.clone(), backpack.clone());

        let ctx = DisplayContext::new(owner, DisplayMode::Player)
            .with_objects(objects)
            .with_flags(DisplayFlags::BRIEF);
        let look_out = backpack.describe(&ctx);
        assert_eq!(look_out, "The backpack is empty.");
    }

    #[test]
    fn look_object_has_no_leading_name_line() {
        let owner = ObjectId::new("player:admin-001");
        let mut backpack = Object {
            id: ObjectId::new("item:backpack-001"),
            name: "backpack".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        backpack.apply_container_role(&crate::object::ContainerSpec {
            capacity: 20,
            max_weight: Some(100),
            max_volume: None,
            wearable: true,
            wear_slot: Some("torso".to_string()),
            ..crate::object::ContainerSpec::default()
        });

        let mut coins = Object {
            id: ObjectId::new("item:coins-001"),
            name: "coins".to_string(),
            aliases: Vec::new(),
            location: Some(backpack.id.clone()),
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        coins.set_property_int("weight", 1);
        coins.apply_stackable_role(&crate::object::StackableSpec {
            count: 20,
            max_stack: 99,
        });
        backpack.set_property_list("contents", vec![coins.id.clone()]);

        let mut objects = HashMap::new();
        objects.insert(coins.id.clone(), coins);
        objects.insert(backpack.id.clone(), backpack.clone());

        let look_ctx = DisplayContext::new(owner.clone(), DisplayMode::Player)
            .with_objects(objects.clone())
            .with_flags(DisplayFlags::BRIEF);
        let look_out = backpack.describe(&look_ctx);
        assert_eq!(look_out, "The backpack contains 20 coins.");
        assert!(!look_out.starts_with("backpack"));

        let examine_ctx = DisplayContext::new(owner, DisplayMode::Player).with_objects(objects);
        let examine_out = backpack.describe(&examine_ctx);
        assert_eq!(
            examine_out,
            "The backpack contains 20 coins and has a capacity of 1/20. It is carrying 20/100 weight."
        );
    }

    #[test]
    fn look_brief_hides_weight_on_objects() {
        let owner = ObjectId::new("player:admin-001");
        let mut purse = Object {
            id: ObjectId::new("item:purse-001"),
            name: "purse".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        purse.apply_container_role(&crate::object::ContainerSpec {
            capacity: 3,
            max_weight: Some(10),
            max_volume: None,
            wearable: false,
            wear_slot: None,
            ..crate::object::ContainerSpec::default()
        });

        let mut coins = Object {
            id: ObjectId::new("item:coins-001"),
            name: "coins".to_string(),
            aliases: Vec::new(),
            location: Some(purse.id.clone()),
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        coins.set_property_int("weight", 1);
        coins.apply_stackable_role(&crate::object::StackableSpec {
            count: 2,
            max_stack: 99,
        });
        purse.set_property_list("contents", vec![coins.id.clone()]);

        let mut objects = HashMap::new();
        objects.insert(coins.id.clone(), coins);
        objects.insert(purse.id.clone(), purse.clone());

        let ctx = DisplayContext::new(owner.clone(), DisplayMode::Player)
            .with_objects(objects.clone())
            .with_flags(DisplayFlags::BRIEF);
        let look_out = purse.describe(&ctx);
        assert!(!look_out.contains("weighs"));
        assert_eq!(look_out, "The purse contains 2 coins.");

        let examine_ctx = DisplayContext::new(owner, DisplayMode::Player).with_objects(objects);
        let examine_out = purse.describe(&examine_ctx);
        assert_eq!(
            examine_out,
            "The purse contains 2 coins and has a capacity of 1/3. It is carrying 2/10 weight."
        );
    }

    #[test]
    fn in_game_examine_shows_container_weight() {
        let owner = ObjectId::new("player:admin-001");
        let mut purse = Object {
            id: ObjectId::new("item:purse-001"),
            name: "purse".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        purse.apply_container_role(&crate::object::ContainerSpec {
            capacity: 3,
            max_weight: Some(10),
            max_volume: None,
            wearable: false,
            wear_slot: None,
            ..crate::object::ContainerSpec::default()
        });

        let mut coins = Object {
            id: ObjectId::new("item:coins-001"),
            name: "coins".to_string(),
            aliases: Vec::new(),
            location: Some(purse.id.clone()),
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        coins.set_property_int("weight", 1);
        coins.apply_stackable_role(&crate::object::StackableSpec {
            count: 2,
            max_stack: 99,
        });
        purse.set_property_list("contents", vec![coins.id.clone()]);

        let mut objects = HashMap::new();
        objects.insert(coins.id.clone(), coins);
        objects.insert(purse.id.clone(), purse.clone());

        let ctx = DisplayContext::new(owner, DisplayMode::Player).with_objects(objects);
        let output = purse.describe(&ctx);
        assert_eq!(
            output,
            "The purse contains 2 coins and has a capacity of 1/3. It is carrying 2/10 weight."
        );
    }

    #[test]
    fn meta_examine_shows_weight_details() {
        let owner = ObjectId::new("player:admin-001");
        let mut purse = Object {
            id: ObjectId::new("item:purse-001"),
            name: "purse".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        purse.set_property_int("weight", 1);
        purse.apply_container_role(&crate::object::ContainerSpec {
            capacity: 3,
            max_weight: Some(10),
            max_volume: None,
            wearable: false,
            wear_slot: None,
            ..crate::object::ContainerSpec::default()
        });

        let mut objects = HashMap::new();
        objects.insert(purse.id.clone(), purse.clone());

        let ctx = DisplayContext::new(owner, DisplayMode::Builder).with_objects(objects);
        let output = purse.describe_detailed(&ctx);
        assert!(output.contains("status:"));
        assert!(output.contains("contents_weight: 0/10"));
        assert!(output.contains("weight: 1"));
    }

    #[test]
    fn examine_self_shows_concise_equipment_summary() {
        let anatomy = load_module("modules/default")
            .unwrap()
            .active_world()
            .unwrap()
            .anatomy
            .clone();
        let owner = ObjectId::new("player:admin-001");
        let mut player = Object {
            id: owner.clone(),
            name: "Admin".to_string(),
            aliases: Vec::new(),
            location: Some(ObjectId::new("room:void-001")),
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        player.init_creature_role(anatomy.player_template("default").unwrap());
        player.set_property_int("max_weight", 100);

        let ctx = DisplayContext::new(owner.clone(), DisplayMode::Player).with_anatomy(anatomy);
        let output = player.describe(&ctx);

        assert!(output.starts_with("You're a human."));
        assert!(output.contains("carry capacity of 0/10"));
        assert!(output.contains("are carrying 0 of 100 weight."));
        assert!(!output.contains("Admin"));
        assert!(!output.contains("Equipped:"));
        assert!(!output.contains("completely naked"));
        assert!(!output.contains("Available slots"));
    }

    #[test]
    fn meta_examine_shows_builder_info() {
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
            revision: 0,
            updated_at: None,
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

        assert!(output.contains("name: Admin"));
        assert!(output.contains("type: player"));
        assert!(output.contains("id: admin-001"));
        assert!(output.contains("state:"));
        assert!(output.contains("owner: you"));
        assert!(output.contains("wave"));
        assert!(!output.contains("player:admin-001"));
    }
}
