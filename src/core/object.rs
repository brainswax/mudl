use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use bitflags::bitflags;

use super::display::{Describable, DisplayContext, DisplayFlags, DisplayMode};
use super::persistence::Persistence;

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

struct WorldDef {
    obj_type: String,
    base_name: String,
    name: String,
    description: Option<String>,
    exits: HashMap<String, String>,
    location: Option<String>,
    starting_location: Option<String>,
}

fn parse_mudl_file(path: &std::path::Path) -> anyhow::Result<Vec<WorldDef>> {
    let content = std::fs::read_to_string(path)?;
    let mut defs: Vec<WorldDef> = Vec::new();
    let mut current = WorldDef {
        obj_type: "room".to_string(),
        base_name: "unknown".to_string(),
        name: "Unknown".to_string(),
        description: None,
        exits: HashMap::new(),
        location: None,
        starting_location: None,
    };
    let mut in_exits = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if current.base_name != "unknown" {
                defs.push(current);
                current = WorldDef {
                    obj_type: "room".to_string(),
                    base_name: "unknown".to_string(),
                    name: "Unknown".to_string(),
                    description: None,
                    exits: HashMap::new(),
                    location: None,
                    starting_location: None,
                };
                in_exits = false;
            }
            continue;
        }
        if trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }
        if trimmed == "exits:" {
            in_exits = true;
            continue;
        }
        if in_exits && trimmed.contains(':') {
            let parts: Vec<&str> = trimmed.splitn(2, ':').collect();
            if parts.len() == 2 {
                let dir = parts[0].trim().to_string();
                let target = parts[1].trim().to_string();
                current.exits.insert(dir, target);
            }
            continue;
        }
        if trimmed.contains(':') {
            in_exits = false;
            let parts: Vec<&str> = trimmed.splitn(2, ':').collect();
            if parts.len() == 2 {
                let key = parts[0].trim().to_lowercase();
                let value = parts[1].trim().to_string();
                match key.as_str() {
                    "type" => current.obj_type = value,
                    "base_name" => current.base_name = value,
                    "name" => current.name = value,
                    "description" => current.description = Some(value),
                    "location" => current.location = Some(value),
                    "starting_location" => current.starting_location = Some(value),
                    _ => {}
                }
            }
        }
    }
    if current.base_name != "unknown" {
        defs.push(current);
    }
    Ok(defs)
}

pub struct ObjectFactory<P: Persistence> {
    persistence: P,
}

impl<P: Persistence> ObjectFactory<P> {
    pub fn new(persistence: P) -> Self {
        Self { persistence }
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

    pub async fn load_world(&self, dir: &str, owner: ObjectId) -> anyhow::Result<ObjectId> {
        let world_id = ObjectId::new("world:default-001");
        let is_loaded = self.load_object(&world_id).await?.is_some();

        let mut world_defs: Vec<WorldDef> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("mudl") {
                    if let Ok(defs) = parse_mudl_file(&path) {
                        world_defs.extend(defs);
                    }
                }
            }
        }

        if world_defs.is_empty() {
            return Err(anyhow::anyhow!("No .mudl files found in {}", dir));
        }

        let mut starting_location: Option<String> = None;
        for def in &world_defs {
            if def.obj_type == "config" {
                starting_location = def.starting_location.clone();
                continue;
            }
        }

        if is_loaded {
            if let Some(start_base) = &starting_location {
                let start_id = ObjectId::new(format!("room:{}-001", start_base));
                return Ok(start_id);
            }
            return Err(anyhow::anyhow!(
                "No starting location specified in init.mudl"
            ));
        }

        let mut name_to_id: HashMap<String, ObjectId> = HashMap::new();

        for def in &world_defs {
            if def.obj_type == "config" {
                continue;
            }
            let mut obj = self
                .create(&def.obj_type, &def.base_name, owner.clone())
                .await?;
            obj.name = def.name.clone();
            if let Some(desc) = &def.description {
                let desc_prop = Property {
                    name: "description".to_string(),
                    value: Value::String(desc.clone()),
                    permissions: PermissionFlags::EVERYONE,
                    behavior: None,
                };
                obj.add_property(desc_prop);
            }
            self.persistence.save_object(&obj).await?;
            name_to_id.insert(def.base_name.clone(), obj.id.clone());
        }

        for def in &world_defs {
            if def.obj_type == "config" {
                continue;
            }
            if let Some(id) = name_to_id.get(&def.base_name) {
                let mut obj = if let Some(o) = self.load_object(id).await? {
                    o
                } else {
                    continue;
                };
                if let Some(loc_base) = &def.location {
                    if let Some(loc_id) = name_to_id.get(loc_base) {
                        obj.location = Some(loc_id.clone());
                    }
                }
                for (dir, target_base) in &def.exits {
                    if let Some(target_id) = name_to_id.get(target_base) {
                        obj.add_exit(dir, target_id.clone());
                    }
                }
                self.persistence.save_object(&obj).await?;
            }
        }

        if self.load_object(&owner).await?.is_none() {
            let mut player = self.create("player", "admin", owner.clone()).await?;
            player.name = "Admin".to_string();
            if let Some(start_base) = &starting_location {
                if let Some(start_id) = name_to_id.get(start_base) {
                    player.location = Some(start_id.clone());
                }
            }
            self.persistence.save_object(&player).await?;
        }

        let start_id = if let Some(start_base) = &starting_location {
            name_to_id
                .get(start_base)
                .cloned()
                .unwrap_or_else(|| ObjectId::new(format!("room:{}-001", start_base)))
        } else {
            name_to_id
                .values()
                .next()
                .cloned()
                .unwrap_or_else(|| ObjectId::new("room:the-void-001"))
        };
        Ok(start_id)
    }

    pub async fn bootstrap(&self, owner: ObjectId) -> anyhow::Result<ObjectId> {
        self.load_world("worlds/default", owner).await
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

    /// Objects located inside this object (by `location` field).
    pub fn contents<'a>(&self, objects: &'a HashMap<ObjectId, Object>) -> Vec<&'a Object> {
        objects
            .values()
            .filter(|obj| obj.location.as_ref() == Some(&self.id))
            .collect()
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

fn describe_entity_player(obj: &Object) -> String {
    let mut lines = vec![obj.name.clone()];
    if let Some(desc) = obj.get_description() {
        lines.push(desc);
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
            DisplayMode::Player => match self.object_type() {
                "room" => describe_room_player(self, ctx),
                _ => describe_entity_player(self),
            },
        }
    }

    fn describe_detailed(&self, ctx: &DisplayContext) -> String {
        match self.object_type() {
            "room" => describe_room_builder(self, ctx),
            _ => describe_entity_builder(self),
        }
    }

    fn dump(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| format!("{self:?}"))
    }
}
