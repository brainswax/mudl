use std::collections::HashMap;
use std::fmt;

use super::object::{Object, ObjectId, PermissionFlags, Property, Value};

/// Where a carried item sits on its bearer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CarriedSlot {
    Pocket,
    LeftHand,
    RightHand,
    Worn,
}

impl CarriedSlot {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pocket => "pocket",
            Self::LeftHand => "left_hand",
            Self::RightHand => "right_hand",
            Self::Worn => "worn",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pocket" => Some(Self::Pocket),
            "left_hand" => Some(Self::LeftHand),
            "right_hand" => Some(Self::RightHand),
            "worn" => Some(Self::Worn),
            _ => None,
        }
    }
}

/// Errors returned by inventory operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InventoryError {
    NotFound(String),
    NotInRoom,
    NotCarried,
    HandsFull,
    PocketsFull,
    ContainerFull,
    NotContainer,
    NotPocketable,
    NotWearable,
    NotWieldable,
    AlreadyCarrying,
    ContainerNotCarried,
    NoRoom,
    InvalidTarget(String),
}

impl fmt::Display for InventoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(name) => write!(f, "You don't see any {name} here."),
            Self::NotInRoom => write!(f, "That isn't here."),
            Self::NotCarried => write!(f, "You aren't carrying that."),
            Self::HandsFull => write!(f, "Your hands are full."),
            Self::PocketsFull => write!(f, "Your pockets are full."),
            Self::ContainerFull => write!(f, "That won't fit — it's full."),
            Self::NotContainer => write!(f, "That isn't a container."),
            Self::NotPocketable => write!(f, "That's too bulky for your pockets."),
            Self::NotWearable => write!(f, "You can't wear that."),
            Self::NotWieldable => write!(f, "You can't wield that."),
            Self::AlreadyCarrying => write!(f, "You're already carrying that."),
            Self::ContainerNotCarried => write!(f, "You aren't carrying that container."),
            Self::NoRoom => write!(f, "You aren't anywhere."),
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
}

impl Object {
    pub fn init_inventory(&mut self) {
        self.set_property_list("pockets", vec![]);
        self.set_property_int("pocket_capacity", 5);
        self.set_property_list("worn", vec![]);
    }

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
        self.set_property_bool("is_pocketable", !wearable);
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

    pub fn get_object_ref_property(&self, name: &str) -> Option<ObjectId> {
        self.get_property(name).and_then(|p| {
            if let Value::ObjectRef(id) = &p.value {
                Some(id.clone())
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

    pub fn set_object_ref_property(&mut self, name: &str, id: Option<ObjectId>) {
        if let Some(id) = id {
            self.add_property(Property {
                name: name.to_string(),
                value: Value::ObjectRef(id),
                permissions: PermissionFlags::OWNER,
                behavior: None,
            });
        } else {
            self.properties.remove(name);
        }
    }

    pub fn is_container(&self) -> bool {
        self.get_bool_property("is_container").unwrap_or(false)
    }

    pub fn is_pocketable(&self) -> bool {
        self.get_bool_property("is_pocketable").unwrap_or(true)
    }

    pub fn is_wearable(&self) -> bool {
        self.get_bool_property("is_wearable").unwrap_or(false)
    }

    pub fn hand_slot(&self) -> Option<String> {
        self.get_string_property("hand_slot")
    }

    pub fn container_capacity(&self) -> Option<u32> {
        self.get_int_property("capacity").map(|c| c as u32)
    }

    pub fn container_contents(&self) -> Vec<ObjectId> {
        self.get_object_list_property("contents")
    }

    pub fn carried_slot(&self) -> Option<CarriedSlot> {
        self.get_string_property("carried_slot")
            .and_then(|s| CarriedSlot::parse(&s))
    }

    pub fn set_carried_slot(&mut self, slot: Option<CarriedSlot>) {
        if let Some(slot) = slot {
            self.set_property_string("carried_slot", slot.as_str());
        } else {
            self.properties.remove("carried_slot");
        }
    }

    pub fn pockets(&self) -> Vec<ObjectId> {
        self.get_object_list_property("pockets")
    }

    pub fn worn_containers(&self) -> Vec<ObjectId> {
        self.get_object_list_property("worn")
    }

    pub fn pocket_capacity(&self) -> u32 {
        self.get_int_property("pocket_capacity")
            .map(|c| c as u32)
            .unwrap_or(5)
    }

    pub fn left_hand_item(&self) -> Option<ObjectId> {
        self.get_object_ref_property("left_hand")
    }

    pub fn right_hand_item(&self) -> Option<ObjectId> {
        self.get_object_ref_property("right_hand")
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

fn resolve_inventory_target(
    name: &str,
    room_id: Option<&ObjectId>,
    player_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
    carried_only: bool,
) -> Result<ObjectId, InventoryError> {
    let needle = name.to_lowercase();

    if needle == "self" || needle == "me" {
        return Ok(player_id.clone());
    }

    if needle == "here" {
        return room_id.cloned().ok_or(InventoryError::NoRoom);
    }

    let id = ObjectId::new(name);
    if objects.contains_key(&id) {
        return Ok(id);
    }

    let mut matches = Vec::new();
    for (obj_id, obj) in objects {
        if name_matches(&needle, obj) {
            if carried_only {
                if is_carried_by(player_id, obj_id, objects) {
                    matches.push(obj_id.clone());
                }
            } else if let Some(room) = room_id {
                if obj.location.as_ref() == Some(room) || is_carried_by(player_id, obj_id, objects)
                {
                    matches.push(obj_id.clone());
                }
            } else if is_carried_by(player_id, obj_id, objects) {
                matches.push(obj_id.clone());
            }
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

pub fn is_carried_by(
    player_id: &ObjectId,
    item_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> bool {
    let Some(player) = objects.get(player_id) else {
        return false;
    };

    if player.pockets().contains(item_id)
        || player.left_hand_item().as_ref() == Some(item_id)
        || player.right_hand_item().as_ref() == Some(item_id)
        || player.worn_containers().contains(item_id)
    {
        return true;
    }

    // Items inside a carried container
    for container_id in player
        .worn_containers()
        .iter()
        .chain(player.pockets().iter())
    {
        if let Some(container) = objects.get(container_id) {
            if container.is_container() && container.container_contents().contains(item_id) {
                return true;
            }
        }
    }
    if let (Some(left), Some(right)) = (player.left_hand_item(), player.right_hand_item()) {
        for hand_container in [&left, &right] {
            if let Some(container) = objects.get(hand_container) {
                if container.is_container() && container.container_contents().contains(item_id) {
                    return true;
                }
            }
        }
    }

    item_id == player_id
}

fn hands_free(player: &Object, objects: &HashMap<ObjectId, Object>) -> (bool, bool) {
    let left_taken = player
        .left_hand_item()
        .map(|id| objects.get(&id).is_some())
        .unwrap_or(false);
    let right_taken = player
        .right_hand_item()
        .map(|id| objects.get(&id).is_some())
        .unwrap_or(false);
    (!left_taken, !right_taken)
}

fn place_in_pocket(
    player_id: &ObjectId,
    item_id: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
) -> Result<(), InventoryError> {
    let player = objects
        .get(player_id)
        .ok_or_else(|| InventoryError::InvalidTarget("Player not found.".into()))?;
    let pockets = player.pockets();
    if pockets.len() >= player.pocket_capacity() as usize {
        return Err(InventoryError::PocketsFull);
    }

    let item = objects
        .get(item_id)
        .ok_or(InventoryError::NotCarried)?
        .clone();
    if !item.is_pocketable() {
        return Err(InventoryError::NotPocketable);
    }

    let mut player = objects.get(player_id).unwrap().clone();
    player.add_to_list_property("pockets", item_id.clone());
    objects.insert(player_id.clone(), player);

    let mut item = objects.get(item_id).unwrap().clone();
    item.location = Some(player_id.clone());
    item.set_carried_slot(Some(CarriedSlot::Pocket));
    objects.insert(item_id.clone(), item);

    Ok(())
}

fn place_in_hand(
    player_id: &ObjectId,
    item_id: &ObjectId,
    slot: CarriedSlot,
    objects: &mut HashMap<ObjectId, Object>,
) -> Result<(), InventoryError> {
    let item = objects
        .get(item_id)
        .ok_or(InventoryError::NotCarried)?
        .clone();
    let hand_slot_value = item.hand_slot();
    let hand_slot = hand_slot_value.as_deref().unwrap_or("right");

    let player = objects.get(player_id).unwrap().clone();
    let (left_free, right_free) = hands_free(&player, objects);

    let (use_left, use_right) = match hand_slot {
        "both" => {
            if !left_free || !right_free {
                return Err(InventoryError::HandsFull);
            }
            (true, true)
        }
        "left" => {
            if !left_free {
                return Err(InventoryError::HandsFull);
            }
            (true, false)
        }
        _ => {
            if slot == CarriedSlot::LeftHand {
                if !left_free {
                    return Err(InventoryError::HandsFull);
                }
                (true, false)
            } else if !right_free {
                return Err(InventoryError::HandsFull);
            } else {
                (false, true)
            }
        }
    };

    let mut player = objects.get(player_id).unwrap().clone();
    if use_left {
        player.set_object_ref_property("left_hand", Some(item_id.clone()));
    }
    if use_right {
        player.set_object_ref_property("right_hand", Some(item_id.clone()));
    }
    objects.insert(player_id.clone(), player);

    let mut item = objects.get(item_id).unwrap().clone();
    item.location = Some(player_id.clone());
    if use_left && use_right {
        item.set_carried_slot(Some(CarriedSlot::LeftHand));
        item.set_property_string("hand_slot", "both");
    } else if use_left {
        item.set_carried_slot(Some(CarriedSlot::LeftHand));
    } else {
        item.set_carried_slot(Some(CarriedSlot::RightHand));
    }
    objects.insert(item_id.clone(), item);

    Ok(())
}

fn clear_hand_refs(player: &mut Object, item_id: &ObjectId) {
    if player.left_hand_item().as_ref() == Some(item_id) {
        player.set_object_ref_property("left_hand", None);
    }
    if player.right_hand_item().as_ref() == Some(item_id) {
        player.set_object_ref_property("right_hand", None);
    }
}

fn remove_from_player(
    player_id: &ObjectId,
    item_id: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
) {
    let mut player = objects.get(player_id).unwrap().clone();
    player.remove_from_list_property("pockets", item_id);
    player.remove_from_list_property("worn", item_id);
    clear_hand_refs(&mut player, item_id);
    objects.insert(player_id.clone(), player);
}

pub fn take_item(
    ctx: &mut InventoryContext<'_>,
    item_name: &str,
) -> Result<String, InventoryError> {
    let room_id = ctx.room_id.ok_or(InventoryError::NoRoom)?.clone();
    let item_id =
        resolve_inventory_target(item_name, Some(&room_id), ctx.player_id, ctx.objects, false)?;

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
    let (left_free, right_free) = hands_free(&player, ctx.objects);

    let placement =
        if item.is_pocketable() && player.pockets().len() < player.pocket_capacity() as usize {
            CarriedSlot::Pocket
        } else if item.hand_slot().as_deref() == Some("both") && left_free && right_free {
            CarriedSlot::LeftHand
        } else if right_free {
            CarriedSlot::RightHand
        } else if left_free {
            CarriedSlot::LeftHand
        } else {
            return Err(InventoryError::HandsFull);
        };

    let item_name_display = item.name.clone();
    match placement {
        CarriedSlot::Pocket => place_in_pocket(ctx.player_id, &item_id, ctx.objects)?,
        slot => place_in_hand(ctx.player_id, &item_id, slot, ctx.objects)?,
    }

    Ok(format!("You take the {item_name_display}."))
}

pub fn drop_item(
    ctx: &mut InventoryContext<'_>,
    item_name: &str,
) -> Result<String, InventoryError> {
    let room_id = ctx.room_id.ok_or(InventoryError::NoRoom)?.clone();
    let item_id = resolve_inventory_target(item_name, None, ctx.player_id, ctx.objects, true)?;

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
    let item_id = resolve_inventory_target(item_name, None, ctx.player_id, ctx.objects, true)?;
    let container_id =
        resolve_inventory_target(container_name, None, ctx.player_id, ctx.objects, true)?;

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
        resolve_inventory_target(container_name, None, ctx.player_id, ctx.objects, true)?;

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
    if item.is_pocketable() && player.pockets().len() < player.pocket_capacity() as usize {
        place_in_pocket(ctx.player_id, &item_id, ctx.objects)?;
    } else {
        let (left_free, right_free) = hands_free(&player, ctx.objects);
        if right_free {
            place_in_hand(ctx.player_id, &item_id, CarriedSlot::RightHand, ctx.objects)?;
        } else if left_free {
            place_in_hand(ctx.player_id, &item_id, CarriedSlot::LeftHand, ctx.objects)?;
        } else {
            return Err(InventoryError::HandsFull);
        }
    }

    Ok(format!(
        "You remove the {item_display} from your {container_display}."
    ))
}

pub fn wield_item(
    ctx: &mut InventoryContext<'_>,
    item_name: &str,
) -> Result<String, InventoryError> {
    let item_id = resolve_inventory_target(item_name, None, ctx.player_id, ctx.objects, true)?;

    if !is_carried_by(ctx.player_id, &item_id, ctx.objects) {
        return Err(InventoryError::NotCarried);
    }

    let item = ctx.objects.get(&item_id).unwrap().clone();
    if item.is_container() && item.hand_slot().is_none() {
        return Err(InventoryError::NotWieldable);
    }

    remove_from_player(ctx.player_id, &item_id, ctx.objects);

    let hand_slot_value = item.hand_slot();
    let hand_slot = hand_slot_value.as_deref().unwrap_or("right");
    let slot = if hand_slot == "left" {
        CarriedSlot::LeftHand
    } else {
        CarriedSlot::RightHand
    };
    place_in_hand(ctx.player_id, &item_id, slot, ctx.objects)?;

    let item = ctx.objects.get(&item_id).unwrap();
    let display = item.name.clone();
    let (left, right) = (
        ctx.objects
            .get(ctx.player_id)
            .and_then(|p| p.left_hand_item()),
        ctx.objects
            .get(ctx.player_id)
            .and_then(|p| p.right_hand_item()),
    );
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
    let item_id =
        resolve_inventory_target(item_name, Some(&room_id), ctx.player_id, ctx.objects, false)?;

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

    let mut player = ctx.objects.get(ctx.player_id).unwrap().clone();
    player.add_to_list_property("worn", item_id.clone());
    ctx.objects.insert(ctx.player_id.clone(), player);

    let mut item = ctx.objects.get(&item_id).unwrap().clone();
    item.location = Some(ctx.player_id.clone());
    item.set_carried_slot(Some(CarriedSlot::Worn));
    let display = item.name.clone();
    ctx.objects.insert(item_id, item);

    Ok(format!("You wear the {display}."))
}

/// Natural-language summary of what a player is carrying (for look self).
pub fn describe_carried(player: &Object, objects: &HashMap<ObjectId, Object>) -> String {
    let mut parts = Vec::new();

    for id in player.worn_containers() {
        if let Some(obj) = objects.get(&id) {
            parts.push(format!("{} (worn)", obj.name));
        }
    }
    for id in player.pockets() {
        if let Some(obj) = objects.get(&id) {
            parts.push(format!("{} (in your pocket)", obj.name));
        }
    }
    if let Some(id) = player.left_hand_item() {
        if let Some(obj) = objects.get(&id) {
            let right = player.right_hand_item();
            if right.as_ref() == Some(&id) {
                parts.push(format!("{} (wielded)", obj.name));
            } else {
                parts.push(format!("{} (in your left hand)", obj.name));
            }
        }
    }
    if let Some(id) = player.right_hand_item() {
        if player.left_hand_item().as_ref() != Some(&id) {
            if let Some(obj) = objects.get(&id) {
                parts.push(format!("{} (in your right hand)", obj.name));
            }
        }
    }

    match parts.len() {
        0 => "You are empty-handed.".to_string(),
        1 => format!("You are carrying {}.", parts[0]),
        2 => format!("You are carrying {} and {}.", parts[0], parts[1]),
        _ => format!(
            "You are carrying {}.",
            parts.join(", ").replacen(", ", ", and ", 1)
        ),
    }
}

/// Full inventory listing for the `inventory` command.
pub fn describe_inventory(player: &Object, objects: &HashMap<ObjectId, Object>) -> String {
    let mut lines = vec!["You are carrying:".to_string()];

    let mut empty = true;

    if let Some(id) = player.left_hand_item() {
        if let Some(obj) = objects.get(&id) {
            empty = false;
            if player.right_hand_item().as_ref() == Some(&id) {
                lines.push(format!("  [wielded] {}", obj.name));
            } else {
                lines.push(format!("  [left hand] {}", obj.name));
            }
        }
    }
    if let Some(id) = player.right_hand_item() {
        if player.left_hand_item().as_ref() != Some(&id) {
            if let Some(obj) = objects.get(&id) {
                empty = false;
                lines.push(format!("  [right hand] {}", obj.name));
            }
        }
    }

    for id in &player.pockets() {
        if let Some(obj) = objects.get(id) {
            empty = false;
            lines.push(format!("  [pocket] {}", obj.name));
        }
    }

    for id in &player.worn_containers() {
        if let Some(container) = objects.get(id) {
            empty = false;
            lines.push(format!("  [worn] {}", container.name));
            for inner_id in container.container_contents() {
                if let Some(inner) = objects.get(&inner_id) {
                    lines.push(format!("    inside {}: {}", container.name, inner.name));
                }
            }
        }
    }

    // Containers held in hands
    for hand in [player.left_hand_item(), player.right_hand_item()]
        .into_iter()
        .flatten()
    {
        if let Some(container) = objects.get(&hand) {
            if container.is_container() && !player.worn_containers().contains(&hand) {
                for inner_id in container.container_contents() {
                    if let Some(inner) = objects.get(&inner_id) {
                        empty = false;
                        lines.push(format!("    inside {}: {}", container.name, inner.name));
                    }
                }
            }
        }
    }

    if empty {
        lines.push("  nothing".to_string());
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::object::{ObjectFactory, PermissionFlags};
    use crate::core::persistence::SqlitePersistence;

    async fn setup_world() -> (
        ObjectFactory<SqlitePersistence>,
        ObjectId,
        ObjectId,
        HashMap<ObjectId, Object>,
    ) {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence);
        let owner = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("room:test-001");

        let mut player = factory.create_player("hero", owner.clone()).await.unwrap();
        player.location = Some(room_id.clone());

        let mut room = factory.create("room", "test", owner.clone()).await.unwrap();
        room.name = "Test Room".to_string();

        let mut coin = factory.create_item("coin", owner.clone()).await.unwrap();
        coin.name = "Gold Coin".to_string();
        coin.location = Some(room_id.clone());

        let mut sword = factory.create_item("sword", owner.clone()).await.unwrap();
        sword.name = "Rusty Sword".to_string();
        sword.set_property_string("hand_slot", "right");
        sword.set_property_bool("is_pocketable", false);
        sword.location = Some(room_id.clone());

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
        objects.insert(backpack.id.clone(), backpack);

        (factory, owner, room_id, objects)
    }

    #[tokio::test]
    async fn take_pocketable_item() {
        let (_factory, player_id, room_id, mut objects) = setup_world().await;
        let coin_id = objects
            .values()
            .find(|o| o.name == "Gold Coin")
            .unwrap()
            .id
            .clone();

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
        };

        let msg = take_item(&mut ctx, "coin").unwrap();
        assert!(msg.contains("Gold Coin"));

        let player = objects.get(&player_id).unwrap();
        assert!(player.pockets().contains(&coin_id));
        assert_eq!(
            objects.get(&coin_id).unwrap().carried_slot(),
            Some(CarriedSlot::Pocket)
        );
    }

    #[tokio::test]
    async fn take_item_to_hand_when_not_pocketable() {
        let (_factory, player_id, room_id, mut objects) = setup_world().await;

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
        };

        take_item(&mut ctx, "sword").unwrap();
        let player = objects.get(&player_id).unwrap();
        assert_eq!(
            player
                .right_hand_item()
                .map(|id| objects.get(&id).unwrap().name.clone()),
            Some("Rusty Sword".to_string())
        );
    }

    #[tokio::test]
    async fn drop_item_to_room() {
        let (_factory, player_id, room_id, mut objects) = setup_world().await;

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
        };

        take_item(&mut ctx, "coin").unwrap();
        drop_item(&mut ctx, "coin").unwrap();

        let coin = objects.values().find(|o| o.name == "Gold Coin").unwrap();
        assert_eq!(coin.location, Some(room_id));
        assert!(objects.get(&player_id).unwrap().pockets().is_empty());
    }

    #[tokio::test]
    async fn put_and_remove_from_container() {
        let (_factory, player_id, room_id, mut objects) = setup_world().await;

        let backpack_id = objects
            .values()
            .find(|o| o.name == "Backpack")
            .unwrap()
            .id
            .clone();
        let coin_id = objects
            .values()
            .find(|o| o.name == "Gold Coin")
            .unwrap()
            .id
            .clone();

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
        };

        take_item(&mut ctx, "coin").unwrap();
        wear_item(&mut ctx, "backpack").unwrap();
        put_item(&mut ctx, "coin", "backpack").unwrap();

        assert!(ctx
            .objects
            .get(&backpack_id)
            .unwrap()
            .container_contents()
            .contains(&coin_id));

        remove_item(&mut ctx, "coin", "backpack").unwrap();
        let player = objects.get(&player_id).unwrap();
        assert!(player.pockets().contains(&coin_id));
    }

    #[tokio::test]
    async fn container_capacity_enforced() {
        let (_factory, player_id, room_id, mut objects) = setup_world().await;

        let mut tiny = objects
            .values()
            .find(|o| o.name == "Backpack")
            .unwrap()
            .clone();
        tiny.set_property_int("capacity", 0);
        objects.insert(tiny.id.clone(), tiny);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
        };

        take_item(&mut ctx, "coin").unwrap();
        wear_item(&mut ctx, "backpack").unwrap();
        let err = put_item(&mut ctx, "coin", "backpack").unwrap_err();
        assert_eq!(err, InventoryError::ContainerFull);
    }

    #[test]
    fn describe_carried_empty_and_full() {
        let owner = ObjectId::new("player:hero-001");
        let mut player = Object {
            id: owner.clone(),
            name: "Hero".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
        };
        player.init_inventory();

        assert_eq!(
            describe_carried(&player, &HashMap::new()),
            "You are empty-handed."
        );

        let coin_id = ObjectId::new("item:coin-001");
        let mut coin = Object {
            id: coin_id.clone(),
            name: "Gold Coin".to_string(),
            aliases: Vec::new(),
            location: Some(owner.clone()),
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
        };
        coin.set_carried_slot(Some(CarriedSlot::Pocket));
        player.add_to_list_property("pockets", coin_id.clone());

        let mut objects = HashMap::new();
        objects.insert(coin_id, coin);

        let desc = describe_carried(&player, &objects);
        assert!(desc.contains("Gold Coin"));
        assert!(desc.contains("pocket"));
    }
}
