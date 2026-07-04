//! Apply composable object roles from MUDL property definitions.

use crate::object::{ContainerSpec, ItemPhysSpec, Object, StackableSpec, WearableSpec};

/// Key-value role properties parsed from MUDL item/object blocks.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MudlRoleProps {
    pub is_container: Option<bool>,
    pub capacity: Option<u32>,
    pub max_weight: Option<i64>,
    pub max_volume: Option<i64>,
    pub is_wearable: Option<bool>,
    pub wear_slot: Option<String>,
    pub weight: Option<i64>,
    pub volume: Option<i64>,
    pub pocketable: Option<bool>,
    pub stackable: Option<bool>,
    pub stack_count: Option<u32>,
    pub max_stack: Option<u32>,
    pub hand_slot: Option<String>,
}

impl MudlRoleProps {
    /// Parse simple `key=value` pairs from MUDL property lines.
    pub fn from_pairs(pairs: &[(&str, &str)]) -> Self {
        let mut props = Self::default();
        for (key, value) in pairs {
            match *key {
                "is_container" => props.is_container = Some(*value == "true"),
                "capacity" => props.capacity = value.parse().ok(),
                "max_weight" => props.max_weight = value.parse().ok(),
                "max_volume" => props.max_volume = value.parse().ok(),
                "is_wearable" => props.is_wearable = Some(*value == "true"),
                "wear_slot" => props.wear_slot = Some(value.to_string()),
                "weight" => props.weight = value.parse().ok(),
                "volume" => props.volume = value.parse().ok(),
                "pocketable" | "is_pocketable" => props.pocketable = Some(*value == "true"),
                "stackable" => props.stackable = Some(*value == "true"),
                "stack_count" => props.stack_count = value.parse().ok(),
                "max_stack" => props.max_stack = value.parse().ok(),
                "hand_slot" => props.hand_slot = Some(value.to_string()),
                _ => {}
            }
        }
        props
    }

    /// Whether any scalar property overrides are present (weight, hand_slot, etc.).
    pub fn has_scalar_overrides(&self) -> bool {
        self.weight.is_some()
            || self.volume.is_some()
            || self.pocketable.is_some()
            || self.hand_slot.is_some()
    }

    /// Apply scalar overrides without re-applying role composition.
    pub fn apply_scalar_overrides(&self, obj: &mut Object) {
        if let Some(w) = self.weight {
            obj.set_property_int("weight", w);
        }
        if let Some(v) = self.volume {
            obj.set_property_int("volume", v);
        }
        if let Some(p) = self.pocketable {
            obj.set_property_bool("is_pocketable", p);
        }
        if let Some(ref slot) = self.hand_slot {
            obj.set_property_string("hand_slot", slot);
        }
    }

    /// Apply parsed role properties onto an object (composable — multiple roles allowed).
    pub fn apply_to(&self, obj: &mut Object) {
        if self.is_container == Some(true) {
            obj.apply_container_role(&ContainerSpec {
                capacity: self.capacity.unwrap_or(10),
                max_weight: self.max_weight,
                max_volume: self.max_volume,
                wearable: self.is_wearable.unwrap_or(false),
                wear_slot: self.wear_slot.clone(),
            });
        } else if let (Some(w), Some(v)) = (self.weight, self.volume) {
            obj.apply_item_phys(&ItemPhysSpec {
                weight: w,
                volume: v,
                pocketable: self.pocketable.unwrap_or(true),
            });
        } else if self.weight.is_some() || self.volume.is_some() {
            obj.apply_item_phys(&ItemPhysSpec {
                weight: self.weight.unwrap_or(1),
                volume: self.volume.unwrap_or(1),
                pocketable: self.pocketable.unwrap_or(true),
            });
        }

        if self.is_wearable == Some(true) && self.is_container != Some(true) {
            obj.apply_wearable_role(&WearableSpec {
                wear_slot: self
                    .wear_slot
                    .clone()
                    .unwrap_or_else(|| "torso".to_string()),
                weight: self.weight.unwrap_or(1),
                volume: self.volume.unwrap_or(1),
            });
        }

        if self.stackable == Some(true) {
            obj.apply_stackable_role(&StackableSpec {
                count: self.stack_count.unwrap_or(1),
                max_stack: self.max_stack.unwrap_or(99),
            });
        }

        if let Some(ref slot) = self.hand_slot {
            obj.set_property_string("hand_slot", slot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{Object, ObjectId, PermissionFlags};
    use std::collections::HashMap;

    fn bare(id: &str) -> Object {
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
    fn mudl_role_props_apply_container() {
        let props = MudlRoleProps::from_pairs(&[
            ("is_container", "true"),
            ("capacity", "8"),
            ("max_weight", "40"),
        ]);
        let mut obj = bare("item:bag-001");
        props.apply_to(&mut obj);
        assert!(obj.is_container());
        assert_eq!(obj.container_capacity(), 8);
        assert_eq!(obj.container_max_weight(), Some(40));
    }

    #[test]
    fn mudl_role_props_apply_stackable() {
        let props = MudlRoleProps::from_pairs(&[
            ("stackable", "true"),
            ("stack_count", "50"),
            ("weight", "1"),
        ]);
        let mut obj = bare("item:coin-001");
        props.apply_to(&mut obj);
        assert!(obj.is_stackable());
        assert_eq!(obj.stack_count(), 50);
    }
}
