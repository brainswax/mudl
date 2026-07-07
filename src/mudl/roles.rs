//! Apply composable object roles from MUDL property definitions.

use std::collections::HashMap;

use crate::object::{
    BreakableSpec, ContainerSpec, ItemPhysSpec, KeySpec, Object, PortalKind, PortalSpec,
    ReadableSpec, StackableSpec, WearableSpec,
};

/// Key-value role properties parsed from MUDL item/object blocks.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MudlRoleProps {
    pub is_container: Option<bool>,
    pub is_open: Option<bool>,
    pub capacity: Option<u32>,
    pub max_weight: Option<i64>,
    pub max_volume: Option<i64>,
    pub is_wearable: Option<bool>,
    pub wear_slot: Option<String>,
    pub weight: Option<f64>,
    pub volume: Option<f64>,
    pub pocketable: Option<bool>,
    pub stackable: Option<bool>,
    pub stack_count: Option<u32>,
    pub max_stack: Option<u32>,
    pub hand_slot: Option<String>,
    pub readable: Option<bool>,
    pub read_text: Option<String>,
    pub writable: Option<bool>,
    pub write_text: Option<String>,
    pub locked: Option<bool>,
    pub lock_id: Option<String>,
    pub is_key: Option<bool>,
    pub key_consumable: Option<bool>,
    pub lock_consumable: Option<bool>,
    pub allowed_types: Option<String>,
    pub is_door: Option<bool>,
    pub is_window: Option<bool>,
    pub portal_kind: Option<String>,
    pub door_direction: Option<String>,
    pub door_destination: Option<String>,
    pub portal_passable: Option<bool>,
    pub portal_transparent: Option<bool>,
    pub mod_max_weight: Option<i64>,
    pub mod_encumbrance: Option<f64>,
    pub mod_max_health: Option<i64>,
    pub stat_mods: HashMap<String, i64>,
    pub skill_mods: HashMap<String, i64>,
    pub grant_effects: Vec<String>,
    pub breakable: Option<bool>,
    pub break_text: Option<String>,
}

impl MudlRoleProps {
    /// Parse simple `key=value` pairs from MUDL property lines.
    pub fn from_pairs(pairs: &[(&str, &str)]) -> Self {
        let mut props = Self::default();
        for (key, value) in pairs {
            match *key {
                "is_container" => props.is_container = Some(*value == "true"),
                "is_open" | "open" => props.is_open = Some(*value == "true"),
                "capacity" => props.capacity = value.parse().ok(),
                "max_weight" => props.max_weight = value.parse().ok(),
                "max_volume" => props.max_volume = value.parse().ok(),
                "is_wearable" => props.is_wearable = Some(*value == "true"),
                "wear_slot" => props.wear_slot = Some(value.to_string()),
                "weight" => props.weight = value.parse::<f64>().ok().filter(|n| n.is_finite()),
                "volume" => props.volume = value.parse::<f64>().ok().filter(|n| n.is_finite()),
                "pocketable" | "is_pocketable" => props.pocketable = Some(*value == "true"),
                "stackable" => props.stackable = Some(*value == "true"),
                "stack_count" => props.stack_count = value.parse().ok(),
                "max_stack" => props.max_stack = value.parse().ok(),
                "hand_slot" => props.hand_slot = Some(value.to_string()),
                "readable" | "is_readable" => props.readable = Some(*value == "true"),
                "read_text" | "text" => props.read_text = Some(value.to_string()),
                "writable" | "is_writable" => props.writable = Some(*value == "true"),
                "write_text" => props.write_text = Some(value.to_string()),
                "locked" | "is_locked" => props.locked = Some(*value == "true"),
                "lock_id" => props.lock_id = Some(value.to_string()),
                "is_key" | "key" => props.is_key = Some(*value == "true"),
                "key_consumable" | "consumable_key" => {
                    props.key_consumable = Some(*value == "true")
                }
                "lock_consumable" | "consumable_lock" => {
                    props.lock_consumable = Some(*value == "true")
                }
                "allowed_types" => props.allowed_types = Some(value.to_string()),
                "is_door" | "door" => props.is_door = Some(*value == "true"),
                "is_window" | "window" => props.is_window = Some(*value == "true"),
                "portal_kind" | "portal" => props.portal_kind = Some(value.to_string()),
                "door_direction" | "portal_direction" | "direction" => {
                    props.door_direction = Some(value.to_string())
                }
                "door_destination" | "portal_destination" | "destination" => {
                    props.door_destination = Some(value.to_string())
                }
                "portal_passable" | "passable" => props.portal_passable = Some(*value == "true"),
                "portal_transparent" | "transparent" => {
                    props.portal_transparent = Some(*value == "true")
                }
                "mod_max_weight" | "carry_bonus" | "max_weight_bonus" => {
                    props.mod_max_weight = value.parse().ok()
                }
                "mod_encumbrance" | "encumbrance_factor" | "encumbrance_reduction" => {
                    props.mod_encumbrance = value.parse::<f64>().ok().filter(|n| n.is_finite())
                }
                "mod_max_health" | "mod_health" | "health_bonus" => {
                    props.mod_max_health = value.parse().ok()
                }
                "grant_effect" | "effect" => {
                    let name = value.trim().to_string();
                    if !name.is_empty() && !props.grant_effects.contains(&name) {
                        props.grant_effects.push(name);
                    }
                }
                key if key.starts_with("mod_stat_") => {
                    let stat = key.trim_start_matches("mod_stat_");
                    if let Ok(v) = value.parse::<i64>() {
                        props.stat_mods.insert(stat.to_string(), v);
                    }
                }
                key if key.starts_with("mod_skill_") => {
                    let skill = key.trim_start_matches("mod_skill_");
                    if let Ok(v) = value.parse::<i64>() {
                        props.skill_mods.insert(skill.to_string(), v);
                    }
                }
                "breakable" | "is_breakable" => props.breakable = Some(*value == "true"),
                "break_text" | "on_break" => props.break_text = Some(value.to_string()),
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
            || self.mod_max_weight.is_some()
            || self.mod_encumbrance.is_some()
            || self.mod_max_health.is_some()
            || !self.stat_mods.is_empty()
            || !self.skill_mods.is_empty()
            || !self.grant_effects.is_empty()
    }

    /// Apply scalar overrides without re-applying role composition.
    pub fn apply_scalar_overrides(&self, obj: &mut Object) {
        if let Some(w) = self.weight {
            obj.set_property_numeric("weight", w);
        }
        if let Some(v) = self.volume {
            obj.set_property_numeric("volume", v);
        }
        if let Some(p) = self.pocketable {
            obj.set_property_bool("is_pocketable", p);
        }
        if let Some(ref slot) = self.hand_slot {
            obj.set_property_string("hand_slot", slot);
        }
        if self.mod_max_weight.is_some() || self.mod_encumbrance.is_some() {
            obj.apply_carry_modifiers(self.mod_max_weight, self.mod_encumbrance);
        }
        if self.mod_max_health.is_some()
            || !self.stat_mods.is_empty()
            || !self.skill_mods.is_empty()
            || !self.grant_effects.is_empty()
        {
            obj.apply_equipment_mods(
                self.mod_max_health,
                self.stat_mods.clone(),
                self.skill_mods.clone(),
                self.grant_effects.clone(),
            );
        }
    }

    /// Apply parsed role properties onto an object (composable — multiple roles allowed).
    pub fn apply_to(&self, obj: &mut Object) {
        if self.is_door == Some(true)
            || self.is_window == Some(true)
            || self.portal_kind.is_some()
            || self.door_direction.is_some()
            || self.door_destination.is_some()
        {
            let kind = self
                .portal_kind
                .as_deref()
                .and_then(PortalKind::parse)
                .or_else(|| {
                    if self.is_window == Some(true) {
                        Some(PortalKind::Window)
                    } else if self.is_door == Some(true) {
                        Some(PortalKind::Door)
                    } else {
                        obj.portal_kind()
                    }
                })
                .unwrap_or(PortalKind::Door);
            let direction = self
                .door_direction
                .clone()
                .or_else(|| obj.portal_direction())
                .unwrap_or_else(|| "in".to_string());
            let destination = self
                .door_destination
                .clone()
                .or_else(|| obj.portal_destination_base())
                .unwrap_or_default();
            obj.apply_portal_role(&PortalSpec {
                kind,
                direction,
                destination,
                open: self
                    .is_open
                    .or_else(|| obj.get_bool_property("is_open"))
                    .unwrap_or(false),
                lock_id: self.lock_id.clone().or_else(|| obj.container_lock_id()),
                locked: self
                    .locked
                    .or_else(|| obj.get_bool_property("is_locked"))
                    .unwrap_or(false),
                lock_consumable: self
                    .lock_consumable
                    .or_else(|| obj.get_bool_property("lock_consumable"))
                    .unwrap_or(false),
                passable: self.portal_passable,
                transparent: self.portal_transparent,
            });
            if let Some(w) = self.weight {
                obj.set_property_numeric("weight", w);
            }
            if let Some(v) = self.volume {
                obj.set_property_numeric("volume", v);
            }
        } else if self.is_container == Some(true) {
            obj.apply_container_role(&ContainerSpec {
                capacity: self.capacity.unwrap_or(10),
                max_weight: self.max_weight,
                max_volume: self.max_volume,
                wearable: self.is_wearable.unwrap_or(false),
                wear_slot: self.wear_slot.clone(),
                open: self.is_open.unwrap_or(true),
                lock_id: self.lock_id.clone(),
                locked: self.locked.unwrap_or(false),
                lock_consumable: self.lock_consumable.unwrap_or(false),
                allowed_types: self
                    .allowed_types
                    .as_ref()
                    .map(|s| crate::object::parse_allowed_types(s))
                    .filter(|types| !types.is_empty()),
            });
        } else if let (Some(w), Some(v)) = (self.weight, self.volume) {
            obj.apply_item_phys(&ItemPhysSpec {
                weight: w,
                volume: v,
                pocketable: self.pocketable.unwrap_or(true),
            });
        } else if self.weight.is_some() || self.volume.is_some() {
            obj.apply_item_phys(&ItemPhysSpec {
                weight: self.weight.unwrap_or(1.0),
                volume: self.volume.unwrap_or(1.0),
                pocketable: self.pocketable.unwrap_or(true),
            });
        }

        if self.is_wearable == Some(true) && self.is_container != Some(true) {
            let mut spec = WearableSpec::new(
                self.wear_slot
                    .clone()
                    .unwrap_or_else(|| "torso".to_string()),
                self.weight.unwrap_or(1.0),
                self.volume.unwrap_or(1.0),
            );
            spec.mod_max_weight = self.mod_max_weight;
            spec.mod_encumbrance = self.mod_encumbrance;
            spec.mod_max_health = self.mod_max_health;
            spec.stat_mods = self.stat_mods.clone();
            spec.skill_mods = self.skill_mods.clone();
            spec.grant_effects = self.grant_effects.clone();
            obj.apply_wearable_role(&spec);
        } else if self.mod_max_weight.is_some() || self.mod_encumbrance.is_some() {
            obj.apply_carry_modifiers(self.mod_max_weight, self.mod_encumbrance);
            if self.mod_max_health.is_some()
                || !self.stat_mods.is_empty()
                || !self.skill_mods.is_empty()
                || !self.grant_effects.is_empty()
            {
                obj.apply_equipment_mods(
                    self.mod_max_health,
                    self.stat_mods.clone(),
                    self.skill_mods.clone(),
                    self.grant_effects.clone(),
                );
            }
        } else if self.mod_max_health.is_some()
            || !self.stat_mods.is_empty()
            || !self.skill_mods.is_empty()
            || !self.grant_effects.is_empty()
        {
            obj.apply_equipment_mods(
                self.mod_max_health,
                self.stat_mods.clone(),
                self.skill_mods.clone(),
                self.grant_effects.clone(),
            );
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

        if self.readable == Some(true) || self.read_text.is_some() {
            obj.apply_readable_role(&ReadableSpec {
                text: self.read_text.clone().unwrap_or_default(),
                writable: self.writable.unwrap_or(false),
            });
        }
        if let Some(ref text) = self.write_text {
            obj.set_property_string("write_text", text);
        }

        if self.is_key == Some(true) {
            if let Some(ref lock_id) = self.lock_id {
                let mut spec = KeySpec::new(lock_id.clone());
                if self.key_consumable == Some(true) {
                    spec = spec.consumable();
                }
                obj.apply_key_role(&spec);
            }
        }

        if self.breakable == Some(true) {
            obj.apply_breakable_role(&BreakableSpec {
                break_text: self.break_text.clone(),
            });
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
    fn mudl_role_props_parse_is_open() {
        let props = MudlRoleProps::from_pairs(&[("is_container", "true"), ("is_open", "false")]);
        let mut obj = bare("item:chest-001");
        props.apply_to(&mut obj);
        assert!(!obj.container_is_open());
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
    fn mudl_role_props_apply_readable() {
        let props =
            MudlRoleProps::from_pairs(&[("readable", "true"), ("read_text", "Mind the dark.")]);
        let mut obj = bare("item:note-001");
        props.apply_to(&mut obj);
        assert!(obj.is_readable());
        assert_eq!(obj.read_text().as_deref(), Some("Mind the dark."));
    }

    #[test]
    fn mudl_role_props_apply_allowed_types_on_container() {
        let props =
            MudlRoleProps::from_pairs(&[("is_container", "true"), ("allowed_types", "key")]);
        let mut obj = bare("item:ring-001");
        props.apply_to(&mut obj);
        assert_eq!(obj.container_allowed_types(), Some(vec!["key".to_string()]));
    }

    #[test]
    fn mudl_role_props_apply_consumable_key_and_lock() {
        let props = MudlRoleProps::from_pairs(&[
            ("is_key", "true"),
            ("lock_id", "oak-whisper"),
            ("key_consumable", "true"),
        ]);
        let mut key = bare("item:charm-001");
        props.apply_to(&mut key);
        assert!(key.is_key());
        assert!(key.key_consumable());

        let portal_props = MudlRoleProps::from_pairs(&[
            ("is_door", "true"),
            ("door_direction", "in"),
            ("door_destination", "haunted-entry"),
            ("locked", "true"),
            ("lock_id", "oak-whisper"),
            ("lock_consumable", "true"),
        ]);
        let mut oak = bare("item:oak-001");
        portal_props.apply_to(&mut oak);
        assert!(oak.lock_consumable());
    }

    #[test]
    fn mudl_role_props_apply_door() {
        let props = MudlRoleProps::from_pairs(&[
            ("is_door", "true"),
            ("door_direction", "in"),
            ("door_destination", "cottage-interior"),
            ("is_open", "false"),
            ("locked", "true"),
            ("lock_id", "cottage-door"),
        ]);
        let mut obj = bare("item:door-001");
        props.apply_to(&mut obj);
        assert!(obj.is_door());
        assert_eq!(obj.portal_direction().as_deref(), Some("in"));
        assert_eq!(
            obj.portal_destination_base().as_deref(),
            Some("cottage-interior")
        );
        assert!(!obj.gate_is_open());
        assert!(obj.gate_is_locked());
        assert_eq!(obj.container_lock_id().as_deref(), Some("cottage-door"));
        assert!(obj.portal_passable());
        assert!(!obj.portal_transparent());
    }

    #[test]
    fn mudl_role_props_apply_wearable_carry_modifiers() {
        let props = MudlRoleProps::from_pairs(&[
            ("is_wearable", "true"),
            ("wear_slot", "left_foot"),
            ("mod_max_weight", "25"),
            ("mod_encumbrance", "0.85"),
        ]);
        let mut obj = bare("item:boots-001");
        props.apply_to(&mut obj);
        assert!(obj.is_wearable());
        assert_eq!(obj.carry_max_weight_bonus(), 25);
        assert!((obj.carry_encumbrance_factor() - 0.85).abs() < f64::EPSILON);
    }

    #[test]
    fn mudl_role_props_apply_window() {
        let props = MudlRoleProps::from_pairs(&[
            ("is_window", "true"),
            ("door_direction", "east"),
            ("door_destination", "cottage-rear"),
            ("is_open", "false"),
        ]);
        let mut obj = bare("item:window-001");
        props.apply_to(&mut obj);
        assert!(obj.is_window());
        assert!(!obj.portal_passable());
        assert!(obj.portal_transparent());
        assert!(obj.portal_allows_view());
    }

    #[test]
    fn mudl_role_props_apply_breakable() {
        let props = MudlRoleProps::from_pairs(&[
            ("breakable", "true"),
            ("break_text", "Shards everywhere."),
            ("weight", "2"),
            ("volume", "2"),
        ]);
        let mut obj = bare("item:pot-001");
        props.apply_to(&mut obj);
        assert!(obj.is_breakable());
        assert_eq!(obj.break_text().as_deref(), Some("Shards everywhere."));
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
