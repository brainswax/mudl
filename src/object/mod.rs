mod factory;
mod location;
mod roles;
mod weight;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use bitflags::bitflags;

use crate::display::{Describable, DisplayContext, DisplayFlags, DisplayMode};
use crate::inventory::describe_carried;

pub use factory::ObjectFactory;
pub use location::LocationRef;
pub use roles::{ContainerSpec, ItemPhysSpec, ObjectRoles, RoleKind, StackableSpec, WearableSpec};
pub use weight::{
    is_unlimited_weight, player_carried_weight, weight_limit_applies, DEFAULT_PLAYER_MAX_WEIGHT,
    UNLIMITED_WEIGHT,
};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[derive(serde::Serialize, serde::Deserialize)]
    pub struct PermissionFlags: u8 {
        const OWNER    = 1 << 0;
        const BUILDER  = 1 << 1;
        const WIZARD   = 1 << 2;
        const EVERYONE = 1 << 3;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectId(String);

impl ObjectId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ObjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Behavior {
    pub code: String,
    pub permissions: PermissionFlags,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Property {
    pub name: String,
    pub value: Value,
    pub permissions: PermissionFlags,
    pub behavior: Option<Behavior>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Verb {
    pub name: String,
    pub code: String,
    pub permissions: PermissionFlags,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Value {
    String(String),
    Int(i64),
    Bool(bool),
    List(Vec<Value>),
    ObjectRef(ObjectId),
    Map(HashMap<String, Value>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Object {
    pub id: ObjectId,
    pub name: String,
    pub aliases: Vec<String>,
    pub location: Option<ObjectId>,
    pub prototype: Option<ObjectId>,
    pub owner: ObjectId,
    pub permissions: PermissionFlags,
    pub properties: HashMap<String, Property>,
    pub verbs: HashMap<String, Verb>,
    pub event_handlers: HashMap<String, Vec<Behavior>>,
    /// Soft-delete flag — object remains in the database but is hidden from normal play.
    #[serde(default)]
    pub is_deleted: bool,
    /// UTC epoch seconds when the object was soft-deleted, if applicable.
    #[serde(default)]
    pub deleted_at: Option<String>,
}

/// Maximum length of the name segment in generated object IDs.
pub const ID_BASE_MAX_LEN: usize = 16;

/// Convert a display name into a lowercase hyphenated slug for object IDs.
pub fn slugify_display_name(name: &str) -> String {
    let mut slug = String::new();
    let mut last_was_sep = true;

    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            slug.push('-');
            last_was_sep = true;
        }
    }

    let slug = slug.trim_end_matches('-').to_string();
    if slug.is_empty() {
        "object".to_string()
    } else {
        slug
    }
}

/// Truncate a slug to [`ID_BASE_MAX_LEN`] on a char boundary.
pub fn constrain_id_base(slug: &str) -> String {
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        return "object".to_string();
    }
    if slug.len() <= ID_BASE_MAX_LEN {
        return slug.to_string();
    }
    let mut end = ID_BASE_MAX_LEN;
    while end > 0 && !slug.is_char_boundary(end) {
        end -= 1;
    }
    let trimmed = slug[..end].trim_end_matches('-');
    if trimmed.is_empty() {
        "object".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Slugify a display name and constrain it for use in object IDs.
pub fn id_base_from_display_name(name: &str) -> String {
    constrain_id_base(&slugify_display_name(name))
}

pub fn generate_object_id(obj_type: &str, base_name: &str, counter: u32) -> ObjectId {
    let ty = obj_type.to_ascii_lowercase();
    let base = constrain_id_base(&base_name.to_ascii_lowercase());
    ObjectId(format!("{ty}:{base}-{counter:03x}"))
}

impl Object {
    /// Returns a direct property if present.
    pub fn get_property(&self, name: &str) -> Option<&Property> {
        self.properties.get(name)
    }

    /// Basic recursive inheritance lookup using a provided lookup function.
    /// In a full implementation, this would be handled by the WorldState.
    pub fn resolve_inherited_property(
        &self,
        name: &str,
        get_prototype: impl Fn(&ObjectId) -> Option<Object>,
    ) -> Option<Property> {
        if let Some(prop) = self.properties.get(name) {
            return Some(prop.clone());
        }
        if let Some(proto_id) = &self.prototype {
            if let Some(proto) = get_prototype(proto_id) {
                return proto.resolve_inherited_property(name, get_prototype);
            }
        }
        None
    }

    pub fn add_property(&mut self, property: Property) {
        self.properties.insert(property.name.clone(), property);
    }

    pub fn add_verb(&mut self, verb: Verb) {
        self.verbs.insert(verb.name.clone(), verb);
    }

    pub fn add_event_handler(&mut self, event_name: String, behavior: Behavior) {
        self.event_handlers
            .entry(event_name)
            .or_default()
            .push(behavior);
    }

    pub fn get_description(&self) -> Option<String> {
        self.get_property("description").and_then(|prop| {
            if let Value::String(s) = &prop.value {
                Some(s.clone())
            } else {
                None
            }
        })
    }

    pub fn get_exits(&self) -> HashMap<String, ObjectId> {
        if let Some(prop) = self.get_property("exits") {
            if let Value::Map(map) = &prop.value {
                let mut result = HashMap::new();
                for (key, val) in map {
                    if let Value::ObjectRef(id) = val {
                        result.insert(key.clone(), id.clone());
                    }
                }
                return result;
            }
        }
        HashMap::new()
    }

    pub fn add_exit(&mut self, direction: &str, target: ObjectId) {
        let exits_prop = self.properties.get_mut("exits");
        if let Some(prop) = exits_prop {
            if let Value::Map(map) = &mut prop.value {
                map.insert(direction.to_string(), Value::ObjectRef(target));
                return;
            }
        }
        let mut map = HashMap::new();
        map.insert(direction.to_string(), Value::ObjectRef(target));
        let prop = Property {
            name: "exits".to_string(),
            value: Value::Map(map),
            permissions: PermissionFlags::OWNER,
            behavior: None,
        };
        self.properties.insert("exits".to_string(), prop);
    }

    /// Object category derived from the ID prefix (e.g. `room`, `player`, `item`).
    pub fn object_type(&self) -> &str {
        self.id.as_str().split(':').next().unwrap_or("unknown")
    }

    /// Whether this object is a navigable place (room, area, location, etc.).
    pub fn is_location(&self) -> bool {
        matches!(
            self.object_type(),
            "room" | "area" | "location" | "region" | "zone"
        )
    }

    /// Whether this object is visible in normal play (not soft-deleted).
    pub fn is_active(&self) -> bool {
        !self.is_deleted
    }

    /// Mark this object as soft-deleted (retained in persistence).
    pub fn soft_delete(&mut self) {
        self.is_deleted = true;
        self.deleted_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs().to_string())
                .unwrap_or_else(|_| "0".to_string()),
        );
    }

    /// Restore a soft-deleted object.
    pub fn undelete(&mut self) {
        self.is_deleted = false;
        self.deleted_at = None;
    }

    /// Objects located inside this object (by `location` field), excluding soft-deleted.
    pub fn contents<'a>(&self, objects: &'a HashMap<ObjectId, Object>) -> Vec<&'a Object> {
        objects
            .values()
            .filter(|obj| obj.is_active() && obj.location.as_ref() == Some(&self.id))
            .collect()
    }

    pub fn creature_name(&self) -> Option<String> {
        self.get_property("creature")
            .or_else(|| self.get_property("body_plan"))
            .and_then(|p| {
                if let Value::String(s) = &p.value {
                    Some(s.clone())
                } else {
                    None
                }
            })
    }

    /// Alias for [`creature_name`](Self::creature_name).
    pub fn body_plan_name(&self) -> Option<String> {
        self.creature_name()
    }

    pub fn gender(&self) -> Option<String> {
        self.get_property("gender").and_then(|p| {
            if let Value::String(s) = &p.value {
                Some(s.clone())
            } else {
                None
            }
        })
    }

    pub fn body_slots(&self) -> HashMap<String, ObjectId> {
        self.get_object_map_property("body_slots")
    }

    pub fn body_slot_item(&self, slot: &str) -> Option<ObjectId> {
        self.body_slots().get(slot).cloned()
    }

    pub fn set_body_slot(&mut self, slot: &str, item: Option<ObjectId>) {
        let mut slots = self.body_slots();
        if let Some(id) = item {
            slots.insert(slot.to_string(), id);
        } else {
            slots.remove(slot);
        }
        self.set_property_map("body_slots", slots);
    }

    pub fn clear_item_from_body_slots(&mut self, item_id: &ObjectId) {
        let slots: HashMap<String, ObjectId> = self
            .body_slots()
            .into_iter()
            .filter(|(_, id)| id != item_id)
            .collect();
        self.set_property_map("body_slots", slots);
    }

    pub fn set_property_map(&mut self, name: &str, map: HashMap<String, ObjectId>) {
        let value_map: HashMap<String, Value> = map
            .into_iter()
            .map(|(k, v)| (k, Value::ObjectRef(v)))
            .collect();
        self.add_property(Property {
            name: name.to_string(),
            value: Value::Map(value_map),
            permissions: PermissionFlags::OWNER,
            behavior: None,
        });
    }

    pub fn get_object_map_property(&self, name: &str) -> HashMap<String, ObjectId> {
        self.get_property(name)
            .and_then(|p| {
                if let Value::Map(map) = &p.value {
                    Some(
                        map.iter()
                            .filter_map(|(k, v)| {
                                if let Value::ObjectRef(id) = v {
                                    Some((k.clone(), id.clone()))
                                } else {
                                    None
                                }
                            })
                            .collect(),
                    )
                } else {
                    None
                }
            })
            .unwrap_or_default()
    }

    pub fn carried_body_items(&self) -> Vec<ObjectId> {
        let mut seen = Vec::new();
        for id in self.body_slots().values() {
            if !seen.contains(id) {
                seen.push(id.clone());
            }
        }
        seen
    }
}

fn format_exits_player(exits: &HashMap<String, ObjectId>) -> String {
    if exits.is_empty() {
        return String::new();
    }
    let mut dirs: Vec<&str> = exits.keys().map(String::as_str).collect();
    dirs.sort_unstable();
    format!("Obvious exits: {}", dirs.join(", "))
}

fn format_contents_player(obj: &Object, ctx: &DisplayContext) -> String {
    let contents: Vec<String> = obj
        .contents(&ctx.objects)
        .into_iter()
        .filter(|item| item.id != ctx.observer)
        .map(|item| match item.object_type() {
            "player" => item.name.clone(),
            "item" | "thing" => {
                let label = crate::display::format_stackable_label(item);
                if let Some(desc) = item.get_description() {
                    format!("{label} — {desc}")
                } else {
                    label
                }
            }
            _ => item.name.clone(),
        })
        .collect();

    if contents.is_empty() {
        String::new()
    } else {
        format!("You see: {}", contents.join("; "))
    }
}

fn describe_room_player(obj: &Object, ctx: &DisplayContext) -> String {
    let mut lines = vec![obj.name.clone()];

    if ctx.flags.contains(DisplayFlags::DARK) {
        lines.push("It is pitch black.".to_string());
    } else if let Some(desc) = obj.get_description() {
        lines.push(desc);
    }

    let exits = format_exits_player(&obj.get_exits());
    if !exits.is_empty() {
        lines.push(exits);
    }

    let contents = format_contents_player(obj, ctx);
    if !contents.is_empty() {
        lines.push(contents);
    }

    lines.join("\n")
}

fn describe_entity_player(obj: &Object, ctx: &DisplayContext) -> String {
    let brief = ctx.flags.contains(DisplayFlags::BRIEF);
    let mut lines = vec![crate::display::format_stackable_label(obj)];
    if let Some(desc) = obj.get_description() {
        lines.push(desc);
    }
    if !brief {
        if let Some(weight) = crate::display::format_weight_examine_player(obj, &ctx.objects) {
            lines.push(weight);
        }
    }
    if obj.is_container() {
        let inside = crate::display::format_inside_container(obj, &ctx.objects);
        if !inside.is_empty() {
            lines.push(inside);
        }
    }
    if obj.object_type() == "player" && obj.id == ctx.observer {
        let carried = if brief {
            crate::display::format_look_self_summary(obj, &ctx.objects, &ctx.anatomy)
        } else {
            describe_carried(obj, &ctx.objects, &ctx.anatomy)
        };
        lines.push(carried);
    }
    lines.join("\n")
}

fn describe_room_builder(obj: &Object, ctx: &DisplayContext) -> String {
    crate::display::format_builder_examine_room(obj, ctx)
}

fn describe_entity_builder(obj: &Object, ctx: &DisplayContext) -> String {
    crate::display::format_builder_examine_entity(obj, ctx)
}

impl Describable for Object {
    fn describe(&self, ctx: &DisplayContext) -> String {
        match ctx.mode {
            DisplayMode::Debug => self.dump(),
            DisplayMode::Builder => self.describe_detailed(ctx),
            DisplayMode::Player => {
                if self.is_location() {
                    describe_room_player(self, ctx)
                } else {
                    describe_entity_player(self, ctx)
                }
            }
        }
    }

    fn describe_detailed(&self, ctx: &DisplayContext) -> String {
        if self.is_location() {
            describe_room_builder(self, ctx)
        } else {
            describe_entity_builder(self, ctx)
        }
    }

    fn dump(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| format!("{self:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_display_name_lowercase_hyphens() {
        assert_eq!(slugify_display_name("Rusty Sword"), "rusty-sword");
        assert_eq!(slugify_display_name("Big_Red Boots!"), "big-red-boots");
    }

    #[test]
    fn generate_object_id_is_always_lowercase() {
        let id = generate_object_id("Sword", "Rusty-Sword", 1);
        assert_eq!(id.as_str(), "sword:rusty-sword-001");
    }

    #[test]
    fn constrain_id_base_truncates_long_names() {
        let long = slugify_display_name("extraordinarily-long-container-name");
        assert!(long.len() > ID_BASE_MAX_LEN);
        let base = constrain_id_base(&long);
        assert!(base.len() <= ID_BASE_MAX_LEN);
        assert_eq!(base, "extraordinarily");
    }

    #[test]
    fn id_base_from_display_name_is_bounded() {
        let base = id_base_from_display_name("Purse");
        assert_eq!(base, "purse");
        assert!(base.len() <= ID_BASE_MAX_LEN);
    }

    #[test]
    fn direct_look_stackable_shows_quantity() {
        let owner = ObjectId::new("player:admin-001");
        let mut coins = Object {
            id: ObjectId::new("item:coins-001"),
            name: "coins".to_string(),
            aliases: Vec::new(),
            location: Some(owner.clone()),
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        coins.apply_stackable_role(&StackableSpec {
            count: 20,
            max_stack: 99,
        });
        coins.add_property(Property {
            name: "description".to_string(),
            value: Value::String("Shiny gold coins.".to_string()),
            permissions: PermissionFlags::EVERYONE,
            behavior: None,
        });

        let mut objects = HashMap::new();
        objects.insert(coins.id.clone(), coins.clone());

        let ctx = DisplayContext::new(owner, DisplayMode::Player).with_objects(objects);
        let output = coins.describe(&ctx);
        assert!(output.starts_with("20 coins"));
        assert!(output.contains("Shiny gold coins."));
    }

    #[test]
    fn room_look_shows_stackable_quantity() {
        let owner = ObjectId::new("player:admin-001");
        let room_id = ObjectId::new("room:void-001");
        let mut room = Object {
            id: room_id.clone(),
            name: "The Void".to_string(),
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
            value: Value::String("Empty.".to_string()),
            permissions: PermissionFlags::EVERYONE,
            behavior: None,
        });

        let mut coins = Object {
            id: ObjectId::new("item:coins-001"),
            name: "coins".to_string(),
            aliases: Vec::new(),
            location: Some(room_id.clone()),
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        coins.apply_stackable_role(&StackableSpec {
            count: 20,
            max_stack: 99,
        });

        let mut objects = HashMap::new();
        objects.insert(room.id.clone(), room.clone());
        objects.insert(coins.id.clone(), coins);

        let ctx = DisplayContext::new(owner, DisplayMode::Player).with_objects(objects);
        let output = room.describe(&ctx);
        assert!(output.contains("You see: 20 coins"));
    }
}
