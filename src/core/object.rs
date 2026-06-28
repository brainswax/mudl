use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use bitflags::bitflags;

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
        let counter = self.persistence.get_next_id_counter(type_name, base_name).await?;
        let id = generate_object_id(type_name, base_name, counter);
        self.persistence.increment_counter(type_name, base_name).await?;

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
}
