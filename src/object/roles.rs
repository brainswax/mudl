//! Composable object roles stored as properties (composition over inheritance).

use std::collections::HashMap;

use crate::mudl::PlayerTemplate;
use crate::object::{Object, ObjectId, PermissionFlags, Property, Value};

/// Role kinds attachable to any object via properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RoleKind {
    Location,
    Container,
    Wearable,
    Creature,
    Stackable,
}

/// Summary of which roles an object currently has.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObjectRoles {
    pub location: bool,
    pub container: bool,
    pub wearable: bool,
    pub creature: bool,
    pub stackable: bool,
}

/// Configuration for a container role.
#[derive(Debug, Clone)]
pub struct ContainerSpec {
    pub capacity: u32,
    pub max_weight: Option<i64>,
    pub max_volume: Option<i64>,
    pub wearable: bool,
    pub wear_slot: Option<String>,
}

impl Default for ContainerSpec {
    fn default() -> Self {
        Self {
            capacity: 10,
            max_weight: None,
            max_volume: None,
            wearable: false,
            wear_slot: None,
        }
    }
}

/// Configuration for a wearable role.
#[derive(Debug, Clone)]
pub struct WearableSpec {
    pub wear_slot: String,
    pub weight: f64,
    pub volume: f64,
}

/// Physical attributes for a generic item.
#[derive(Debug, Clone)]
pub struct ItemPhysSpec {
    pub weight: f64,
    pub volume: f64,
    pub pocketable: bool,
}

impl Default for ItemPhysSpec {
    fn default() -> Self {
        Self {
            weight: 1.0,
            volume: 1.0,
            pocketable: true,
        }
    }
}

/// Configuration for stackable identical items.
#[derive(Debug, Clone)]
pub struct StackableSpec {
    pub count: u32,
    pub max_stack: u32,
}

impl Default for StackableSpec {
    fn default() -> Self {
        Self {
            count: 1,
            max_stack: 99,
        }
    }
}

impl Object {
    /// Inspect which composable roles are active on this object.
    pub fn roles(&self) -> ObjectRoles {
        ObjectRoles {
            location: self.is_location(),
            container: self.has_container_role(),
            wearable: self.has_wearable_role(),
            creature: self.has_creature_role(),
            stackable: self.is_stackable(),
        }
    }

    pub fn has_container_role(&self) -> bool {
        self.get_bool_property("is_container").unwrap_or(false)
    }

    pub fn has_wearable_role(&self) -> bool {
        self.get_bool_property("is_wearable").unwrap_or(false)
    }

    pub fn has_creature_role(&self) -> bool {
        self.object_type() == "player" || self.get_property("creature").is_some()
    }

    pub fn is_stackable(&self) -> bool {
        self.get_bool_property("stackable").unwrap_or(false)
    }

    pub fn weight(&self) -> f64 {
        let unit = self.unit_weight();
        if self.is_stackable() {
            unit * f64::from(self.stack_count())
        } else {
            unit
        }
    }

    pub fn unit_weight(&self) -> f64 {
        self.get_numeric_property("weight").unwrap_or(1.0)
    }

    pub fn volume(&self) -> f64 {
        let unit = self.unit_volume();
        if self.is_stackable() {
            unit * f64::from(self.stack_count())
        } else {
            unit
        }
    }

    pub fn unit_volume(&self) -> f64 {
        self.get_numeric_property("volume").unwrap_or(1.0)
    }

    pub fn stack_count(&self) -> u32 {
        self.get_int_property("stack_count").unwrap_or(1) as u32
    }

    pub fn max_stack(&self) -> u32 {
        self.get_int_property("max_stack").unwrap_or(99) as u32
    }

    pub fn set_stack_count(&mut self, count: u32) {
        self.set_property_int("stack_count", i64::from(count));
    }

    pub fn container_capacity(&self) -> u32 {
        self.get_int_property("capacity").unwrap_or(10) as u32
    }

    pub fn container_max_weight(&self) -> Option<i64> {
        self.get_int_property("max_weight")
    }

    pub fn container_max_volume(&self) -> Option<i64> {
        self.get_int_property("max_volume")
    }

    pub fn container_contents(&self) -> Vec<ObjectId> {
        self.get_object_list_property("contents")
    }

    /// Items worn on body slots (subset of `body_slots` for wear-type slots).
    pub fn worn_items(&self) -> HashMap<String, ObjectId> {
        self.body_slots()
    }

    pub fn apply_container_role(&mut self, spec: &ContainerSpec) {
        self.set_property_bool("is_container", true);
        self.set_property_int("capacity", i64::from(spec.capacity));
        self.set_property_list("contents", vec![]);
        if let Some(w) = spec.max_weight {
            self.set_property_int("max_weight", w);
        }
        if let Some(v) = spec.max_volume {
            self.set_property_int("max_volume", v);
        }
        self.set_property_bool("is_wearable", spec.wearable);
        if spec.wearable {
            let slot = spec.wear_slot.as_deref().unwrap_or("torso");
            self.set_property_string("wear_slot", slot);
        }
        self.set_property_bool("is_pocketable", false);
        if self.get_numeric_property("weight").is_none() {
            self.set_property_numeric("weight", 1.0);
        }
        if self.get_numeric_property("volume").is_none() {
            self.set_property_numeric("volume", 1.0);
        }
    }

    pub fn apply_wearable_role(&mut self, spec: &WearableSpec) {
        self.set_property_bool("is_wearable", true);
        self.set_property_string("wear_slot", &spec.wear_slot);
        self.set_property_numeric("weight", spec.weight);
        self.set_property_numeric("volume", spec.volume);
    }

    pub fn apply_item_phys(&mut self, spec: &ItemPhysSpec) {
        self.set_property_numeric("weight", spec.weight);
        self.set_property_numeric("volume", spec.volume);
        self.set_property_bool("is_pocketable", spec.pocketable);
        if !self.has_container_role() {
            self.set_property_bool("is_container", false);
        }
        if !self.has_wearable_role() {
            self.set_property_bool("is_wearable", false);
        }
    }

    pub fn apply_stackable_role(&mut self, spec: &StackableSpec) {
        self.set_property_bool("stackable", true);
        self.set_property_int("stack_count", i64::from(spec.count));
        self.set_property_int("max_stack", i64::from(spec.max_stack));
    }

    /// Initialize a naked player from a MUDL player template (creature role).
    pub fn init_creature_role(&mut self, template: &PlayerTemplate) {
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
        self.set_property_int(
            "max_weight",
            crate::object::weight::DEFAULT_PLAYER_MAX_WEIGHT,
        );
    }

    /// Backward-compatible alias for [`init_creature_role`](Self::init_creature_role).
    pub fn init_body(&mut self, template: &PlayerTemplate) {
        self.init_creature_role(template);
    }

    /// Default item properties (backward-compatible with pre-M1 objects).
    pub fn init_item_defaults(&mut self, pocketable: bool) {
        self.init_item_defaults_if_unset(pocketable);
    }

    /// Fill generic item fields only when not already set by a prototype or role.
    pub fn init_item_defaults_if_unset(&mut self, pocketable: bool) {
        if self.get_numeric_property("weight").is_none() {
            self.set_property_numeric("weight", 1.0);
        }
        if self.get_numeric_property("volume").is_none() {
            self.set_property_numeric("volume", 1.0);
        }
        if self.get_bool_property("is_pocketable").is_none() {
            self.set_property_bool("is_pocketable", pocketable);
        }
        if !self.has_container_role() && self.get_property("is_container").is_none() {
            self.set_property_bool("is_container", false);
        }
        if !self.has_wearable_role() && self.get_property("is_wearable").is_none() {
            self.set_property_bool("is_wearable", false);
        }
    }

    /// Default container properties (backward-compatible with pre-M1 objects).
    pub fn init_container_defaults(&mut self, capacity: u32, wearable: bool) {
        self.apply_container_role(&ContainerSpec {
            capacity,
            max_weight: None,
            max_volume: None,
            wearable,
            wear_slot: if wearable {
                Some("torso".to_string())
            } else {
                None
            },
        });
    }

    pub fn set_property_bool(&mut self, name: &str, value: bool) {
        self.add_property(Property {
            name: name.to_string(),
            value: Value::Bool(value),
            permissions: PermissionFlags::OWNER,
            behavior: None,
        });
    }

    pub fn set_property_int(&mut self, name: &str, value: i64) {
        self.add_property(Property {
            name: name.to_string(),
            value: Value::Int(value),
            permissions: PermissionFlags::OWNER,
            behavior: None,
        });
    }

    pub fn set_property_string(&mut self, name: &str, value: impl Into<String>) {
        self.add_property(Property {
            name: name.to_string(),
            value: Value::String(value.into()),
            permissions: PermissionFlags::OWNER,
            behavior: None,
        });
    }

    pub fn set_property_list(&mut self, name: &str, items: Vec<ObjectId>) {
        self.add_property(Property {
            name: name.to_string(),
            value: Value::List(items.into_iter().map(Value::ObjectRef).collect()),
            permissions: PermissionFlags::OWNER,
            behavior: None,
        });
    }

    pub fn get_bool_property(&self, name: &str) -> Option<bool> {
        self.get_property(name).and_then(|p| {
            if let Value::Bool(b) = &p.value {
                Some(*b)
            } else {
                None
            }
        })
    }

    pub fn get_int_property(&self, name: &str) -> Option<i64> {
        self.get_property(name).and_then(|p| {
            if let Value::Int(n) = &p.value {
                Some(*n)
            } else {
                None
            }
        })
    }

    pub fn get_numeric_property(&self, name: &str) -> Option<f64> {
        self.get_property(name).and_then(|p| match &p.value {
            Value::Int(n) => Some(*n as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        })
    }

    pub fn set_property_numeric(&mut self, name: &str, value: f64) {
        let stored = if value.fract().abs() < 1e-9
            && value >= i64::MIN as f64
            && value <= i64::MAX as f64
        {
            Value::Int(value.round() as i64)
        } else {
            Value::Float(value)
        };
        self.add_property(Property {
            name: name.to_string(),
            value: stored,
            permissions: PermissionFlags::OWNER,
            behavior: None,
        });
    }

    pub fn get_string_property(&self, name: &str) -> Option<String> {
        self.get_property(name).and_then(|p| {
            if let Value::String(s) = &p.value {
                Some(s.clone())
            } else {
                None
            }
        })
    }

    pub fn get_object_list_property(&self, name: &str) -> Vec<ObjectId> {
        self.get_property(name)
            .and_then(|p| {
                if let Value::List(items) = &p.value {
                    Some(
                        items
                            .iter()
                            .filter_map(|v| {
                                if let Value::ObjectRef(id) = v {
                                    Some(id.clone())
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

    pub fn is_container(&self) -> bool {
        self.has_container_role()
    }

    pub fn is_wearable(&self) -> bool {
        self.has_wearable_role()
    }

    pub fn hand_slot(&self) -> Option<String> {
        self.get_string_property("hand_slot")
    }

    pub fn wear_slot(&self) -> Option<String> {
        self.get_string_property("wear_slot")
    }

    pub fn carried_slot(&self) -> Option<String> {
        self.get_string_property("carried_slot")
    }

    pub fn set_carried_slot(&mut self, slot: Option<&str>) {
        if let Some(slot) = slot {
            self.set_property_string("carried_slot", slot);
        } else {
            self.properties.remove("carried_slot");
        }
    }

    pub(crate) fn add_to_list_property(&mut self, prop: &str, id: ObjectId) {
        let mut list = self.get_object_list_property(prop);
        if !list.contains(&id) {
            list.push(id);
            self.set_property_list(prop, list);
        }
    }

    pub(crate) fn remove_from_list_property(&mut self, prop: &str, id: &ObjectId) {
        let list: Vec<ObjectId> = self
            .get_object_list_property(prop)
            .into_iter()
            .filter(|item| item != id)
            .collect();
        self.set_property_list(prop, list);
    }

    /// Sum volume of all objects inside this container.
    pub fn contents_volume(&self, objects: &HashMap<ObjectId, Object>) -> f64 {
        self.container_contents()
            .iter()
            .filter_map(|id| objects.get(id))
            .map(|obj| obj.volume())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::PermissionFlags;

    fn bare_object(id: &str) -> Object {
        Object {
            id: ObjectId::new(id),
            name: "test".to_string(),
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
    fn container_role_sets_expected_properties() {
        let mut obj = bare_object("item:bag-001");
        obj.apply_container_role(&ContainerSpec {
            capacity: 5,
            max_weight: Some(100),
            max_volume: Some(50),
            wearable: true,
            wear_slot: Some("torso".to_string()),
        });

        assert!(obj.has_container_role());
        assert!(obj.has_wearable_role());
        assert_eq!(obj.container_capacity(), 5);
        assert_eq!(obj.container_max_weight(), Some(100));
        assert_eq!(obj.wear_slot(), Some("torso".to_string()));
    }

    #[test]
    fn stackable_weight_scales_with_count() {
        let mut obj = bare_object("item:coin-001");
        obj.set_property_int("weight", 2);
        obj.apply_stackable_role(&StackableSpec {
            count: 10,
            max_stack: 99,
        });
        assert!((obj.weight() - 20.0).abs() < f64::EPSILON);
        assert!((obj.unit_weight() - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn init_item_defaults_if_unset_preserves_role_phys() {
        let mut obj = bare_object("item:cloak-001");
        obj.apply_wearable_role(&WearableSpec {
            wear_slot: "back".to_string(),
            weight: 2.5,
            volume: 3.0,
        });
        obj.init_item_defaults_if_unset(false);

        assert!((obj.weight() - 2.5).abs() < f64::EPSILON);
        assert!((obj.volume() - 3.0).abs() < f64::EPSILON);
        assert_eq!(obj.get_bool_property("is_pocketable"), Some(false));
    }

    #[test]
    fn role_summary_reflects_active_roles() {
        let mut obj = bare_object("item:pack-001");
        obj.apply_container_role(&ContainerSpec::default());
        let roles = obj.roles();
        assert!(roles.container);
        assert!(!roles.creature);
    }
}
