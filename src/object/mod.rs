use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use bitflags::bitflags;

use crate::display::{Describable, DisplayContext, DisplayFlags, DisplayMode};
use crate::inventory::describe_carried;
use crate::mudl::{AnatomyRegistry, PlayerTemplate};
use crate::persistence::Persistence;

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
}

pub fn generate_object_id(obj_type: &str, base_name: &str, counter: u32) -> ObjectId {
    ObjectId(format!("{}:{}-{:03x}", obj_type, base_name, counter))
}

pub struct ObjectFactory<P: Persistence> {
    persistence: P,
}

impl<P: Persistence> ObjectFactory<P> {
    pub fn new(persistence: P) -> Self {
        Self { persistence }
    }

    pub fn persistence(&self) -> &P {
        &self.persistence
    }

    pub async fn create_player(
        &self,
        base_name: &str,
        owner: ObjectId,
        anatomy: &AnatomyRegistry,
    ) -> anyhow::Result<Object> {
        let template = anatomy
            .default_template()
            .cloned()
            .unwrap_or(PlayerTemplate {
                name: "default".to_string(),
                creature: "human".to_string(),
                gender: "neutral".to_string(),
            });
        let mut player = self.create("player", base_name, owner).await?;
        player.name = base_name.to_string();
        player.init_body(&template);
        self.persistence.save_object(&player).await?;
        Ok(player)
    }

    pub async fn create_item(&self, base_name: &str, owner: ObjectId) -> anyhow::Result<Object> {
        let mut item = self.create("item", base_name, owner).await?;
        item.init_item_defaults(true);
        self.persistence.save_object(&item).await?;
        Ok(item)
    }

    pub async fn create_container(
        &self,
        base_name: &str,
        owner: ObjectId,
        capacity: u32,
        wearable: bool,
    ) -> anyhow::Result<Object> {
        let mut container = self.create("item", base_name, owner).await?;
        container.init_container_defaults(capacity, wearable);
        self.persistence.save_object(&container).await?;
        Ok(container)
    }

    pub async fn create(
        &self,
        type_name: &str,
        base_name: &str,
        owner: ObjectId,
    ) -> anyhow::Result<Object> {
        let counter = self
            .persistence
            .get_next_id_counter(type_name, base_name)
            .await?;
        let id = generate_object_id(type_name, base_name, counter);
        self.persistence
            .increment_counter(type_name, base_name)
            .await?;

        let name = base_name.to_string();
        let object = Object {
            id,
            name,
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner,
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
        };

        self.persistence.save_object(&object).await?;
        Ok(object)
    }

    pub async fn load_object(&self, id: &ObjectId) -> anyhow::Result<Option<Object>> {
        self.persistence.load_object(id).await
    }
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

    /// Objects located inside this object (by `location` field).
    pub fn contents<'a>(&self, objects: &'a HashMap<ObjectId, Object>) -> Vec<&'a Object> {
        objects
            .values()
            .filter(|obj| obj.location.as_ref() == Some(&self.id))
            .collect()
    }

    /// Initialize a naked player from a MUDL player template and creature definition.
    pub fn init_body(&mut self, template: &PlayerTemplate) {
        self.add_property(Property {
            name: "creature".to_string(),
            value: Value::String(template.creature.clone()),
            permissions: PermissionFlags::OWNER,
            behavior: None,
        });
        self.add_property(Property {
            name: "gender".to_string(),
            value: Value::String(template.gender.clone()),
            permissions: PermissionFlags::OWNER,
            behavior: None,
        });
        self.set_property_map("body_slots", HashMap::new());
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

fn format_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::ObjectRef(id) => id.to_string(),
        Value::List(items) => format!(
            "[{}]",
            items
                .iter()
                .map(format_value)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Map(map) => {
            let pairs: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", k, format_value(v)))
                .collect();
            format!("{{{}}}", pairs.join(", "))
        }
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
                if let Some(desc) = item.get_description() {
                    format!("{} — {}", item.name, desc)
                } else {
                    item.name.clone()
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

fn format_properties_builder(obj: &Object) -> String {
    if obj.properties.is_empty() {
        return "  (none)".to_string();
    }
    let mut names: Vec<&str> = obj.properties.keys().map(String::as_str).collect();
    names.sort_unstable();
    names
        .into_iter()
        .map(|name| {
            let prop = &obj.properties[name];
            format!("  {} = {}", name, format_value(&prop.value))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_verbs_builder(obj: &Object) -> String {
    if obj.verbs.is_empty() {
        return "  (none)".to_string();
    }
    let mut names: Vec<&str> = obj.verbs.keys().map(String::as_str).collect();
    names.sort_unstable();
    names
        .into_iter()
        .map(|name| {
            let verb = &obj.verbs[name];
            format!("  {}: {}", name, verb.code)
        })
        .collect::<Vec<_>>()
        .join("\n")
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
    let mut lines = vec![obj.name.clone()];
    if let Some(desc) = obj.get_description() {
        lines.push(desc);
    }
    if obj.object_type() == "player" && obj.id == ctx.observer {
        lines.push(describe_carried(obj, &ctx.objects, &ctx.anatomy));
    }
    lines.join("\n")
}

fn describe_room_builder(obj: &Object, ctx: &DisplayContext) -> String {
    let mut lines = vec![
        format!("{} [{}]", obj.name, obj.id),
        format!("Owner: {}", obj.owner),
    ];

    if let Some(desc) = obj.get_description() {
        lines.push(format!("Description: {}", desc));
    }

    let exits = obj.get_exits();
    if !exits.is_empty() {
        let mut dirs: Vec<&str> = exits.keys().map(String::as_str).collect();
        dirs.sort_unstable();
        let exit_list: Vec<String> = dirs
            .into_iter()
            .map(|dir| format!("{} -> {}", dir, exits[dir]))
            .collect();
        lines.push(format!("Exits: {}", exit_list.join(", ")));
    }

    let contents: Vec<String> = obj
        .contents(&ctx.objects)
        .into_iter()
        .map(|item| format!("{} [{}]", item.name, item.id))
        .collect();
    if !contents.is_empty() {
        lines.push(format!("Contents: {}", contents.join(", ")));
    }

    lines.push("Properties:".to_string());
    lines.push(format_properties_builder(obj));
    lines.push("Verbs:".to_string());
    lines.push(format_verbs_builder(obj));

    lines.join("\n")
}

fn describe_entity_builder(obj: &Object) -> String {
    let mut lines = vec![
        format!("{} [{}]", obj.name, obj.id),
        format!("Owner: {}", obj.owner),
    ];

    if let Some(loc) = &obj.location {
        lines.push(format!("Location: {}", loc));
    }
    if let Some(desc) = obj.get_description() {
        lines.push(format!("Description: {}", desc));
    }

    lines.push("Properties:".to_string());
    lines.push(format_properties_builder(obj));
    lines.push("Verbs:".to_string());
    lines.push(format_verbs_builder(obj));

    lines.join("\n")
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
            describe_entity_builder(self)
        }
    }

    fn dump(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| format!("{self:?}"))
    }
}
