use std::collections::HashMap;
use std::fmt;

use crate::display::{resolve_object, ResolveScope as LookupScope, TargetResolution};
use crate::mudl::{slot_display_name, AnatomyRegistry, BodyPlan, SlotType};
use crate::object::{LocationRef, Object, ObjectId};
use crate::world::move_manager::{
    move_object, move_to_container, move_to_room, MoveContext, MoveError, MoveHooks,
};

/// Errors returned by inventory operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InventoryError {
    NotFound(String),
    /// Item exists on the ground but must be carried for this command (put, drop, etc.).
    NotCarriedButOnGround(String),
    /// Item is already in the player's possession (take/get).
    AlreadyHolding(String),
    /// Container is on the ground but must be worn/carried (put in).
    ContainerNotCarriedButOnGround(String),
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
            Self::NotCarriedButOnGround(name) => write!(
                f,
                "You aren't holding any {name}, but there are {name} on the ground. Try: get {name}"
            ),
            Self::AlreadyHolding(name) => write!(f, "You're already holding {name}."),
            Self::ContainerNotCarriedButOnGround(name) => write!(
                f,
                "You aren't carrying the {name}, but it's here on the ground. Try: get {name}"
            ),
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

fn with_move_ctx<'a, 'b, F, T>(ctx: &'a mut InventoryContext<'b>, f: F) -> Result<T, InventoryError>
where
    F: FnOnce(&mut MoveContext<'a>) -> Result<T, MoveError>,
{
    let mut move_ctx = MoveContext {
        objects: ctx.objects,
        anatomy: Some(ctx.anatomy),
        hooks: MoveHooks::default(),
        dirty: None,
    };
    f(&mut move_ctx).map_err(Into::into)
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

fn resolve_scope_to_lookup(scope: ResolveScope) -> LookupScope {
    match scope {
        ResolveScope::Carried => LookupScope::PossessionOnly,
        ResolveScope::Ground => LookupScope::RoomOnly,
        ResolveScope::CarriedOrGround => LookupScope::PossessionOrRoom,
    }
}

fn resolve_inventory_target(
    name: &str,
    room_id: Option<&ObjectId>,
    player_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
    scope: ResolveScope,
) -> Result<ObjectId, InventoryError> {
    match resolve_object(
        name,
        player_id,
        room_id,
        objects,
        resolve_scope_to_lookup(scope),
    ) {
        TargetResolution::Found(id) => Ok(id),
        TargetResolution::Ambiguous(msg) => Err(InventoryError::InvalidTarget(msg)),
        TargetResolution::NotFound => {
            if name.to_lowercase() == "here" {
                Err(InventoryError::NoRoom)
            } else {
                Err(InventoryError::NotFound(name.to_string()))
            }
        }
    }
}

fn target_visible_on_ground(
    name: &str,
    room_id: &ObjectId,
    player_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> bool {
    matches!(
        resolve_object(
            name,
            player_id,
            Some(room_id),
            objects,
            LookupScope::RoomOnly,
        ),
        TargetResolution::Found(_) | TargetResolution::Ambiguous(_)
    )
}

fn target_carried_by_player(
    name: &str,
    player_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> bool {
    matches!(
        resolve_object(
            name,
            player_id,
            None,
            objects,
            LookupScope::PossessionOnly,
        ),
        TargetResolution::Found(_)
    )
}

fn resolve_carried_item(
    ctx: &InventoryContext<'_>,
    item_name: &str,
) -> Result<ObjectId, InventoryError> {
    match resolve_inventory_target(
        item_name,
        None,
        ctx.player_id,
        ctx.objects,
        ResolveScope::Carried,
    ) {
        Ok(id) => Ok(id),
        Err(InventoryError::NotFound(_)) => {
            if let Some(room_id) = ctx.room_id {
                if target_visible_on_ground(item_name, room_id, ctx.player_id, ctx.objects) {
                    return Err(InventoryError::NotCarriedButOnGround(item_name.to_string()));
                }
            }
            Err(InventoryError::NotFound(item_name.to_string()))
        }
        Err(e) => Err(e),
    }
}

fn resolve_carried_container(
    ctx: &InventoryContext<'_>,
    container_name: &str,
) -> Result<ObjectId, InventoryError> {
    match resolve_inventory_target(
        container_name,
        None,
        ctx.player_id,
        ctx.objects,
        ResolveScope::Carried,
    ) {
        Ok(id) => Ok(id),
        Err(InventoryError::NotFound(_)) => {
            if let Some(room_id) = ctx.room_id {
                if target_visible_on_ground(container_name, room_id, ctx.player_id, ctx.objects) {
                    return Err(InventoryError::ContainerNotCarriedButOnGround(
                        container_name.to_string(),
                    ));
                }
            }
            Err(InventoryError::NotFound(container_name.to_string()))
        }
        Err(e) => Err(e),
    }
}

fn resolve_ground_item(
    ctx: &InventoryContext<'_>,
    item_name: &str,
    room_id: &ObjectId,
) -> Result<ObjectId, InventoryError> {
    match resolve_inventory_target(
        item_name,
        Some(room_id),
        ctx.player_id,
        ctx.objects,
        ResolveScope::Ground,
    ) {
        Ok(id) => Ok(id),
        Err(InventoryError::NotFound(_)) => {
            if target_carried_by_player(item_name, ctx.player_id, ctx.objects) {
                return Err(InventoryError::AlreadyHolding(item_name.to_string()));
            }
            Err(InventoryError::NotFound(item_name.to_string()))
        }
        Err(e) => Err(e),
    }
}

pub fn is_carried_by(
    player_id: &ObjectId,
    item_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> bool {
    crate::display::is_in_player_possession(player_id, item_id, objects)
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
    let item_id = resolve_ground_item(ctx, item_name, &room_id)?;

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

    let player_id = ctx.player_id.clone();
    with_move_ctx(ctx, |mctx| {
        move_object(
            mctx,
            &item_id,
            LocationRef::Room(room_id),
            LocationRef::Inventory(player_id),
        )
    })?;

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

    with_move_ctx(ctx, |mctx| move_to_room(mctx, &item_id, &room_id))?;

    Ok(format!("You drop the {item_name_display}."))
}

/// Parsed `put [count] <item> in <container>` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PutRequest {
    pub quantity: Option<u32>,
    pub item_name: String,
    pub container_name: String,
}

/// Parse `put 10 coins in purse` or `put coins in purse`.
pub fn parse_put_args(rest: &str) -> Result<PutRequest, InventoryError> {
    let (left, container_name) = rest.split_once(" in ").ok_or_else(|| {
        InventoryError::InvalidTarget("Usage: put [count] <item> in <container>".into())
    })?;
    let left = left.trim();
    let container_name = container_name.trim().to_string();
    if left.is_empty() || container_name.is_empty() {
        return Err(InventoryError::InvalidTarget(
            "Usage: put [count] <item> in <container>".into(),
        ));
    }

    let tokens: Vec<&str> = left.split_whitespace().collect();
    if tokens.len() >= 2 {
        if let Ok(qty) = tokens[0].parse::<u32>() {
            if qty == 0 {
                return Err(InventoryError::InvalidTarget(
                    "You must put at least one.".into(),
                ));
            }
            return Ok(PutRequest {
                quantity: Some(qty),
                item_name: tokens[1..].join(" "),
                container_name,
            });
        }
    }

    Ok(PutRequest {
        quantity: None,
        item_name: left.to_string(),
        container_name,
    })
}

/// Build player feedback after a put operation.
pub fn format_put_message(
    item_display: &str,
    container_display: &str,
    transferred: u32,
    total_held: u32,
    quantity: Option<u32>,
) -> String {
    let remainder_in_hand = total_held.saturating_sub(transferred);

    let base = if transferred == 1 && quantity.is_none() && total_held == 1 {
        format!("You put the {item_display} in your {container_display}.")
    } else {
        format!("You put {transferred} {item_display} in your {container_display}.")
    };

    if let Some(req) = quantity {
        if transferred < req {
            return format!("{base} {} won't fit.", req.saturating_sub(transferred));
        }
        base
    } else if remainder_in_hand > 0 {
        format!("{base} {remainder_in_hand} won't fit.")
    } else {
        base
    }
}

pub fn put_item(
    ctx: &mut InventoryContext<'_>,
    item_name: &str,
    container_name: &str,
    quantity: Option<u32>,
) -> Result<String, InventoryError> {
    let item_id = resolve_carried_item(ctx, item_name)?;
    let container_id = resolve_carried_container(ctx, container_name)?;

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

    let item = ctx.objects.get(&item_id).unwrap().clone();
    let item_display = item.name.clone();
    let container_display = container.name.clone();
    let total_count = if item.is_stackable() {
        item.stack_count()
    } else {
        1
    };

    let result = with_move_ctx(ctx, |mctx| {
        move_to_container(mctx, &item_id, &container_id, quantity)
    })?;

    let transferred = result.units_transferred.unwrap_or(total_count);
    Ok(format_put_message(
        &item_display,
        &container_display,
        transferred,
        total_count,
        quantity,
    ))
}

pub fn remove_item(
    ctx: &mut InventoryContext<'_>,
    item_name: &str,
    container_name: &str,
) -> Result<String, InventoryError> {
    let container_id = resolve_inventory_target(
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
    let mut matches: Vec<ObjectId> = container
        .container_contents()
        .into_iter()
        .filter(|id| {
            ctx.objects
                .get(id)
                .is_some_and(|obj| obj.is_active() && crate::display::name_matches(&needle, obj))
        })
        .collect();

    let item_id = match matches.len() {
        0 => return Err(InventoryError::NotFound(item_name.to_string())),
        1 => matches.remove(0),
        _ => {
            let resolved: Vec<_> = matches
                .into_iter()
                .map(|id| crate::display::ResolvedMatch {
                    id,
                    location_hint: Some(format!("in {}", container.name.to_lowercase())),
                })
                .collect();
            return Err(InventoryError::InvalidTarget(
                crate::display::format_disambiguation(item_name, &resolved),
            ));
        }
    };

    let item = ctx.objects.get(&item_id).unwrap().clone();
    let item_display = item.name.clone();
    let container_display = container.name.clone();

    let player_id = ctx.player_id.clone();
    with_move_ctx(ctx, |mctx| {
        move_object(
            mctx,
            &item_id,
            LocationRef::Container(container_id.clone(), None),
            LocationRef::Inventory(player_id),
        )
    })?;

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

    let player = ctx.objects.get(ctx.player_id).unwrap().clone();
    let plan = player_body_plan(&player, ctx.anatomy)?;
    let target_slot = item
        .wear_slot()
        .or_else(|| plan.wear_slots().first().map(|s| s.name.clone()))
        .ok_or(InventoryError::NotWearable)?;

    if plan.slot(&target_slot).is_none() {
        return Err(InventoryError::NotWearable);
    }

    let src = if is_carried_by(ctx.player_id, &item_id, ctx.objects) {
        crate::world::move_manager::resolve_location(&item_id, ctx.objects)
            .ok_or(InventoryError::NotCarried)?
    } else if item.location.as_ref() == Some(&room_id) {
        LocationRef::Room(room_id)
    } else {
        return Err(InventoryError::NotInRoom);
    };

    let player_id = ctx.player_id.clone();
    let display = item.name.clone();
    with_move_ctx(ctx, |mctx| {
        move_object(
            mctx,
            &item_id,
            src,
            LocationRef::BodySlot(player_id, target_slot),
        )
    })?;

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
                        item_name: crate::display::format_stackable_label(obj),
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
                    return Some(format!(
                        "You are wielding {} with both hands.",
                        crate::display::format_stackable_label(obj)
                    ));
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
                let label = crate::display::format_stackable_label(obj);
                entries.push(format!("  {label} — {placement}"));

                if obj.is_container() {
                    for inner_id in obj.container_contents() {
                        if let Some(inner) = objects.get(&inner_id) {
                            let label = crate::display::format_stackable_label(inner);
                            entries.push(format!("    {label} — inside your {}", obj.name));
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
        assert_eq!(
            player.get_int_property("max_weight"),
            Some(crate::object::DEFAULT_PLAYER_MAX_WEIGHT)
        );
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
        match err {
            InventoryError::InvalidTarget(msg) => {
                assert!(msg.contains("Which sword do you mean?"));
                assert!(msg.contains("sword-"));
            }
            other => panic!("expected InvalidTarget, got {other:?}"),
        }
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
        put_item(&mut ctx, "coin", "backpack", None).unwrap();

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

    #[tokio::test]
    async fn put_coins_in_purse_partial_by_weight() {
        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut purse = factory
            .create_container_with_spec(
                "purse",
                player_id.clone(),
                crate::object::ContainerSpec {
                    capacity: 3,
                    max_weight: Some(10),
                    max_volume: None,
                    wearable: true,
                    wear_slot: Some("torso".to_string()),
                },
                None,
            )
            .await
            .unwrap();
        purse.name = "purse".to_string();
        purse.location = Some(room_id.clone());

        let mut coins = factory
            .create_stackable_item("coins", player_id.clone(), None, 20)
            .await
            .unwrap();
        coins.set_property_int("weight", 1);
        coins.set_property_int("volume", 1);
        coins.location = Some(player_id.clone());

        let purse_id = purse.id.clone();
        let coins_id = coins.id.clone();

        let mut player = objects.get(&player_id).unwrap().clone();
        player.set_body_slot("torso", Some(purse_id.clone()));
        player.set_body_slot("right_hand", Some(coins_id.clone()));
        objects.insert(player_id.clone(), player);
        objects.insert(purse_id.clone(), purse);
        objects.insert(coins_id.clone(), coins);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
        };

        let msg = put_item(&mut ctx, "coins", "purse", None).unwrap();
        assert!(msg.contains("10"));
        assert!(msg.contains("won't fit"));
        assert!(msg.contains("purse"));

        let purse = objects.get(&purse_id).unwrap();
        assert_eq!(purse.container_contents().len(), 1);
        let stored = objects.get(&purse.container_contents()[0]).unwrap();
        assert_eq!(stored.stack_count(), 10);

        let held = objects.get(&coins_id).unwrap();
        assert_eq!(held.stack_count(), 10);
    }

    #[test]
    fn parse_put_args_with_quantity() {
        let req = parse_put_args("10 coins in purse").unwrap();
        assert_eq!(req.quantity, Some(10));
        assert_eq!(req.item_name, "coins");
        assert_eq!(req.container_name, "purse");
    }

    #[test]
    fn parse_put_args_without_quantity() {
        let req = parse_put_args("coins in purse").unwrap();
        assert_eq!(req.quantity, None);
        assert_eq!(req.item_name, "coins");
    }

    #[test]
    fn format_put_message_partial_auto() {
        let msg = format_put_message("coins", "purse", 15, 20, None);
        assert_eq!(msg, "You put 15 coins in your purse. 5 won't fit.");
    }

    #[test]
    fn format_put_message_exact_quantity() {
        let msg = format_put_message("coins", "purse", 10, 20, Some(10));
        assert_eq!(msg, "You put 10 coins in your purse.");
    }

    #[tokio::test]
    async fn put_item_on_ground_hints_to_get_first() {
        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut backpack = factory
            .create_container("backpack", player_id.clone(), 5, true)
            .await
            .unwrap();
        backpack.name = "backpack".to_string();
        let backpack_id = backpack.id.clone();

        let mut player = objects.get(&player_id).unwrap().clone();
        player.set_body_slot("torso", Some(backpack_id.clone()));
        objects.insert(player_id.clone(), player);
        objects.insert(backpack_id, backpack);

        let mut coins = factory
            .create_stackable_item("coins", player_id.clone(), None, 10)
            .await
            .unwrap();
        coins.name = "coins".to_string();
        coins.location = Some(room_id.clone());
        objects.insert(coins.id.clone(), coins);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
        };

        let err = put_item(&mut ctx, "coins", "backpack", None).unwrap_err();
        assert_eq!(
            err,
            InventoryError::NotCarriedButOnGround("coins".to_string())
        );
        assert!(err.to_string().contains("on the ground"));
        assert!(err.to_string().contains("get coins"));
    }

    #[tokio::test]
    async fn put_item_container_on_ground_hints_to_get_first() {
        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut coins = factory
            .create_stackable_item("coins", player_id.clone(), None, 5)
            .await
            .unwrap();
        coins.name = "coins".to_string();
        let coins_id = coins.id.clone();

        let mut player = objects.get(&player_id).unwrap().clone();
        player.set_body_slot("right_hand", Some(coins_id.clone()));
        objects.insert(player_id.clone(), player);
        objects.insert(coins_id, coins);

        let mut backpack = factory
            .create_container("backpack", player_id.clone(), 5, true)
            .await
            .unwrap();
        backpack.name = "backpack".to_string();
        backpack.location = Some(room_id.clone());
        objects.insert(backpack.id.clone(), backpack);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
        };

        let err = put_item(&mut ctx, "coins", "backpack", None).unwrap_err();
        assert_eq!(
            err,
            InventoryError::ContainerNotCarriedButOnGround("backpack".to_string())
        );
        assert!(err.to_string().contains("get backpack"));
    }

    #[tokio::test]
    async fn take_item_already_held_hints_instead_of_not_found() {
        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut coins = factory
            .create_stackable_item("coins", player_id.clone(), None, 10)
            .await
            .unwrap();
        coins.name = "coins".to_string();
        let coins_id = coins.id.clone();

        let mut player = objects.get(&player_id).unwrap().clone();
        player.set_body_slot("right_hand", Some(coins_id.clone()));
        objects.insert(player_id.clone(), player);
        objects.insert(coins_id, coins);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
        };

        let err = take_item(&mut ctx, "coins").unwrap_err();
        assert_eq!(err, InventoryError::AlreadyHolding("coins".to_string()));
        assert!(err.to_string().contains("already holding"));
    }

    #[tokio::test]
    async fn put_specified_quantity_in_purse() {
        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut purse = factory
            .create_container_with_spec(
                "purse",
                player_id.clone(),
                crate::object::ContainerSpec {
                    capacity: 3,
                    max_weight: Some(10),
                    max_volume: None,
                    wearable: true,
                    wear_slot: Some("torso".to_string()),
                },
                None,
            )
            .await
            .unwrap();
        purse.name = "purse".to_string();

        let mut coins = factory
            .create_stackable_item("coins", player_id.clone(), None, 20)
            .await
            .unwrap();
        coins.set_property_int("weight", 1);
        coins.location = Some(player_id.clone());

        let purse_id = purse.id.clone();
        let coins_id = coins.id.clone();

        let mut player = objects.get(&player_id).unwrap().clone();
        player.set_body_slot("torso", Some(purse_id.clone()));
        player.set_body_slot("right_hand", Some(coins_id.clone()));
        objects.insert(player_id.clone(), player);
        objects.insert(purse_id.clone(), purse);
        objects.insert(coins_id.clone(), coins);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
        };

        let msg = put_item(&mut ctx, "coins", "purse", Some(10)).unwrap();
        assert_eq!(msg, "You put 10 coins in your purse.");

        let held = objects.get(&coins_id).unwrap();
        assert_eq!(held.stack_count(), 10);
        let purse = objects.get(&purse_id).unwrap();
        let stored = objects.get(&purse.container_contents()[0]).unwrap();
        assert_eq!(stored.stack_count(), 10);
    }

    #[tokio::test]
    async fn look_self_shows_stackable_quantity_in_hand() {
        let (factory, anatomy, player_id, _room_id, mut objects) = setup_world().await;

        let mut coins = factory
            .create_stackable_item("coins", player_id.clone(), None, 20)
            .await
            .unwrap();
        coins.location = Some(player_id.clone());

        let coins_id = coins.id.clone();
        let mut player = objects.get(&player_id).unwrap().clone();
        player.set_body_slot("right_hand", Some(coins_id.clone()));
        objects.insert(player_id.clone(), player);
        objects.insert(coins_id, coins);

        let player = objects.get(&player_id).unwrap();
        let carried = describe_carried(player, &objects, &anatomy);
        assert!(carried.contains("20 coins"));
        assert!(carried.contains("right hand"));

        let inv = describe_inventory(player, &objects, &anatomy);
        assert!(inv.contains("20 coins — in your right hand"));
    }

    #[tokio::test]
    async fn look_purse_shows_stackable_contents() {
        use crate::display::{Describable, DisplayContext, DisplayMode};

        let (factory, anatomy, player_id, _room_id, mut objects) = setup_world().await;

        let mut purse = factory
            .create_container_with_spec(
                "purse",
                player_id.clone(),
                crate::object::ContainerSpec {
                    capacity: 3,
                    max_weight: Some(10),
                    max_volume: None,
                    wearable: false,
                    wear_slot: None,
                },
                None,
            )
            .await
            .unwrap();
        purse.name = "purse".to_string();

        let mut coins = factory
            .create_stackable_item("coins", player_id.clone(), None, 20)
            .await
            .unwrap();
        coins.location = Some(purse.id.clone());
        purse.set_property_list("contents", vec![coins.id.clone()]);

        objects.insert(purse.id.clone(), purse.clone());
        objects.insert(coins.id.clone(), coins);

        let ctx = DisplayContext::new(player_id.clone(), DisplayMode::Player)
            .with_objects(objects)
            .with_anatomy(anatomy);

        let output = purse.describe(&ctx);
        assert!(output.contains("The purse contains 20 coins"));
    }
}
