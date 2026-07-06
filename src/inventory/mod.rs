use std::collections::HashMap;
use std::fmt;

use crate::display::{
    display_name_for_single_unit, format_stack_transfer_message, item_lookup_variants,
    name_looks_plural, resolve_object, stack_quantity_phrase, ResolveScope as LookupScope,
    StackRemainderLocation, TargetResolution,
};
use crate::display::grammar::indefinite_article;
use crate::mudl::{slot_display_name, AnatomyRegistry, BodyPlan, SlotType};
use crate::object::{LocationRef, Object, ObjectId};
use crate::world::move_manager::{
    move_object, move_to_container, move_to_grasp, move_to_room, MoveContext, MoveError, MoveHooks,
};
use crate::world::possession::{
    grasp_action_phrase, is_carried_by as possession_is_carried_by, PossessionError,
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
    ContainerClosed(String),
    NotContainer,
    NotWearable,
    NotWieldable,
    AlreadyCarrying,
    ContainerNotCarried,
    NoRoom,
    NoBodyPlan,
    TooHeavy(String),
    InvalidTarget(String),
    /// Object has no readable text.
    NotReadable(String),
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
            Self::ContainerClosed(name) => write!(f, "The {name} is closed."),
            Self::NotContainer => write!(f, "That isn't a container."),
            Self::NotWearable => write!(f, "You can't wear that."),
            Self::NotWieldable => write!(f, "You can't wield that."),
            Self::AlreadyCarrying => write!(f, "You're already carrying that."),
            Self::ContainerNotCarried => write!(f, "You aren't carrying that container."),
            Self::NoRoom => write!(f, "You aren't anywhere."),
            Self::NoBodyPlan => write!(f, "You have no body plan."),
            Self::TooHeavy(name) => write!(f, "The {name} is too heavy for you to carry."),
            Self::InvalidTarget(msg) => write!(f, "{msg}"),
            Self::NotReadable(name) => {
                write!(f, "There's nothing to read on the {name}.")
            }
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
    /// When set, move operations mark touched object IDs for incremental persist.
    pub dirty: Option<&'a mut crate::world::DirtyTracker>,
}

fn with_move_ctx<'a, 'b, F, T>(ctx: &'a mut InventoryContext<'b>, f: F) -> Result<T, InventoryError>
where
    F: FnOnce(&mut MoveContext<'a>) -> Result<T, MoveError>,
{
    let mut move_ctx = MoveContext {
        objects: ctx.objects,
        anatomy: Some(ctx.anatomy),
        hooks: MoveHooks::default(),
        dirty: ctx.dirty.as_deref_mut(),
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
    if !target_visible_on_ground(item_name, room_id, ctx.player_id, ctx.objects)
        && target_carried_by_player(item_name, ctx.player_id, ctx.objects)
    {
        return Err(InventoryError::AlreadyHolding(item_name.to_string()));
    }

    resolve_item_with_variants(
        item_name,
        |name| {
            resolve_inventory_target(
                name,
                Some(room_id),
                ctx.player_id,
                ctx.objects,
                ResolveScope::Ground,
            )
        },
        || {
            if target_carried_by_player(item_name, ctx.player_id, ctx.objects) {
                Err(InventoryError::AlreadyHolding(item_name.to_string()))
            } else {
                Err(InventoryError::NotFound(item_name.to_string()))
            }
        },
    )
}

fn resolve_carried_item_with_variants(
    ctx: &InventoryContext<'_>,
    item_name: &str,
) -> Result<ObjectId, InventoryError> {
    resolve_item_with_variants(
        item_name,
        |name| resolve_carried_item(ctx, name),
        || Err(InventoryError::NotFound(item_name.to_string())),
    )
}

fn resolve_item_with_variants<F, G>(
    item_name: &str,
    mut resolve_one: F,
    mut on_not_found: G,
) -> Result<ObjectId, InventoryError>
where
    F: FnMut(&str) -> Result<ObjectId, InventoryError>,
    G: FnMut() -> Result<ObjectId, InventoryError>,
{
    let variants = item_lookup_variants(item_name);
    for name in &variants {
        match resolve_one(name) {
            Ok(id) => return Ok(id),
            Err(InventoryError::AlreadyHolding(_)) => return Err(InventoryError::AlreadyHolding(
                item_name.to_string(),
            )),
            Err(InventoryError::NotCarriedButOnGround(n)) => {
                return Err(InventoryError::NotCarriedButOnGround(n));
            }
            Err(InventoryError::ContainerNotCarriedButOnGround(n)) => {
                return Err(InventoryError::ContainerNotCarriedButOnGround(n));
            }
            Err(InventoryError::NotFound(_)) => {}
            Err(e) => return Err(e),
        }
    }
    on_not_found()
}

/// Parsed `take|drop [count] <item>` quantity semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemQuantityRequest {
    /// `None` = entire stack; `Some(n)` = explicit count (singular defaults to 1).
    pub quantity: Option<u32>,
    pub item_name: String,
}

/// Parse `take 5 gold bar`, `take gold bars`, or `take gold bar` (singular → 1).
pub fn parse_item_quantity_args(rest: &str) -> Result<ItemQuantityRequest, InventoryError> {
    let rest = rest.trim();
    if rest.is_empty() {
        return Err(InventoryError::InvalidTarget(
            "You must name an item.".into(),
        ));
    }

    let tokens: Vec<&str> = rest.split_whitespace().collect();
    if tokens.len() >= 2 {
        if let Ok(qty) = tokens[0].parse::<u32>() {
            if qty == 0 {
                return Err(InventoryError::InvalidTarget(
                    "You must move at least one.".into(),
                ));
            }
            return Ok(ItemQuantityRequest {
                quantity: Some(qty),
                item_name: tokens[1..].join(" "),
            });
        }
    }

    let item_name = rest.to_string();
    let quantity = if name_looks_plural(&item_name) {
        None
    } else {
        Some(1)
    };
    Ok(ItemQuantityRequest {
        quantity,
        item_name,
    })
}

/// Units to transfer for a stackable or single item.
pub fn effective_transfer_units(item: &Object, req: &ItemQuantityRequest) -> u32 {
    if !item.is_stackable() {
        return 1;
    }
    let available = item.stack_count();
    match req.quantity {
        None => available,
        Some(n) => n.min(available),
    }
}

fn move_units_for_request(item: &Object, req: &ItemQuantityRequest) -> Option<u32> {
    if !item.is_stackable() {
        return None;
    }
    let units = effective_transfer_units(item, req);
    if units >= item.stack_count() {
        None
    } else {
        Some(units)
    }
}

pub fn is_carried_by(
    player_id: &ObjectId,
    item_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> bool {
    possession_is_carried_by(player_id, item_id, objects)
}

impl From<PossessionError> for InventoryError {
    fn from(err: PossessionError) -> Self {
        match err {
            PossessionError::HandsFull => Self::HandsFull,
            PossessionError::NotCarried => Self::NotCarried,
            PossessionError::NotFound(name) => Self::NotFound(name),
        }
    }
}

pub fn take_item(
    ctx: &mut InventoryContext<'_>,
    args: &str,
) -> Result<String, InventoryError> {
    let req = parse_item_quantity_args(args)?;
    let room_id = ctx.room_id.ok_or(InventoryError::NoRoom)?.clone();
    let item_id = resolve_ground_item(ctx, &req.item_name, &room_id)?;

    let item = ctx
        .objects
        .get(&item_id)
        .ok_or_else(|| InventoryError::NotFound(req.item_name.clone()))?
        .clone();

    if !crate::display::resolve::is_accessible_in_room(&item_id, &room_id, ctx.player_id, ctx.objects)
    {
        return Err(InventoryError::NotInRoom);
    }
    if is_carried_by(ctx.player_id, &item_id, ctx.objects) {
        return Err(InventoryError::AlreadyCarrying);
    }

    let units = effective_transfer_units(&item, &req);
    let move_units = move_units_for_request(&item, &req);

    let player_id = ctx.player_id.clone();
    let src = crate::world::move_manager::resolve_location(&item_id, ctx.objects)
        .filter(|loc| match loc {
            LocationRef::Room(r) => r == &room_id,
            LocationRef::Container(_, _) => {
                crate::display::resolve::is_accessible_in_room(
                    &item_id,
                    &room_id,
                    ctx.player_id,
                    ctx.objects,
                )
            }
            _ => false,
        })
        .ok_or(InventoryError::NotInRoom)?;
    let result = with_move_ctx(ctx, |mctx| {
        move_object(
            mctx,
            &item_id,
            src,
            LocationRef::Inventory(player_id),
            move_units,
        )
    })?;

    let transferred = result.units_transferred.unwrap_or(units);
    let remainder_on_ground = ctx
        .objects
        .get(&item_id)
        .filter(|o| o.location.as_ref() == Some(&room_id))
        .map(|o| o.stack_count())
        .unwrap_or(0);
    let remainder = if transferred < units && remainder_on_ground > 0 {
        Some((remainder_on_ground, StackRemainderLocation::OnGround))
    } else {
        None
    };
    Ok(format_stack_transfer_message(
        "pick up",
        &item,
        transferred,
        remainder,
    ))
}

pub fn drop_item(
    ctx: &mut InventoryContext<'_>,
    args: &str,
) -> Result<String, InventoryError> {
    let req = parse_item_quantity_args(args)?;
    let room_id = ctx.room_id.ok_or(InventoryError::NoRoom)?.clone();
    let item_id = resolve_carried_item_with_variants(ctx, &req.item_name)?;

    if !is_carried_by(ctx.player_id, &item_id, ctx.objects) {
        return Err(InventoryError::NotCarried);
    }

    let item = ctx.objects.get(&item_id).unwrap().clone();
    let units = effective_transfer_units(&item, &req);
    let move_units = move_units_for_request(&item, &req);

    if item.is_container() && move_units.is_none() {
        for contained in item.container_contents() {
            if let Some(mut inner) = ctx.objects.get(&contained).cloned() {
                inner.location = Some(room_id.clone());
                inner.set_carried_slot(None);
                ctx.objects.insert(contained, inner);
            }
        }
    }

    let result = with_move_ctx(ctx, |mctx| move_to_room(mctx, &item_id, &room_id, move_units))?;
    let transferred = result.units_transferred.unwrap_or(units);
    let remainder_in_hand = ctx
        .objects
        .get(&item_id)
        .filter(|_| is_carried_by(ctx.player_id, &item_id, ctx.objects))
        .map(|o| o.stack_count())
        .unwrap_or(0);
    let remainder = if transferred < units && remainder_in_hand > 0 {
        Some((remainder_in_hand, StackRemainderLocation::InHand))
    } else {
        None
    };
    Ok(format_stack_transfer_message(
        "drop",
        &item,
        transferred,
        remainder,
    ))
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
    let container_name = container_name.trim().to_string();
    if left.trim().is_empty() || container_name.is_empty() {
        return Err(InventoryError::InvalidTarget(
            "Usage: put [count] <item> in <container>".into(),
        ));
    }

    let item_req = parse_item_quantity_args(left.trim())?;
    Ok(PutRequest {
        quantity: item_req.quantity,
        item_name: item_req.item_name,
        container_name,
    })
}

/// Build player feedback after a put operation.
pub fn format_put_message(
    item: &Object,
    container_display: &str,
    transferred: u32,
    total_held: u32,
    quantity: Option<u32>,
) -> String {
    let remainder_in_hand = total_held.saturating_sub(transferred);

    let mut snap = item.clone();
    snap.set_stack_count(transferred);
    let item_label = if transferred == 1 {
        display_name_for_single_unit(&item.name)
    } else {
        stack_quantity_phrase(&snap)
    };

    let base = if transferred == 1 && quantity.is_none() && total_held == 1 {
        format!(
            "You put {} {item_label} in your {container_display}",
            indefinite_article(&item_label)
        )
    } else {
        format!("You put {item_label} in your {container_display}")
    };

    if let Some(req) = quantity {
        if transferred < req {
            return format!(
                "{base}, {}.",
                format_remainder_in_hand_clause(item, req.saturating_sub(transferred))
            );
        }
        format!("{base}.")
    } else if remainder_in_hand > 0 {
        format!(
            "{base}, {}.",
            format_remainder_in_hand_clause(item, remainder_in_hand)
        )
    } else {
        format!("{base}.")
    }
}

fn format_remainder_in_hand_clause(item: &Object, count: u32) -> String {
    if count == 1 {
        let label = display_name_for_single_unit(&item.name);
        format!(
            "but {} remains in your hand",
            indefinite_article(&label)
        )
    } else {
        let mut snap = item.clone();
        snap.set_stack_count(count);
        format!(
            "but {} remain in your hand",
            stack_quantity_phrase(&snap)
        )
    }
}

pub fn put_item(
    ctx: &mut InventoryContext<'_>,
    item_name: &str,
    container_name: &str,
    quantity: Option<u32>,
) -> Result<String, InventoryError> {
    let item_id = resolve_carried_item_with_variants(ctx, item_name)?;
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
    if !container.container_is_open() {
        return Err(InventoryError::ContainerClosed(container.name.clone()));
    }

    let item = ctx.objects.get(&item_id).unwrap().clone();
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
        &item,
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
    if !container.container_is_open() {
        return Err(InventoryError::ContainerClosed(container.name.clone()));
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
            None,
        )
    })?;

    Ok(format!(
        "You remove the {item_display} from your {container_display}."
    ))
}

fn resolve_room_or_carried_container(
    ctx: &InventoryContext<'_>,
    container_name: &str,
) -> Result<ObjectId, InventoryError> {
    let room_id = ctx.room_id.cloned();
    resolve_inventory_target(
        container_name,
        room_id.as_ref(),
        ctx.player_id,
        ctx.objects,
        ResolveScope::CarriedOrGround,
    )
}

/// Open a container on the ground or in your possession.
pub fn open_container(
    ctx: &mut InventoryContext<'_>,
    container_name: &str,
) -> Result<String, InventoryError> {
    let container_id = resolve_room_or_carried_container(ctx, container_name)?;
    let container = ctx
        .objects
        .get(&container_id)
        .ok_or_else(|| InventoryError::NotFound(container_name.to_string()))?
        .clone();

    if !container.is_container() {
        return Err(InventoryError::NotContainer);
    }

    let display = container.name.to_lowercase();
    if container.container_is_open() {
        return Ok(format!("The {display} is already open."));
    }

    let mut container = container;
    container.set_container_open(true);
    ctx.objects.insert(container_id, container);

    Ok(format!("You open the {display}."))
}

/// Close a container on the ground or in your possession.
pub fn close_container(
    ctx: &mut InventoryContext<'_>,
    container_name: &str,
) -> Result<String, InventoryError> {
    let container_id = resolve_room_or_carried_container(ctx, container_name)?;
    let container = ctx
        .objects
        .get(&container_id)
        .ok_or_else(|| InventoryError::NotFound(container_name.to_string()))?
        .clone();

    if !container.is_container() {
        return Err(InventoryError::NotContainer);
    }

    let display = container.name.to_lowercase();
    if !container.container_is_open() {
        return Ok(format!("The {display} is already closed."));
    }

    let mut container = container;
    container.set_container_open(false);
    ctx.objects.insert(container_id, container);

    Ok(format!("You close the {display}."))
}

/// Read text from an object in the room or in your possession.
pub fn read_item(ctx: &InventoryContext<'_>, item_name: &str) -> Result<String, InventoryError> {
    let room_id = ctx.room_id.cloned();
    let item_id = resolve_inventory_target(
        item_name,
        room_id.as_ref(),
        ctx.player_id,
        ctx.objects,
        ResolveScope::CarriedOrGround,
    )?;

    let item = ctx
        .objects
        .get(&item_id)
        .ok_or_else(|| InventoryError::NotFound(item_name.to_string()))?
        .clone();

    if let Some(room) = ctx.room_id {
        if !is_carried_by(ctx.player_id, &item_id, ctx.objects)
            && !crate::display::resolve::is_accessible_in_room(
                &item_id,
                room,
                ctx.player_id,
                ctx.objects,
            )
        {
            return Err(InventoryError::NotFound(item_name.to_string()));
        }
    }

    if !item.is_readable() {
        return Err(InventoryError::NotReadable(item.name.to_lowercase()));
    }

    crate::display::format_read_message(&item)
        .ok_or_else(|| InventoryError::NotReadable(item.name.to_lowercase()))
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

    let display = item.name.clone();
    let player_id = ctx.player_id.clone();
    with_move_ctx(ctx, |mctx| move_to_grasp(mctx, &item_id, &player_id, None))?;

    let item = ctx.objects.get(&item_id).unwrap();
    let player = ctx.objects.get(ctx.player_id).unwrap();
    let plan = player_body_plan(player, ctx.anatomy)?;
    let phrase = grasp_action_phrase(item, player, &item_id, plan);

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
            None,
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
                let mut line = format!("  {label} — {placement}", label = crate::display::format_stackable_label(obj));
                if obj.is_container() && !obj.container_is_open() {
                    line.push_str(" (closed)");
                }
                entries.push(line);

                if obj.is_container() && obj.container_is_open() {
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
    async fn take_rejects_item_heavier_than_max_weight() {
        let (_factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut boulder = Object {
            id: ObjectId::new("item:boulder-001"),
            name: "boulder".to_string(),
            aliases: Vec::new(),
            location: Some(room_id.clone()),
            prototype: None,
            owner: player_id.clone(),
            permissions: crate::object::PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        boulder.set_property_int("weight", 200);
        objects.insert(boulder.id.clone(), boulder.clone());

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        let err = take_item(&mut ctx, "boulder").unwrap_err();
        assert_eq!(err, InventoryError::TooHeavy("boulder".to_string()));
        assert_eq!(
            objects.get(&boulder.id).unwrap().location.as_ref(),
            Some(&room_id)
        );
    }

    #[tokio::test]
    async fn take_rejects_when_carrying_would_exceed_max_weight() {
        let (_factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut heavy = Object {
            id: ObjectId::new("item:heavy-001"),
            name: "iron ingot".to_string(),
            aliases: Vec::new(),
            location: Some(room_id.clone()),
            prototype: None,
            owner: player_id.clone(),
            permissions: crate::object::PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        heavy.set_property_int("weight", 60);

        let mut boulder = Object {
            id: ObjectId::new("item:boulder-001"),
            name: "boulder".to_string(),
            aliases: Vec::new(),
            location: Some(room_id.clone()),
            prototype: None,
            owner: player_id.clone(),
            permissions: crate::object::PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        boulder.set_property_int("weight", 50);
        objects.insert(heavy.id.clone(), heavy);
        objects.insert(boulder.id.clone(), boulder);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        take_item(&mut ctx, "iron ingot").unwrap();
        let err = take_item(&mut ctx, "boulder").unwrap_err();
        assert_eq!(err, InventoryError::TooHeavy("boulder".to_string()));
    }

    #[tokio::test]
    async fn take_allows_item_within_max_weight() {
        let (_factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        take_item(&mut ctx, "coin").unwrap();
        let player = objects.get(&player_id).unwrap();
        assert!(
            player.body_slot_item("left_hand").is_some()
                || player.body_slot_item("right_hand").is_some()
        );
    }

    #[tokio::test]
    async fn take_allows_heavy_item_when_max_weight_unlimited() {
        let (_factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        if let Some(player) = objects.get_mut(&player_id) {
            player.set_property_int("max_weight", crate::object::UNLIMITED_WEIGHT);
        }

        let mut boulder = Object {
            id: ObjectId::new("item:boulder-001"),
            name: "boulder".to_string(),
            aliases: Vec::new(),
            location: Some(room_id.clone()),
            prototype: None,
            owner: player_id.clone(),
            permissions: crate::object::PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        boulder.set_property_int("weight", 200);
        objects.insert(boulder.id.clone(), boulder);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        take_item(&mut ctx, "boulder").unwrap();
        let player = objects.get(&player_id).unwrap();
        assert!(
            player.body_slot_item("left_hand").is_some()
                || player.body_slot_item("right_hand").is_some()
        );
    }

    #[tokio::test]
    async fn take_singular_splits_stack_on_ground() {
        let (_factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut bars = Object {
            id: ObjectId::new("item:bars-001"),
            name: "gold bar".to_string(),
            aliases: Vec::new(),
            location: Some(room_id.clone()),
            prototype: None,
            owner: player_id.clone(),
            permissions: crate::object::PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        bars.apply_stackable_role(&crate::object::StackableSpec {
            count: 10,
            max_stack: 99,
        });
        objects.insert(bars.id.clone(), bars);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        let msg = take_item(&mut ctx, "gold bar").unwrap();
        assert_eq!(msg, "You pick up a gold bar.");

        let ground = objects.get(&ObjectId::new("item:bars-001")).unwrap();
        assert_eq!(ground.stack_count(), 9);
        assert_eq!(ground.location.as_ref(), Some(&room_id));

        let held: Vec<_> = objects
            .values()
            .filter(|o| o.stack_count() == 1 && o.location.as_ref() == Some(&player_id))
            .collect();
        assert_eq!(held.len(), 1);
    }

    #[tokio::test]
    async fn take_plural_takes_entire_stack() {
        let (_factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut bars = Object {
            id: ObjectId::new("item:bars-001"),
            name: "gold bar".to_string(),
            aliases: Vec::new(),
            location: Some(room_id.clone()),
            prototype: None,
            owner: player_id.clone(),
            permissions: crate::object::PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        bars.apply_stackable_role(&crate::object::StackableSpec {
            count: 10,
            max_stack: 99,
        });
        objects.insert(bars.id.clone(), bars);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        let msg = take_item(&mut ctx, "gold bars").unwrap();
        assert_eq!(msg, "You pick up 10 gold bars.");

        let ground = objects.get(&ObjectId::new("item:bars-001")).unwrap();
        assert_ne!(ground.location.as_ref(), Some(&room_id));
    }

    #[tokio::test]
    async fn take_partial_respects_weight_limit() {
        let (_factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut bars = Object {
            id: ObjectId::new("item:bars-001"),
            name: "gold bar".to_string(),
            aliases: Vec::new(),
            location: Some(room_id.clone()),
            prototype: None,
            owner: player_id.clone(),
            permissions: crate::object::PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        bars.set_property_int("weight", 20);
        bars.apply_stackable_role(&crate::object::StackableSpec {
            count: 10,
            max_stack: 99,
        });
        objects.insert(bars.id.clone(), bars);

        if let Some(player) = objects.get_mut(&player_id) {
            player.set_property_int("max_weight", 100);
        }

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        let msg = take_item(&mut ctx, "10 gold bars").unwrap();
        assert_eq!(
            msg,
            "You pick up 5 gold bars, but leave 5 on the ground."
        );

        let ground = objects.get(&ObjectId::new("item:bars-001")).unwrap();
        assert_eq!(ground.stack_count(), 5);
        assert_eq!(ground.location.as_ref(), Some(&room_id));
    }

    #[tokio::test]
    async fn take_merges_into_held_stack_when_hands_occupied() {
        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut held = factory
            .create_stackable_item("gold bar", player_id.clone(), None, 3)
            .await
            .unwrap();
        held.name = "gold bar".to_string();
        held.location = Some(player_id.clone());
        let held_id = held.id.clone();

        let mut ground = factory
            .create_stackable_item("gold bar", player_id.clone(), None, 7)
            .await
            .unwrap();
        ground.name = "gold bar".to_string();
        ground.location = Some(room_id.clone());
        let ground_id = ground.id.clone();

        let mut sword = factory.create_item("sword", player_id.clone()).await.unwrap();
        sword.name = "Rusty Sword".to_string();
        sword.set_property_string("hand_slot", "right");
        sword.location = Some(player_id.clone());
        let sword_id = sword.id.clone();

        let mut player = objects.get(&player_id).unwrap().clone();
        player.set_body_slot("right_hand", Some(sword_id.clone()));
        player.set_body_slot("left_hand", Some(held_id.clone()));
        objects.insert(player_id.clone(), player);
        objects.insert(held_id.clone(), held);
        objects.insert(ground_id.clone(), ground);
        objects.insert(sword_id, sword);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        let msg = take_item(&mut ctx, "gold bars").unwrap();
        assert_eq!(msg, "You pick up 7 gold bars.");

        let merged = objects.get(&held_id).unwrap();
        assert_eq!(merged.stack_count(), 10);
        assert_eq!(merged.location.as_ref(), Some(&player_id));
        assert!(objects.get(&ground_id).is_none());
    }

    #[tokio::test]
    async fn take_singular_merges_one_into_held_stack() {
        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut held = factory
            .create_stackable_item("gold bar", player_id.clone(), None, 5)
            .await
            .unwrap();
        held.name = "gold bar".to_string();
        held.location = Some(player_id.clone());
        let held_id = held.id.clone();

        let mut ground = Object {
            id: ObjectId::new("item:bars-ground"),
            name: "gold bar".to_string(),
            aliases: Vec::new(),
            location: Some(room_id.clone()),
            prototype: held.prototype.clone(),
            owner: player_id.clone(),
            permissions: crate::object::PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        ground.apply_stackable_role(&crate::object::StackableSpec {
            count: 10,
            max_stack: 99,
        });
        let ground_id = ground.id.clone();

        let mut player = objects.get(&player_id).unwrap().clone();
        player.set_body_slot("right_hand", Some(held_id.clone()));
        objects.insert(player_id.clone(), player);
        objects.insert(held_id.clone(), held);
        objects.insert(ground_id.clone(), ground);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        let msg = take_item(&mut ctx, "gold bar").unwrap();
        assert_eq!(msg, "You pick up a gold bar.");

        let merged = objects.get(&held_id).unwrap();
        assert_eq!(merged.stack_count(), 6);
        let remainder = objects.get(&ground_id).unwrap();
        assert_eq!(remainder.stack_count(), 9);
    }

    #[tokio::test]
    async fn take_by_short_id_after_split_stack_disambiguation() {
        use crate::display::short_id;

        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        // Nearly-full ground stack: dropping 15 merges 9 and leaves 6 as a split pile.
        let mut main_stack = factory
            .create_stackable_item("gold bar", player_id.clone(), None, 90)
            .await
            .unwrap();
        main_stack.name = "gold bar".to_string();
        main_stack.location = Some(room_id.clone());
        let main_id = main_stack.id.clone();
        let shared_proto = main_stack.prototype.clone();

        let mut held = factory
            .create_stackable_item("gold bar", player_id.clone(), shared_proto, 15)
            .await
            .unwrap();
        held.name = "gold bar".to_string();
        let held_id = held.id.clone();

        let mut player = objects.get(&player_id).unwrap().clone();
        player.set_body_slot("right_hand", Some(held_id.clone()));
        objects.insert(player_id.clone(), player);
        objects.insert(main_id.clone(), main_stack);
        objects.insert(held_id.clone(), held);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        drop_item(&mut ctx, "gold bars").unwrap();

        let player = ctx.objects.get(&player_id).unwrap();
        assert!(player.body_slot_item("right_hand").is_none());
        assert!(player.body_slot_item("left_hand").is_none());

        let ground_piles: Vec<_> = ctx
            .objects
            .values()
            .filter(|o| {
                o.name == "gold bar"
                    && o.location.as_ref() == Some(&room_id)
                    && !is_carried_by(ctx.player_id, &o.id, ctx.objects)
            })
            .collect();
        assert_eq!(ground_piles.len(), 2, "drop should leave two ground piles");
        let split_id = ground_piles
            .iter()
            .find(|o| o.id != main_id)
            .unwrap()
            .id
            .clone();

        let main = ctx.objects.get(&main_id).unwrap();
        assert_eq!(main.stack_count(), 99);
        let split = ctx.objects.get(&split_id).unwrap();
        assert_eq!(split.stack_count(), 6);

        let err = take_item(&mut ctx, "gold bars").unwrap_err();
        match err {
            InventoryError::InvalidTarget(msg) => {
                assert!(msg.contains("Which gold bar do you mean?"));
                assert!(msg.contains(&short_id(&main_id)));
                assert!(msg.contains(&short_id(&split_id)));
            }
            other => panic!("expected ambiguous InvalidTarget, got {other:?}"),
        }

        let by_main = take_item(&mut ctx, &short_id(&main_id)).unwrap();
        assert!(by_main.contains("gold bar"));

        let main = ctx.objects.get(&main_id).unwrap();
        assert_eq!(main.stack_count(), 98);
        assert_eq!(main.location.as_ref(), Some(&room_id));
        assert_eq!(ctx.objects.get(&split_id).unwrap().location.as_ref(), Some(&room_id));

        take_item(&mut ctx, &format!("6 {}", short_id(&split_id))).unwrap();
        let player = ctx.objects.get(&player_id).unwrap();
        assert!(
            player.body_slot_item("right_hand").is_some()
                || player.body_slot_item("left_hand").is_some()
        );
    }

    #[tokio::test]
    async fn take_fails_hands_full_only_when_no_merge_and_no_slot() {
        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut held = factory
            .create_stackable_item("gold bar", player_id.clone(), None, 99)
            .await
            .unwrap();
        held.name = "gold bar".to_string();
        held.set_property_int("weight", 0);
        held.apply_stackable_role(&crate::object::StackableSpec {
            count: 99,
            max_stack: 99,
        });
        held.location = Some(player_id.clone());
        held.set_carried_slot(Some("left_hand"));
        let held_id = held.id.clone();

        let mut ground = factory
            .create_stackable_item("gold bar", player_id.clone(), None, 5)
            .await
            .unwrap();
        ground.name = "gold bar".to_string();
        ground.set_property_int("weight", 0);
        ground.location = Some(room_id.clone());

        let mut sword = factory.create_item("sword", player_id.clone()).await.unwrap();
        sword.name = "Sword".to_string();
        sword.set_property_int("weight", 0);
        sword.location = Some(player_id.clone());
        sword.set_carried_slot(Some("right_hand"));
        let sword_id = sword.id.clone();

        let mut player = objects.get(&player_id).unwrap().clone();
        player.set_property_int("max_weight", crate::object::UNLIMITED_WEIGHT);
        player.set_body_slot("right_hand", Some(sword_id.clone()));
        player.set_body_slot("left_hand", Some(held_id.clone()));
        objects.insert(player_id.clone(), player);
        objects.insert(held_id, held);
        objects.insert(ground.id.clone(), ground);
        objects.insert(sword_id, sword);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        let err = take_item(&mut ctx, "gold bars").unwrap_err();
        assert_eq!(err, InventoryError::HandsFull);
    }

    #[tokio::test]
    async fn drop_full_stack_clears_grasp_slots() {
        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut bars = factory
            .create_stackable_item("gold bar", player_id.clone(), None, 15)
            .await
            .unwrap();
        bars.name = "gold bar".to_string();
        let bars_id = bars.id.clone();

        let mut player = objects.get(&player_id).unwrap().clone();
        player.set_body_slot("right_hand", Some(bars_id.clone()));
        objects.insert(player_id.clone(), player);
        objects.insert(bars_id.clone(), bars);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        drop_item(&mut ctx, "gold bars").unwrap();

        let player = ctx.objects.get(&player_id).unwrap();
        assert!(player.body_slot_item("right_hand").is_none());
        assert!(player.body_slot_item("left_hand").is_none());

        let dropped = ctx.objects.get(&bars_id).unwrap();
        assert_eq!(dropped.location.as_ref(), Some(&room_id));
    }

    #[tokio::test]
    async fn take_drop_take_cycle_keeps_hands_available() {
        use crate::display::format_look_self_summary;

        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut bars = factory
            .create_stackable_item("gold bar", player_id.clone(), None, 10)
            .await
            .unwrap();
        bars.name = "gold bar".to_string();
        bars.location = Some(room_id.clone());
        objects.insert(bars.id.clone(), bars);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        take_item(&mut ctx, "gold bar").unwrap();
        drop_item(&mut ctx, "gold bar").unwrap();
        take_item(&mut ctx, "gold bar").unwrap();

        let player = ctx.objects.get(&player_id).unwrap();
        let summary = format_look_self_summary(player, ctx.objects, &anatomy);
        assert!(summary.contains("gold bar"));
        assert!(player.body_slot_item("right_hand").is_some()
            || player.body_slot_item("left_hand").is_some());
    }

    #[tokio::test]
    async fn drop_split_stack_on_ground_clears_slots_for_next_take() {
        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut main_stack = factory
            .create_stackable_item("gold bar", player_id.clone(), None, 90)
            .await
            .unwrap();
        main_stack.name = "gold bar".to_string();
        main_stack.location = Some(room_id.clone());
        let main_id = main_stack.id.clone();
        let shared_proto = main_stack.prototype.clone();

        let mut held = factory
            .create_stackable_item("gold bar", player_id.clone(), shared_proto, 15)
            .await
            .unwrap();
        held.name = "gold bar".to_string();
        let held_id = held.id.clone();

        let mut player = objects.get(&player_id).unwrap().clone();
        player.set_body_slot("right_hand", Some(held_id.clone()));
        objects.insert(player_id.clone(), player);
        objects.insert(main_id.clone(), main_stack);
        objects.insert(held_id.clone(), held);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        drop_item(&mut ctx, "gold bars").unwrap();

        let player = ctx.objects.get(&player_id).unwrap();
        assert!(player.body_slot_item("right_hand").is_none());

        let split_id = ctx
            .objects
            .values()
            .find(|o| o.name == "gold bar" && o.stack_count() == 6)
            .unwrap()
            .id
            .clone();
        take_item(&mut ctx, &format!("1 {}", crate::display::short_id(&split_id))).unwrap();
        let player = ctx.objects.get(&player_id).unwrap();
        assert!(
            player.body_slot_item("right_hand").is_some()
                || player.body_slot_item("left_hand").is_some()
        );
    }

    #[tokio::test]
    async fn drop_singular_leaves_remainder_in_hand() {
        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut bars = factory
            .create_stackable_item("gold bar", player_id.clone(), None, 10)
            .await
            .unwrap();
        bars.name = "gold bar".to_string();
        let bars_id = bars.id.clone();

        let mut player = objects.get(&player_id).unwrap().clone();
        player.set_body_slot("right_hand", Some(bars_id.clone()));
        objects.insert(player_id.clone(), player);
        objects.insert(bars_id.clone(), bars);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        let msg = drop_item(&mut ctx, "gold bar").unwrap();
        assert_eq!(msg, "You drop a gold bar.");

        let held = objects.get(&bars_id).unwrap();
        assert_eq!(held.stack_count(), 9);

        let on_ground: Vec<_> = objects
            .values()
            .filter(|o| {
                o.name == "gold bar"
                    && o.stack_count() == 1
                    && o.location.as_ref() == Some(&room_id)
            })
            .collect();
        assert_eq!(on_ground.len(), 1);
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
            dirty: None,
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
            dirty: None,
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
            dirty: None,
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
            dirty: None,
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
            dirty: None,
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
            dirty: None,
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
            dirty: None,
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
            dirty: None,
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
            dirty: None,
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
            dirty: None,
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
            ..crate::object::ContainerSpec::default()
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
            dirty: None,
        };

        let msg = put_item(&mut ctx, "coins", "purse", None).unwrap();
        assert!(msg.contains("10"));
        assert!(msg.contains("remain in your hand"));
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
        let mut coins = Object {
            id: ObjectId::new("item:coins-001"),
            name: "coins".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new("player:hero-001"),
            permissions: crate::object::PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        coins.apply_stackable_role(&crate::object::StackableSpec {
            count: 20,
            max_stack: 99,
        });
        let msg = format_put_message(&coins, "purse", 15, 20, None);
        assert_eq!(
            msg,
            "You put 15 coins in your purse, but 5 coins remain in your hand."
        );
    }

    #[test]
    fn format_put_message_exact_quantity() {
        let mut coins = Object {
            id: ObjectId::new("item:coins-001"),
            name: "coins".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new("player:hero-001"),
            permissions: crate::object::PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        coins.apply_stackable_role(&crate::object::StackableSpec {
            count: 20,
            max_stack: 99,
        });
        let msg = format_put_message(&coins, "purse", 10, 20, Some(10));
        assert_eq!(msg, "You put 10 coins in your purse.");
    }

    #[test]
    fn parse_take_singular_implies_one() {
        let req = parse_item_quantity_args("gold bar").unwrap();
        assert_eq!(req.quantity, Some(1));
        assert_eq!(req.item_name, "gold bar");
    }

    #[test]
    fn parse_take_plural_implies_all() {
        let req = parse_item_quantity_args("gold bars").unwrap();
        assert_eq!(req.quantity, None);
        assert_eq!(req.item_name, "gold bars");
    }

    #[test]
    fn parse_take_explicit_quantity() {
        let req = parse_item_quantity_args("5 gold bar").unwrap();
        assert_eq!(req.quantity, Some(5));
        assert_eq!(req.item_name, "gold bar");
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
            dirty: None,
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
            dirty: None,
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
            dirty: None,
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
            ..crate::object::ContainerSpec::default()
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
            dirty: None,
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
    async fn look_self_shows_gold_bar_after_take_from_ground() {
        use crate::display::format_look_self_summary;

        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut bars = factory
            .create_stackable_item("gold bar", player_id.clone(), None, 10)
            .await
            .unwrap();
        bars.name = "gold bar".to_string();
        bars.location = Some(room_id.clone());
        objects.insert(bars.id.clone(), bars);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        take_item(&mut ctx, "gold bar").unwrap();

        let player = ctx.objects.get(&player_id).unwrap();
        let summary = format_look_self_summary(player, ctx.objects, &anatomy);
        assert!(
            summary.contains("gold bar"),
            "look self should list held gold bar, got: {summary}"
        );
    }

    #[tokio::test]
    async fn look_self_shows_partial_stack_after_take_by_short_id() {
        use crate::display::format_look_self_summary;
        use crate::display::short_id;

        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut split = factory
            .create_stackable_item("gold bar", player_id.clone(), None, 6)
            .await
            .unwrap();
        split.name = "gold bar".to_string();
        split.location = Some(room_id.clone());
        let split_id = split.id.clone();
        objects.insert(split_id.clone(), split);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        take_item(&mut ctx, &format!("6 {}", short_id(&split_id))).unwrap();

        let player = ctx.objects.get(&player_id).unwrap();
        let summary = format_look_self_summary(player, ctx.objects, &anatomy);
        assert!(
            summary.contains("6 gold bars"),
            "look self should show partial stack count, got: {summary}"
        );
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
            ..crate::object::ContainerSpec::default()
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

    #[tokio::test]
    async fn drop_worn_backpack_clears_torso_slot() {
        use crate::display::format_look_self_summary;

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
            dirty: None,
        };

        wear_item(&mut ctx, "backpack").unwrap();
        drop_item(&mut ctx, "backpack").unwrap();

        let player = ctx.objects.get(&player_id).unwrap();
        assert!(player.body_slot_item("torso").is_none());
        assert_eq!(
            ctx.objects.get(&backpack_id).unwrap().location.as_ref(),
            Some(&room_id)
        );

        let summary = format_look_self_summary(player, ctx.objects, &anatomy);
        assert!(!summary.contains("unknown"));
        assert!(!summary.contains("backpack"));
    }

    #[tokio::test]
    async fn put_merges_coins_into_existing_purse_stack() {
        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut purse = factory
            .create_container_with_spec(
                "purse",
                player_id.clone(),
                crate::object::ContainerSpec {
                    capacity: 3,
                    max_weight: Some(100),
                    max_volume: None,
                    wearable: true,
                    wear_slot: Some("torso".to_string()),
            ..crate::object::ContainerSpec::default()
                },
                None,
            )
            .await
            .unwrap();
        purse.name = "purse".to_string();

        let proto = factory
            .create_item("coin", player_id.clone())
            .await
            .unwrap();

        let mut in_purse = factory
            .create_stackable_item("coins", player_id.clone(), Some(proto.id.clone()), 5)
            .await
            .unwrap();
        in_purse.location = Some(purse.id.clone());
        purse.set_property_list("contents", vec![in_purse.id.clone()]);

        let mut in_hand = factory
            .create_stackable_item("coins", player_id.clone(), Some(proto.id), 7)
            .await
            .unwrap();
        in_hand.set_property_int("weight", 1);
        in_hand.location = Some(player_id.clone());

        let purse_id = purse.id.clone();
        let in_hand_id = in_hand.id.clone();
        let in_purse_id = in_purse.id.clone();

        let mut player = objects.get(&player_id).unwrap().clone();
        player.set_body_slot("torso", Some(purse_id.clone()));
        player.set_body_slot("right_hand", Some(in_hand_id.clone()));
        objects.insert(player_id.clone(), player);
        objects.insert(purse_id.clone(), purse);
        objects.insert(in_purse_id.clone(), in_purse);
        objects.insert(in_hand_id.clone(), in_hand);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        put_item(&mut ctx, "coins", "purse", None).unwrap();

        let merged = objects.get(&in_purse_id).unwrap();
        assert_eq!(merged.stack_count(), 12);
        assert!(objects.get(&in_hand_id).is_none());
        assert_eq!(objects.get(&purse_id).unwrap().container_contents().len(), 1);
    }

    #[tokio::test]
    async fn put_into_unlimited_weight_container_accepts_full_stack() {
        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut chest = factory
            .create_container_with_spec(
                "chest",
                player_id.clone(),
                crate::object::ContainerSpec {
                    capacity: 5,
                    max_weight: Some(crate::object::UNLIMITED_WEIGHT),
                    max_volume: None,
                    wearable: false,
                    wear_slot: None,
            ..crate::object::ContainerSpec::default()
                },
                None,
            )
            .await
            .unwrap();
        chest.name = "chest".to_string();

        let mut bars = factory
            .create_stackable_item("gold bar", player_id.clone(), None, 50)
            .await
            .unwrap();
        bars.set_property_int("weight", 10);
        bars.location = Some(player_id.clone());

        let chest_id = chest.id.clone();
        let bars_id = bars.id.clone();

        let mut player = objects.get(&player_id).unwrap().clone();
        player.set_body_slot("right_hand", Some(bars_id.clone()));
        player.set_body_slot("torso", Some(chest_id.clone()));
        objects.insert(player_id.clone(), player);
        objects.insert(chest_id.clone(), chest);
        objects.insert(bars_id.clone(), bars);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        let msg = put_item(&mut ctx, "gold bars", "chest", None).unwrap();
        assert!(!msg.contains("remain in your hand"));

        let stored_id = objects.get(&chest_id).unwrap().container_contents()[0].clone();
        assert_eq!(objects.get(&stored_id).unwrap().stack_count(), 50);
    }

    #[tokio::test]
    async fn open_and_take_from_ground_chest() {
        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut chest = factory
            .create_container_with_spec(
                "travel chest",
                player_id.clone(),
                crate::object::ContainerSpec {
                    capacity: 8,
                    max_weight: Some(100),
                    max_volume: None,
                    wearable: false,
                    wear_slot: None,
                    open: false,
                    ..crate::object::ContainerSpec::default()
                },
                None,
            )
            .await
            .unwrap();
        chest.name = "Travel Chest".to_string();
        chest.location = Some(room_id.clone());

        let mut lantern = factory
            .create_item("iron lantern", player_id.clone())
            .await
            .unwrap();
        lantern.location = Some(chest.id.clone());
        chest.add_to_list_property("contents", lantern.id.clone());

        let chest_id = chest.id.clone();
        let lantern_id = lantern.id.clone();
        objects.insert(chest_id.clone(), chest);
        objects.insert(lantern_id.clone(), lantern);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        let err = take_item(&mut ctx, "lantern").unwrap_err();
        assert_eq!(err, InventoryError::NotFound("lantern".to_string()));

        let msg = open_container(&mut ctx, "chest").unwrap();
        assert_eq!(msg, "You open the travel chest.");

        take_item(&mut ctx, "lantern").unwrap();
        assert!(crate::world::possession::is_carried_by(
            &player_id,
            &lantern_id,
            ctx.objects
        ));
    }

    #[tokio::test]
    async fn put_into_closed_container_is_blocked() {
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
                    open: false,
                    ..crate::object::ContainerSpec::default()
                },
                None,
            )
            .await
            .unwrap();

        let coins = factory
            .create_stackable_item("coins", player_id.clone(), None, 5)
            .await
            .unwrap();

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
            dirty: None,
        };

        let err = put_item(&mut ctx, "coins", "purse", None).unwrap_err();
        assert_eq!(err, InventoryError::ContainerClosed("purse".to_string()));
    }

    #[tokio::test]
    async fn close_container_on_ground() {
        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut chest = factory
            .create_container_with_spec(
                "chest",
                player_id.clone(),
                crate::object::ContainerSpec::default(),
                None,
            )
            .await
            .unwrap();
        chest.location = Some(room_id.clone());

        let chest_id = chest.id.clone();
        objects.insert(chest_id, chest);

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        let msg = close_container(&mut ctx, "chest").unwrap();
        assert_eq!(msg, "You close the chest.");
        assert!(!ctx.objects.values().find(|o| o.name == "chest").unwrap().container_is_open());
    }

    #[tokio::test]
    async fn read_mailbox_on_ground() {
        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut mailbox = factory
            .create_item("Worn Mailbox", player_id.clone())
            .await
            .unwrap();
        mailbox.apply_readable_role(&crate::object::ReadableSpec {
            text: "WEST CLEARING — Edge of Nowhere.".to_string(),
            writable: false,
        });
        mailbox.location = Some(room_id.clone());
        objects.insert(mailbox.id.clone(), mailbox);

        let ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        let msg = read_item(&ctx, "mailbox").unwrap();
        assert!(msg.contains("WEST CLEARING"));
    }

    #[tokio::test]
    async fn read_note_in_open_chest() {
        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut chest = factory
            .create_container_with_spec(
                "travel chest",
                player_id.clone(),
                crate::object::ContainerSpec {
                    open: true,
                    ..crate::object::ContainerSpec::default()
                },
                None,
            )
            .await
            .unwrap();
        chest.location = Some(room_id.clone());

        let mut note = factory
            .create_item("Folded Note", player_id.clone())
            .await
            .unwrap();
        note.apply_readable_role(&crate::object::ReadableSpec {
            text: "Supplies within — mind the dark.".to_string(),
            writable: false,
        });
        note.location = Some(chest.id.clone());
        chest.add_to_list_property("contents", note.id.clone());

        let chest_id = chest.id.clone();
        objects.insert(chest_id, chest);
        objects.insert(note.id.clone(), note);

        let ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        let msg = read_item(&ctx, "note").unwrap();
        assert_eq!(
            msg,
            "You read the folded note:\n\nSupplies within — mind the dark."
        );
    }

    #[tokio::test]
    async fn read_note_in_closed_chest_not_found() {
        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut chest = factory
            .create_container_with_spec(
                "travel chest",
                player_id.clone(),
                crate::object::ContainerSpec {
                    open: false,
                    ..crate::object::ContainerSpec::default()
                },
                None,
            )
            .await
            .unwrap();
        chest.location = Some(room_id.clone());

        let mut note = factory
            .create_item("Folded Note", player_id.clone())
            .await
            .unwrap();
        note.apply_readable_role(&crate::object::ReadableSpec {
            text: "Supplies within — mind the dark.".to_string(),
            writable: false,
        });
        note.location = Some(chest.id.clone());
        chest.add_to_list_property("contents", note.id.clone());

        objects.insert(chest.id.clone(), chest);
        objects.insert(note.id.clone(), note);

        let ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        let err = read_item(&ctx, "note").unwrap_err();
        assert_eq!(err, InventoryError::NotFound("note".to_string()));
    }

    #[tokio::test]
    async fn read_non_readable_item_fails() {
        let (factory, anatomy, player_id, room_id, mut objects) = setup_world().await;

        let mut sword = factory
            .create_item("Chipped Blade", player_id.clone())
            .await
            .unwrap();
        sword.location = Some(room_id.clone());
        objects.insert(sword.id.clone(), sword);

        let ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        let err = read_item(&ctx, "blade").unwrap_err();
        assert_eq!(
            err,
            InventoryError::NotReadable("chipped blade".to_string())
        );
    }
}
