use std::collections::HashMap;
use std::fmt;

use crate::mudl::{slot_display_name, AnatomyRegistry, BodyPlan, SlotType};
use crate::object::{Object, ObjectId, PermissionFlags, Property, Value};

/// Errors returned by inventory operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InventoryError {
    NotFound(String),
    NotInRoom,
    NotCarried,
    HandsFull,
    SlotFull(String),
    ContainerFull,
    NotContainer,
    NotWearable,
    NotWieldable,
    AlreadyCarrying,
    ContainerNotCarried,
    NoRoom,
    NoBodyPlan,
    InvalidTarget(String),
}

impl fmt::Display for InventoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(name) => write!(f, "You don't see any {name} here."),
            Self::NotInRoom => write!(f, "That isn't here."),
            Self::NotCarried => write!(f, "You aren't carrying that."),
            Self::HandsFull => write!(f, "Your hands are full."),
            Self::SlotFull(slot) => {
                write!(f, "Your {} is already occupied.", slot_display_name(slot))
            }
            Self::ContainerFull => write!(f, "That won't fit — it's full."),
            Self::NotContainer => write!(f, "That isn't a container."),
            Self::NotWearable => write!(f, "You can't wear that."),
            Self::NotWieldable => write!(f, "You can't wield that."),
            Self::AlreadyCarrying => write!(f, "You're already carrying that."),
            Self::ContainerNotCarried => write!(f, "You aren't carrying that container."),
            Self::NoRoom => write!(f, "You aren't anywhere."),
            Self::NoBodyPlan => write!(f, "You have no body plan."),
            Self::InvalidTarget(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for InventoryError {}

/// Mutable world slice used by inventory commands.
pub struct InventoryContext<'a> {
    pub player_id: &'a ObjectId,
    pub room_id: Option<&'a ObjectId>,
    pub objects: &'a mut HashMap<ObjectId, Object>,
    pub anatomy: &'a AnatomyRegistry,
}

impl Object {
    pub fn init_item_defaults(&mut self, pocketable: bool) {
        self.set_property_bool("is_pocketable", pocketable);
        self.set_property_bool("is_wearable", false);
        self.set_property_bool("is_container", false);
    }

    pub fn init_container_defaults(&mut self, capacity: u32, wearable: bool) {
        self.set_property_bool("is_container", true);
        self.set_property_int("capacity", i64::from(capacity));
        self.set_property_list("contents", vec![]);
        self.set_property_bool("is_wearable", wearable);
        self.set_property_bool("is_pocketable", false);
        if wearable {
            self.set_property_string("wear_slot", "torso");
        }
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
        self.get_bool_property("is_container").unwrap_or(false)
    }

    pub fn is_wearable(&self) -> bool {
        self.get_bool_property("is_wearable").unwrap_or(false)
    }

    pub fn hand_slot(&self) -> Option<String> {
        self.get_string_property("hand_slot")
    }

    pub fn wear_slot(&self) -> Option<String> {
        self.get_string_property("wear_slot")
    }

    pub fn container_capacity(&self) -> Option<u32> {
        self.get_int_property("capacity").map(|c| c as u32)
    }

    pub fn container_contents(&self) -> Vec<ObjectId> {
        self.get_object_list_property("contents")
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

    fn add_to_list_property(&mut self, prop: &str, id: ObjectId) {
        let mut list = self.get_object_list_property(prop);
        if !list.contains(&id) {
            list.push(id);
            self.set_property_list(prop, list);
        }
    }

    fn remove_from_list_property(&mut self, prop: &str, id: &ObjectId) {
        let list: Vec<ObjectId> = self
            .get_object_list_property(prop)
            .into_iter()
            .filter(|item| item != id)
            .collect();
        self.set_property_list(prop, list);
    }
}

fn player_body_plan<'a>(
    player: &Object,
    anatomy: &'a AnatomyRegistry,
) -> Result<&'a BodyPlan, InventoryError> {
    let plan_name = player.body_plan_name().ok_or(InventoryError::NoBodyPlan)?;
    anatomy
        .body_plan(&plan_name)
        .ok_or(InventoryError::NoBodyPlan)
}

fn name_matches(needle: &str, obj: &Object) -> bool {
    let name_lower = obj.name.to_lowercase();
    name_lower == needle
        || name_lower.contains(needle)
        || name_lower
            .split_whitespace()
            .any(|word| word == needle || word.starts_with(needle))
        || obj.aliases.iter().any(|a| {
            let alias = a.to_lowercase();
            alias == needle || alias.contains(needle)
        })
}

/// Where to search when resolving an inventory command target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolveScope {
    /// Items carried by the player (drop, put, remove, etc.).
    Carried,
    /// Items on the ground in the current location only (take/get).
    Ground,
    /// Carried items or items in the current location (wear).
    CarriedOrGround,
}

fn is_on_ground(
    obj: &Object,
    obj_id: &ObjectId,
    room_id: &ObjectId,
    player_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> bool {
    obj.location.as_ref() == Some(room_id) && !is_carried_by(player_id, obj_id, objects)
}

fn resolve_inventory_target(
    name: &str,
    room_id: Option<&ObjectId>,
    player_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
    scope: ResolveScope,
) -> Result<ObjectId, InventoryError> {
    let needle = name.to_lowercase();

    if needle == "self" || needle == "me" {
        return Ok(player_id.clone());
    }

    if needle == "here" {
        return room_id.cloned().ok_or(InventoryError::NoRoom);
    }

    let id = ObjectId::new(name);
    if let Some(obj) = objects.get(&id) {
        if obj.is_active() && scope_matches(obj, &id, room_id, player_id, objects, scope) {
            return Ok(id);
        }
    }

    let mut matches = Vec::new();
    for (obj_id, obj) in objects {
        if !obj.is_active() {
            continue;
        }
        if name_matches(&needle, obj) && scope_matches(obj, obj_id, room_id, player_id, objects, scope)
        {
            matches.push(obj_id.clone());
        }
    }

    match matches.len() {
        0 => Err(InventoryError::NotFound(name.to_string())),
        1 => Ok(matches[0].clone()),
        _ => Err(InventoryError::InvalidTarget(format!(
            "Which {name} do you mean?"
        ))),
    }
}

fn scope_matches(
    obj: &Object,
    obj_id: &ObjectId,
    room_id: Option<&ObjectId>,
    player_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
    scope: ResolveScope,
) -> bool {
    match scope {
        ResolveScope::Carried => is_carried_by(player_id, obj_id, objects),
        ResolveScope::Ground => room_id
            .is_some_and(|room| is_on_ground(obj, obj_id, room, player_id, objects)),
        ResolveScope::CarriedOrGround => {
            is_carried_by(player_id, obj_id, objects)
                || room_id.is_some_and(|room| obj.location.as_ref() == Some(room))
        }
    }
}

pub fn is_carried_by(
    player_id: &ObjectId,
    item_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> bool {
    let Some(player) = objects.get(player_id) else {
        return false;
    };

    if player.body_slots().values().any(|id| id == item_id) {
        return true;
    }

    for carried_id in player.carried_body_items() {
        if let Some(container) = objects.get(&carried_id) {
            if container.is_container() && container.container_contents().contains(item_id) {
                return true;
            }
        }
    }

    item_id == player_id
}

fn grasp_slot_free(player: &Object, slot: &str) -> bool {
    player.body_slot_item(slot).is_none()
}

fn place_in_grasp_slots(
    player_id: &ObjectId,
    item_id: &ObjectId,
    plan: &BodyPlan,
    objects: &mut HashMap<ObjectId, Object>,
) -> Result<Vec<String>, InventoryError> {
    let item = objects
        .get(item_id)
        .ok_or(InventoryError::NotCarried)?
        .clone();
    let player = objects.get(player_id).unwrap().clone();
    let hand_pref = item.hand_slot();
    let preference = hand_pref.as_deref().unwrap_or("right");

    let grasp_names: Vec<String> = plan.grasp_slots().iter().map(|s| s.name.clone()).collect();

    let (target_slots, carried_label) = if preference == "both" {
        let left = "left_hand";
        let right = "right_hand";
        if !grasp_slot_free(&player, left) || !grasp_slot_free(&player, right) {
            return Err(InventoryError::HandsFull);
        }
        (
            vec![left.to_string(), right.to_string()],
            Some(left.to_string()),
        )
    } else if preference == "left" {
        if !grasp_slot_free(&player, "left_hand") {
            return Err(InventoryError::HandsFull);
        }
        (vec!["left_hand".to_string()], Some("left_hand".to_string()))
    } else {
        if grasp_slot_free(&player, "right_hand") {
            (
                vec!["right_hand".to_string()],
                Some("right_hand".to_string()),
            )
        } else if grasp_slot_free(&player, "left_hand") {
            (vec!["left_hand".to_string()], Some("left_hand".to_string()))
        } else {
            return Err(InventoryError::HandsFull);
        }
    };

    for slot in &grasp_names {
        if target_slots.contains(slot) && !grasp_slot_free(&player, slot) {
            return Err(InventoryError::HandsFull);
        }
    }

    let mut player = objects.get(player_id).unwrap().clone();
    for slot in &target_slots {
        player.set_body_slot(slot, Some(item_id.clone()));
    }
    objects.insert(player_id.clone(), player);

    let mut item = objects.get(item_id).unwrap().clone();
    item.location = Some(player_id.clone());
    item.set_carried_slot(carried_label.as_deref().or(Some(target_slots[0].as_str())));
    objects.insert(item_id.clone(), item);

    Ok(target_slots)
}

fn place_in_wear_slot(
    player_id: &ObjectId,
    item_id: &ObjectId,
    slot: &str,
    objects: &mut HashMap<ObjectId, Object>,
) -> Result<(), InventoryError> {
    let player = objects.get(player_id).unwrap().clone();
    if player.body_slot_item(slot).is_some() {
        return Err(InventoryError::SlotFull(slot.to_string()));
    }

    let mut player = objects.get(player_id).unwrap().clone();
    player.set_body_slot(slot, Some(item_id.clone()));
    objects.insert(player_id.clone(), player);

    let mut item = objects.get(item_id).unwrap().clone();
    item.location = Some(player_id.clone());
    item.set_carried_slot(Some(slot));
    objects.insert(item_id.clone(), item);

    Ok(())
}

fn remove_from_player(
    player_id: &ObjectId,
    item_id: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
) {
    let mut player = objects.get(player_id).unwrap().clone();
    player.clear_item_from_body_slots(item_id);
    objects.insert(player_id.clone(), player);
}

pub fn take_item(
    ctx: &mut InventoryContext<'_>,
    item_name: &str,
) -> Result<String, InventoryError> {
    let room_id = ctx.room_id.ok_or(InventoryError::NoRoom)?.clone();
    let item_id = resolve_inventory_target(
        item_name,
        Some(&room_id),
        ctx.player_id,
        ctx.objects,
        ResolveScope::Ground,
    )?;

    let item = ctx
        .objects
        .get(&item_id)
        .ok_or_else(|| InventoryError::NotFound(item_name.to_string()))?
        .clone();

    if item.location.as_ref() != Some(&room_id) {
        return Err(InventoryError::NotInRoom);
    }
    if is_carried_by(ctx.player_id, &item_id, ctx.objects) {
        return Err(InventoryError::AlreadyCarrying);
    }

    let player = ctx.objects.get(ctx.player_id).unwrap().clone();
    let plan = player_body_plan(&player, ctx.anatomy)?;
    place_in_grasp_slots(ctx.player_id, &item_id, plan, ctx.objects)?;

    Ok(format!("You pick up the {}.", item.name))
}

pub fn drop_item(
    ctx: &mut InventoryContext<'_>,
    item_name: &str,
) -> Result<String, InventoryError> {
    let room_id = ctx.room_id.ok_or(InventoryError::NoRoom)?.clone();
    let item_id = resolve_inventory_target(
        item_name,
        None,
        ctx.player_id,
        ctx.objects,
        ResolveScope::Carried,
    )?;

    if !is_carried_by(ctx.player_id, &item_id, ctx.objects) {
        return Err(InventoryError::NotCarried);
    }

    let item = ctx.objects.get(&item_id).unwrap().clone();
    let item_name_display = item.name.clone();

    if item.is_container() {
        for contained in item.container_contents() {
            if let Some(mut inner) = ctx.objects.get(&contained).cloned() {
                inner.location = Some(room_id.clone());
                inner.set_carried_slot(None);
                ctx.objects.insert(contained, inner);
            }
        }
    }

    remove_from_player(ctx.player_id, &item_id, ctx.objects);

    let mut item = ctx.objects.get(&item_id).unwrap().clone();
    item.location = Some(room_id);
    item.set_carried_slot(None);
    ctx.objects.insert(item_id, item);

    Ok(format!("You drop the {item_name_display}."))
}

pub fn put_item(
    ctx: &mut InventoryContext<'_>,
    item_name: &str,
    container_name: &str,
) -> Result<String, InventoryError> {
    let item_id = resolve_inventory_target(
        item_name,
        None,
        ctx.player_id,
        ctx.objects,
        ResolveScope::Carried,
    )?;
    let container_id =
        resolve_inventory_target(
            container_name,
            None,
            ctx.player_id,
            ctx.objects,
            ResolveScope::Carried,
        )?;

    if item_id == container_id {
        return Err(InventoryError::InvalidTarget(
            "You can't put something inside itself.".into(),
        ));
    }

    if !is_carried_by(ctx.player_id, &item_id, ctx.objects) {
        return Err(InventoryError::NotCarried);
    }

    let container = ctx
        .objects
        .get(&container_id)
        .ok_or_else(|| InventoryError::NotFound(container_name.to_string()))?
        .clone();

    if !container.is_container() {
        return Err(InventoryError::NotContainer);
    }
    if !is_carried_by(ctx.player_id, &container_id, ctx.objects) {
        return Err(InventoryError::ContainerNotCarried);
    }

    let capacity = container.container_capacity().unwrap_or(10) as usize;
    if container.container_contents().len() >= capacity {
        return Err(InventoryError::ContainerFull);
    }

    let item = ctx.objects.get(&item_id).unwrap().clone();
    let item_display = item.name.clone();
    let container_display = container.name.clone();

    remove_from_player(ctx.player_id, &item_id, ctx.objects);

    let mut container = ctx.objects.get(&container_id).unwrap().clone();
    container.add_to_list_property("contents", item_id.clone());
    ctx.objects.insert(container_id.clone(), container);

    let mut item = ctx.objects.get(&item_id).unwrap().clone();
    item.location = Some(container_id);
    item.set_carried_slot(None);
    ctx.objects.insert(item_id, item);

    Ok(format!(
        "You put the {item_display} in your {container_display}."
    ))
}

pub fn remove_item(
    ctx: &mut InventoryContext<'_>,
    item_name: &str,
    container_name: &str,
) -> Result<String, InventoryError> {
    let container_id =
        resolve_inventory_target(
            container_name,
            None,
            ctx.player_id,
            ctx.objects,
            ResolveScope::Carried,
        )?;

    let container = ctx
        .objects
        .get(&container_id)
        .ok_or_else(|| InventoryError::NotFound(container_name.to_string()))?
        .clone();

    if !container.is_container() {
        return Err(InventoryError::NotContainer);
    }
    if !is_carried_by(ctx.player_id, &container_id, ctx.objects) {
        return Err(InventoryError::ContainerNotCarried);
    }

    let needle = item_name.to_lowercase();
    let item_id = container
        .container_contents()
        .into_iter()
        .find(|id| {
            ctx.objects
                .get(id)
                .is_some_and(|obj| name_matches(&needle, obj))
        })
        .ok_or_else(|| InventoryError::NotFound(item_name.to_string()))?;

    let item = ctx.objects.get(&item_id).unwrap().clone();
    let item_display = item.name.clone();
    let container_display = container.name.clone();

    let mut container = ctx.objects.get(&container_id).unwrap().clone();
    container.remove_from_list_property("contents", &item_id);
    ctx.objects.insert(container_id.clone(), container);

    let player = ctx.objects.get(ctx.player_id).unwrap().clone();
    let plan = player_body_plan(&player, ctx.anatomy)?;
    place_in_grasp_slots(ctx.player_id, &item_id, plan, ctx.objects)?;

    Ok(format!(
        "You remove the {item_display} from your {container_display}."
    ))
}

pub fn wield_item(
    ctx: &mut InventoryContext<'_>,
    item_name: &str,
) -> Result<String, InventoryError> {
    let item_id = resolve_inventory_target(
        item_name,
        None,
        ctx.player_id,
        ctx.objects,
        ResolveScope::Carried,
    )?;

    if !is_carried_by(ctx.player_id, &item_id, ctx.objects) {
        return Err(InventoryError::NotCarried);
    }

    let item = ctx.objects.get(&item_id).unwrap().clone();
    if item.is_container() && item.hand_slot().is_none() {
        return Err(InventoryError::NotWieldable);
    }

    remove_from_player(ctx.player_id, &item_id, ctx.objects);

    let player = ctx.objects.get(ctx.player_id).unwrap().clone();
    let plan = player_body_plan(&player, ctx.anatomy)?;
    place_in_grasp_slots(ctx.player_id, &item_id, plan, ctx.objects)?;

    let item = ctx.objects.get(&item_id).unwrap();
    let display = item.name.clone();
    let player = ctx.objects.get(ctx.player_id).unwrap();
    let left = player.body_slot_item("left_hand");
    let right = player.body_slot_item("right_hand");

    let phrase = if item.hand_slot().as_deref() == Some("both") || (left == right && left.is_some())
    {
        "wield"
    } else if right.as_ref() == Some(&item_id) {
        "hold in your right hand"
    } else {
        "hold in your left hand"
    };

    Ok(format!("You {phrase} the {display}."))
}

pub fn wear_item(
    ctx: &mut InventoryContext<'_>,
    item_name: &str,
) -> Result<String, InventoryError> {
    let room_id = ctx.room_id.ok_or(InventoryError::NoRoom)?.clone();
    let item_id = resolve_inventory_target(
        item_name,
        Some(&room_id),
        ctx.player_id,
        ctx.objects,
        ResolveScope::CarriedOrGround,
    )?;

    let item = ctx
        .objects
        .get(&item_id)
        .ok_or_else(|| InventoryError::NotFound(item_name.to_string()))?
        .clone();

    if !item.is_wearable() {
        return Err(InventoryError::NotWearable);
    }

    if is_carried_by(ctx.player_id, &item_id, ctx.objects) {
        remove_from_player(ctx.player_id, &item_id, ctx.objects);
    } else if item.location.as_ref() != Some(&room_id) {
        return Err(InventoryError::NotInRoom);
    }

    let player = ctx.objects.get(ctx.player_id).unwrap().clone();
    let plan = player_body_plan(&player, ctx.anatomy)?;
    let target_slot = item
        .wear_slot()
        .or_else(|| plan.wear_slots().first().map(|s| s.name.clone()))
        .ok_or(InventoryError::NotWearable)?;

    if plan.slot(&target_slot).is_none() {
        return Err(InventoryError::NotWearable);
    }

    place_in_wear_slot(ctx.player_id, &item_id, &target_slot, ctx.objects)?;

    let display = ctx.objects.get(&item_id).unwrap().name.clone();
    Ok(format!("You wear the {display}."))
}

fn any_wear_slots_occupied(player: &Object, plan: &BodyPlan) -> bool {
    plan.wear_slots()
        .iter()
        .any(|slot| player.body_slot_item(&slot.name).is_some())
}

struct HeldInGrasp {
    slot_name: String,
    slot: String,
    item_name: String,
}

fn grasp_slot_sort_key(name: &str) -> u8 {
    match name {
        "right_hand" => 0,
        "left_hand" => 1,
        _ => 2,
    }
}

fn grasp_held_items(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
    plan: &BodyPlan,
) -> Vec<HeldInGrasp> {
    let mut held = Vec::new();
    for slot in plan.grasp_slots() {
        if let Some(item_id) = player.body_slot_item(&slot.name) {
            if let Some(obj) = objects.get(&item_id) {
                if obj.is_active() {
                    held.push(HeldInGrasp {
                        slot_name: slot.name.clone(),
                        slot: slot_display_name(&slot.name),
                        item_name: obj.name.clone(),
                    });
                }
            }
        }
    }
    held.sort_by(|a, b| {
        grasp_slot_sort_key(&a.slot_name)
            .cmp(&grasp_slot_sort_key(&b.slot_name))
            .then(a.slot_name.cmp(&b.slot_name))
    });
    held
}

fn describe_grasp(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
    plan: &BodyPlan,
) -> Option<String> {
    let left = player.body_slot_item("left_hand");
    let right = player.body_slot_item("right_hand");

    if let (Some(left_id), Some(right_id)) = (&left, &right) {
        if left_id == right_id {
            if let Some(obj) = objects.get(left_id) {
                if obj.is_active() {
                    return Some(format!("You are wielding {} with both hands.", obj.name));
                }
            }
        }
    }

    let held = grasp_held_items(player, objects, plan);
    match held.len() {
        0 => None,
        1 => Some(format!(
            "You are holding {} in your {}.",
            held[0].item_name, held[0].slot
        )),
        2 => Some(format!(
            "You are holding {} in your {} and {} in your {}.",
            held[0].item_name, held[0].slot, held[1].item_name, held[1].slot
        )),
        _ => {
            let mut lines = vec!["You are holding:".to_string()];
            for entry in held {
                lines.push(format!("  - {} in your {}", entry.item_name, entry.slot));
            }
            Some(lines.join("\n"))
        }
    }
}

/// Natural-language summary of what a player is carrying (for look self).
pub fn describe_carried(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> String {
    let plan_name = player
        .body_plan_name()
        .unwrap_or_else(|| "human".to_string());
    let Some(plan) = anatomy.body_plan(&plan_name) else {
        return "You are completely naked and empty-handed.".to_string();
    };

    let naked = !any_wear_slots_occupied(player, plan);
    let grasp = describe_grasp(player, objects, plan);

    match (naked, grasp) {
        (true, None) => "You are completely naked and empty-handed.".to_string(),
        (false, None) => "You are wearing clothing.".to_string(),
        (true, Some(g)) => format!("You are completely naked.\n{g}"),
        (false, Some(g)) => g,
    }
}

/// Full inventory listing for the `inventory` command.
pub fn describe_inventory(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> String {
    let plan_name = player
        .body_plan_name()
        .unwrap_or_else(|| "human".to_string());
    let plan = anatomy.body_plan(&plan_name);

    let mut lines = Vec::new();
    let naked = plan.is_none_or(|p| !any_wear_slots_occupied(player, p));
    if naked {
        lines.push("You are completely naked.".to_string());
    }

    let mut entries = Vec::new();
    let slots = player.body_slots();
    let mut slot_names: Vec<String> = slots.keys().cloned().collect();
    slot_names.sort_unstable();

    for slot in &slot_names {
        if let Some(item_id) = slots.get(slot) {
            if let Some(obj) = objects.get(item_id) {
                if !obj.is_active() {
                    continue;
                }
                let left = player.body_slot_item("left_hand");
                let right = player.body_slot_item("right_hand");
                let placement = if slot.as_str() == "left_hand"
                    && right.as_ref() == Some(item_id)
                    && left.as_ref() == Some(item_id)
                {
                    "wielded in both hands".to_string()
                } else if plan
                    .as_ref()
                    .and_then(|p| p.slot(slot))
                    .is_some_and(|s| s.slot_type == SlotType::Wear)
                {
                    format!("worn on your {}", slot_display_name(slot))
                } else {
                    format!("in your {}", slot_display_name(slot))
                };
                entries.push(format!("  {} — {}", obj.name, placement));

                if obj.is_container() {
                    for inner_id in obj.container_contents() {
                        if let Some(inner) = objects.get(&inner_id) {
                            entries.push(format!(
                                "    {} — inside your {}",
                                inner.name, obj.name
                            ));
                        }
                    }
                }
            }
        }
    }

    if entries.is_empty() {
        lines.push("Your hands are empty.".to_string());
    } else {
        lines.push("You are carrying:".to_string());
        lines.extend(entries);
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mudl::load_module;
    use crate::object::ObjectFactory;
    use crate::persistence::SqlitePersistence;

    async fn test_anatomy() -> AnatomyRegistry {
        load_module("modules/default")
            .unwrap()
            .active_world()
            .unwrap()
            .anatomy
            .clone()
    }

    async fn setup_world() -> (
        ObjectFactory<SqlitePersistence>,
        AnatomyRegistry,
        ObjectId,
        ObjectId,
        HashMap<ObjectId, Object>,
    ) {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence);
        let anatomy = test_anatomy().await;
        let owner = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("room:test-001");

        let mut player = factory
            .create_player("hero", owner.clone(), &anatomy)
            .await
            .unwrap();
        player.location = Some(room_id.clone());

        let mut room = factory.create("room", "test", owner.clone()).await.unwrap();
        room.name = "Test Room".to_string();

        let mut coin = factory.create_item("coin", owner.clone()).await.unwrap();
        coin.name = "Gold Coin".to_string();
        coin.location = Some(room_id.clone());

        let mut sword = factory.create_item("sword", owner.clone()).await.unwrap();
        sword.name = "Rusty Sword".to_string();
        sword.set_property_string("hand_slot", "right");
        sword.location = Some(room_id.clone());

        let mut greatsword = factory
            .create_item("greatsword", owner.clone())
            .await
            .unwrap();
        greatsword.name = "Greatsword".to_string();
        greatsword.set_property_string("hand_slot", "both");
        greatsword.location = Some(room_id.clone());

        let mut backpack = factory
            .create_container("backpack", owner.clone(), 5, true)
            .await
            .unwrap();
        backpack.name = "Backpack".to_string();
        backpack.location = Some(room_id.clone());

        let mut objects = HashMap::new();
        objects.insert(player.id.clone(), player);
        objects.insert(room_id.clone(), room);
        objects.insert(coin.id.clone(), coin);
        objects.insert(sword.id.clone(), sword);
        objects.insert(greatsword.id.clone(), greatsword);
        objects.insert(backpack.id.clone(), backpack);

        (factory, anatomy, owner, room_id, objects)
    }

    #[tokio::test]
    async fn naked_player_has_no_pockets() {
        let (factory, anatomy, owner, _, _) = setup_world().await;
        let player = factory
            .create_player("naked", owner, &anatomy)
            .await
            .unwrap();
        assert!(player.get_property("pockets").is_none());
        assert_eq!(player.body_plan_name(), Some("human".to_string()));
        assert!(player.body_slots().is_empty());
    }

    #[tokio::test]
    async fn take_item_from_area_location() {
        let (_factory, anatomy, player_id, _, mut objects) = setup_world().await;
        let area_id = ObjectId::new("area:void-001");
        let mut area = objects
            .values()
            .find(|o| o.name == "Test Room")
            .unwrap()
            .clone();
        area.id = area_id.clone();

        if let Some(player) = objects.get_mut(&player_id) {
            player.location = Some(area_id.clone());
        }
        for obj in objects.values_mut() {
            if obj.location.as_ref() == Some(&ObjectId::new("room:test-001")) {
                obj.location = Some(area_id.clone());
            }
        }
        objects.insert(area_id.clone(), area);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&area_id),
            objects: &mut objects,
            anatomy: &anatomy,
        };

        take_item(&mut ctx, "coin").unwrap();
        let player = objects.get(&player_id).unwrap();
        assert!(
            player.body_slot_item("left_hand").is_some()
                || player.body_slot_item("right_hand").is_some()
        );
    }

    #[tokio::test]
    async fn take_item_to_hand() {
        let (_factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
        };

        take_item(&mut ctx, "coin").unwrap();
        let player = objects.get(&player_id).unwrap();
        assert!(
            player.body_slot_item("left_hand").is_some()
                || player.body_slot_item("right_hand").is_some()
        );
    }

    #[tokio::test]
    async fn take_non_pocketable_to_right_hand() {
        let (_factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
        };

        take_item(&mut ctx, "rusty").unwrap();
        let player = objects.get(&player_id).unwrap();
        assert_eq!(
            player
                .body_slot_item("right_hand")
                .map(|id| objects.get(&id).unwrap().name.clone()),
            Some("Rusty Sword".to_string())
        );
    }

    #[tokio::test]
    async fn two_handed_item_occupies_both_hands() {
        let (_factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
        };

        take_item(&mut ctx, "greatsword").unwrap();
        let player = objects.get(&player_id).unwrap();
        let gs_id = player.body_slot_item("left_hand").unwrap();
        assert_eq!(player.body_slot_item("right_hand"), Some(gs_id));
    }

    #[tokio::test]
    async fn describe_naked_empty_handed() {
        let (_factory, anatomy, player_id, _, objects) = setup_world().await;
        let player = objects.get(&player_id).unwrap();
        let desc = describe_carried(player, &objects, &anatomy);
        assert_eq!(desc, "You are completely naked and empty-handed.");
    }

    #[tokio::test]
    async fn describe_holding_sword() {
        let (_factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
        };
        take_item(&mut ctx, "rusty").unwrap();

        let player = objects.get(&player_id).unwrap();
        let desc = describe_carried(player, &objects, &anatomy);
        assert!(desc.contains("Rusty Sword"));
        assert!(desc.contains("right hand"));
    }

    #[tokio::test]
    async fn describe_carried_lists_both_hands() {
        let (factory, anatomy, player_id, room_id, mut objects) = minimal_take_world().await;

        let mut rusty = factory
            .create_named("sword", "rusty-sword", "Rusty Sword", player_id.clone())
            .await
            .unwrap();
        rusty.location = Some(room_id.clone());

        let mut wooden = factory
            .create_named("sword", "wooden-sword", "Wooden Sword", player_id.clone())
            .await
            .unwrap();
        wooden.location = Some(room_id.clone());

        objects.insert(rusty.id.clone(), rusty);
        objects.insert(wooden.id.clone(), wooden);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
        };

        take_item(&mut ctx, "rusty").unwrap();
        take_item(&mut ctx, "wooden").unwrap();

        let player = objects.get(&player_id).unwrap();
        let carried = describe_carried(player, &objects, &anatomy);
        assert!(carried.contains("You are completely naked."));
        assert!(carried.contains("Rusty Sword"));
        assert!(carried.contains("Wooden Sword"));
        assert!(carried.contains("right hand"));
        assert!(carried.contains("left hand"));

        let inv = describe_inventory(player, &objects, &anatomy);
        assert!(inv.contains("Rusty Sword — in your right hand"));
        assert!(inv.contains("Wooden Sword — in your left hand"));
    }

    async fn minimal_take_world() -> (
        ObjectFactory<SqlitePersistence>,
        AnatomyRegistry,
        ObjectId,
        ObjectId,
        HashMap<ObjectId, Object>,
    ) {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence);
        let anatomy = test_anatomy().await;
        let player_id = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("room:test-001");

        let mut player = factory
            .create_player("hero", player_id.clone(), &anatomy)
            .await
            .unwrap();
        player.location = Some(room_id.clone());

        let mut room = factory
            .create("room", "test", player_id.clone())
            .await
            .unwrap();
        room.name = "Test Room".to_string();

        let mut objects = HashMap::new();
        objects.insert(player.id.clone(), player);
        objects.insert(room_id.clone(), room);

        (factory, anatomy, player_id, room_id, objects)
    }

    #[tokio::test]
    async fn take_ignores_carried_items_when_one_on_ground() {
        let (factory, anatomy, player_id, room_id, mut objects) = minimal_take_world().await;

        let mut sword_held = factory
            .create_named("sword", "sword", "Sword", player_id.clone())
            .await
            .unwrap();
        sword_held.location = Some(player_id.clone());

        let mut sword_ground = factory
            .create_named("sword", "sword", "Sword", player_id.clone())
            .await
            .unwrap();
        sword_ground.location = Some(room_id.clone());

        let mut player = objects.get(&player_id).unwrap().clone();
        player.set_body_slot("right_hand", Some(sword_held.id.clone()));
        objects.insert(player_id.clone(), player);
        objects.insert(sword_held.id.clone(), sword_held);
        objects.insert(sword_ground.id.clone(), sword_ground.clone());

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
        };

        take_item(&mut ctx, "sword").unwrap();
        let player = objects.get(&player_id).unwrap();
        assert!(player.body_slot_item("left_hand").is_some());
        assert!(player.body_slot_item("right_hand").is_some());
        assert_eq!(
            objects.get(&sword_ground.id).unwrap().location.as_ref(),
            Some(&player_id)
        );
    }

    #[tokio::test]
    async fn take_two_swords_sequentially_from_ground() {
        let (factory, anatomy, player_id, room_id, mut objects) = minimal_take_world().await;

        let mut sword1 = factory
            .create_named("sword", "sword", "Sword", player_id.clone())
            .await
            .unwrap();
        sword1.location = Some(room_id.clone());

        let mut sword2 = factory
            .create_named("sword", "sword", "Sword", player_id.clone())
            .await
            .unwrap();
        sword2.location = Some(room_id.clone());

        objects.insert(sword1.id.clone(), sword1.clone());
        objects.insert(sword2.id.clone(), sword2);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
        };

        take_item(&mut ctx, sword1.id.as_str()).unwrap();
        take_item(&mut ctx, "sword").unwrap();

        let player = objects.get(&player_id).unwrap();
        assert!(player.body_slot_item("left_hand").is_some());
        assert!(player.body_slot_item("right_hand").is_some());

        let inv = describe_inventory(player, &objects, &anatomy);
        assert!(inv.contains("in your left hand"));
        assert!(inv.contains("in your right hand"));
    }

    #[tokio::test]
    async fn take_disambiguates_multiple_on_ground() {
        let (factory, anatomy, player_id, room_id, mut objects) = minimal_take_world().await;

        let mut sword1 = factory
            .create_named("sword", "sword", "Sword", player_id.clone())
            .await
            .unwrap();
        sword1.location = Some(room_id.clone());

        let mut sword2 = factory
            .create_named("sword", "sword", "Sword", player_id.clone())
            .await
            .unwrap();
        sword2.location = Some(room_id.clone());

        objects.insert(sword1.id.clone(), sword1);
        objects.insert(sword2.id.clone(), sword2);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
        };

        let err = take_item(&mut ctx, "sword").unwrap_err();
        assert_eq!(
            err,
            InventoryError::InvalidTarget("Which sword do you mean?".to_string())
        );
    }

    #[tokio::test]
    async fn wear_and_container_operations() {
        let (_factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let backpack_id = objects
            .values()
            .find(|o| o.name == "Backpack")
            .unwrap()
            .id
            .clone();

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
        };

        wear_item(&mut ctx, "backpack").unwrap();
        assert_eq!(
            ctx.objects.get(&player_id).unwrap().body_slot_item("torso"),
            Some(backpack_id.clone())
        );

        take_item(&mut ctx, "coin").unwrap();
        put_item(&mut ctx, "coin", "backpack").unwrap();

        let coin_id = objects
            .values()
            .find(|o| o.name == "Gold Coin")
            .unwrap()
            .id
            .clone();
        assert!(objects
            .get(&backpack_id)
            .unwrap()
            .container_contents()
            .contains(&coin_id));
    }
}
