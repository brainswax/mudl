//! Centralized object movement with capacity/weight/volume validation and event hooks.

use std::collections::HashMap;

use crate::mudl::{AnatomyRegistry, BodyPlan};
use crate::world::possession::{
    clear_creature_slots_for_item, grasp_slot_available, is_in_player_possession,
    place_in_grasp_slots, prune_creature_body_slots, PossessionError,
};
use crate::object::LocationRef;
use crate::object::{
    is_unlimited_weight, player_carried_weight, transfer_weight, player_weight_bearer,
    would_exceed_player_max_weight, Object, ObjectId,
};
use crate::world::stack_transfer::{
    compute_stack_transfer_plan, cap_stack_transfer_plan_to_weight, fit_failure_reason,
    split_stack_id, StackTransferPlan,
};
use crate::world::dirty::DirtyTracker;

/// Errors returned by move operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveError {
    NotFound(String),
    NotAtSource,
    NotInRoom,
    NotCarried,
    HandsFull,
    SlotFull(String),
    ContainerFull,
    WeightExceeded,
    /// Item would exceed a player's `max_weight` carry limit.
    TooHeavy(String),
    VolumeExceeded,
    NotContainer,
    /// Source or destination container is closed.
    ContainerClosed(String),
    ContainerLocked(String),
    /// Item type is not permitted in a type-restricted container.
    TypeNotAllowed { container: String, allowed: Vec<String> },
    NotWearable,
    InvalidTarget(String),
    NoBodyPlan,
    SelfContainment,
}

impl std::fmt::Display for MoveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(name) => write!(f, "You don't see any {name} here."),
            Self::NotAtSource => write!(f, "That isn't where you expected."),
            Self::NotInRoom => write!(f, "That isn't here."),
            Self::NotCarried => write!(f, "You aren't carrying that."),
            Self::HandsFull => write!(f, "Your hands are full."),
            Self::SlotFull(slot) => {
                write!(
                    f,
                    "Your {} is already occupied.",
                    crate::mudl::slot_display_name(slot)
                )
            }
            Self::ContainerFull => write!(f, "That won't fit — it's full."),
            Self::WeightExceeded => write!(f, "That would be too heavy."),
            Self::TooHeavy(name) => write!(f, "The {name} is too heavy for you to carry."),
            Self::VolumeExceeded => write!(f, "That would take up too much space."),
            Self::NotContainer => write!(f, "That isn't a container."),
            Self::ContainerClosed(name) => write!(f, "The {name} is closed."),
            Self::ContainerLocked(name) => write!(f, "The {name} is locked."),
            Self::TypeNotAllowed { container, allowed } => {
                let types = crate::object::format_allowed_type_labels(allowed);
                write!(f, "The {container} only holds {types}.")
            }
            Self::NotWearable => write!(f, "You can't wear that."),
            Self::InvalidTarget(msg) => write!(f, "{msg}"),
            Self::NoBodyPlan => write!(f, "You have no body plan."),
            Self::SelfContainment => write!(f, "You can't put something inside itself."),
        }
    }
}

impl std::error::Error for MoveError {}

impl From<PossessionError> for MoveError {
    fn from(err: PossessionError) -> Self {
        match err {
            PossessionError::HandsFull => Self::HandsFull,
            PossessionError::NotCarried => Self::NotCarried,
            PossessionError::NotFound(name) => Self::NotFound(name),
        }
    }
}

impl From<MoveError> for crate::inventory::InventoryError {
    fn from(err: MoveError) -> Self {
        match err {
            MoveError::NotFound(n) => Self::NotFound(n),
            MoveError::NotAtSource | MoveError::NotInRoom => Self::NotInRoom,
            MoveError::NotCarried => Self::NotCarried,
            MoveError::HandsFull => Self::HandsFull,
            MoveError::SlotFull(s) => Self::SlotFull(s),
            MoveError::ContainerFull => Self::ContainerFull,
            MoveError::NotContainer => Self::NotContainer,
            MoveError::ContainerClosed(name) => Self::ContainerClosed(name),
            MoveError::ContainerLocked(name) => Self::ContainerLocked(name),
            MoveError::TypeNotAllowed { container, allowed } => {
                Self::TypeNotAllowed { container, allowed }
            }
            MoveError::NotWearable => Self::NotWearable,
            MoveError::InvalidTarget(m) => Self::InvalidTarget(m),
            MoveError::NoBodyPlan => Self::NoBodyPlan,
            MoveError::SelfContainment => {
                Self::InvalidTarget("You can't put something inside itself.".into())
            }
            MoveError::WeightExceeded => Self::InvalidTarget("That would be too heavy.".into()),
            MoveError::TooHeavy(name) => Self::TooHeavy(name),
            MoveError::VolumeExceeded => {
                Self::InvalidTarget("That would take up too much space.".into())
            }
        }
    }
}

/// Event payload for move hooks (stub for future trigger system).
#[derive(Debug, Clone)]
pub struct MoveEvent {
    pub object_id: ObjectId,
    pub source: LocationRef,
    pub destination: LocationRef,
}

/// Callback type for post-move event hooks.
pub type OnMoveHook = dyn Fn(&MoveEvent) + Send;

/// Optional hooks fired after a successful move.
#[derive(Default)]
pub struct MoveHooks {
    pub on_move: Option<Box<OnMoveHook>>,
}

impl MoveHooks {
    pub fn fire_on_move(&self, event: &MoveEvent) {
        if let Some(ref hook) = self.on_move {
            hook(event);
        }
    }
}

/// Mutable world slice for move operations.
pub struct MoveContext<'a> {
    pub objects: &'a mut HashMap<ObjectId, Object>,
    pub anatomy: Option<&'a AnatomyRegistry>,
    pub hooks: MoveHooks,
    pub dirty: Option<&'a mut DirtyTracker>,
}

impl<'a> MoveContext<'a> {
    fn mark_dirty(&mut self, ids: impl IntoIterator<Item = ObjectId>) {
        if let Some(dirty) = self.dirty.as_mut() {
            dirty.mark_many(ids);
        }
    }
}

/// Result of a successful move.
#[derive(Debug, Clone)]
pub struct MoveResult {
    pub object_id: ObjectId,
    pub source: LocationRef,
    pub destination: LocationRef,
    /// Units transferred when moving stackables (partial or full).
    pub units_transferred: Option<u32>,
}

/// Resolve where an object currently resides.
pub fn resolve_location(
    obj_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> Option<LocationRef> {
    let obj = objects.get(obj_id)?;
    if let Some(slot) = obj.carried_slot() {
        if let Some(loc) = &obj.location {
            if objects
                .get(loc)
                .is_some_and(|holder| holder.has_creature_role())
            {
                return Some(LocationRef::BodySlot(loc.clone(), slot));
            }
            if objects.get(loc).is_some_and(|holder| holder.is_container()) {
                return Some(LocationRef::Container(loc.clone(), None));
            }
        }
    }
    if let Some(loc) = &obj.location {
        if let Some(holder) = objects.get(loc) {
            if holder.is_container() && holder.container_contents().contains(obj_id) {
                return Some(LocationRef::Container(loc.clone(), None));
            }
            if holder.has_creature_role() {
                for (slot, id) in holder.body_slots() {
                    if &id == obj_id {
                        return Some(LocationRef::BodySlot(loc.clone(), slot));
                    }
                }
                return Some(LocationRef::Inventory(loc.clone()));
            }
            if holder.is_location() {
                return Some(LocationRef::Room(loc.clone()));
            }
        }
        return Some(LocationRef::Room(loc.clone()));
    }
    Some(LocationRef::Nowhere)
}

fn player_body_plan<'a>(
    player: &Object,
    anatomy: &'a AnatomyRegistry,
) -> Result<&'a BodyPlan, MoveError> {
    let plan_name = player.body_plan_name().ok_or(MoveError::NoBodyPlan)?;
    anatomy.body_plan(&plan_name).ok_or(MoveError::NoBodyPlan)
}

fn max_carry_units_for_weight(
    player: &Object,
    item: &Object,
    objects: &HashMap<ObjectId, Object>,
) -> u32 {
    let Some(max_w) = player.get_int_property("max_weight") else {
        return u32::MAX;
    };
    if is_unlimited_weight(max_w) {
        return u32::MAX;
    }
    let room = max_w as f64 - player_carried_weight(player, objects);
    let unit_w = item.unit_weight().max(0.0);
    if unit_w <= 0.0 {
        return u32::MAX;
    }
    (room / unit_w).floor().max(0.0) as u32
}

fn inventory_transfer_failure(
    item: &Object,
    weight_cap: u32,
) -> MoveError {
    if weight_cap == 0 && item.unit_weight() > 0.0 {
        MoveError::TooHeavy(item.name.clone())
    } else {
        MoveError::HandsFull
    }
}

fn merge_stack_units(
    ctx: &mut MoveContext<'_>,
    source_id: &ObjectId,
    merge_id: &ObjectId,
    units: u32,
    src: &LocationRef,
) -> Result<(), MoveError> {
    let source = ctx
        .objects
        .get(source_id)
        .ok_or_else(|| MoveError::NotFound(source_id.to_string()))?
        .clone();
    let stack_count = if source.is_stackable() {
        source.stack_count()
    } else {
        1
    };
    if units == 0 || units > stack_count {
        return Err(MoveError::InvalidTarget(
            "Invalid merge quantity.".into(),
        ));
    }

    let mut target = ctx
        .objects
        .get(merge_id)
        .ok_or_else(|| MoveError::NotFound(merge_id.to_string()))?
        .clone();
    target.set_stack_count(target.stack_count() + units);
    ctx.objects.insert(merge_id.clone(), target);

    if units >= stack_count {
        detach_from_source(ctx, source_id, src, true)?;
        ctx.objects.remove(source_id);
    } else {
        let mut remainder = source;
        remainder.set_stack_count(stack_count - units);
        ctx.objects.insert(source_id.clone(), remainder);
    }
    Ok(())
}

fn place_new_stack_in_container(
    ctx: &mut MoveContext<'_>,
    source_id: &ObjectId,
    src: &LocationRef,
    container_id: &ObjectId,
    units: u32,
) -> Result<ObjectId, MoveError> {
    let item = ctx
        .objects
        .get(source_id)
        .ok_or_else(|| MoveError::NotFound(source_id.to_string()))?
        .clone();
    let stack_count = if item.is_stackable() {
        item.stack_count()
    } else {
        1
    };

    if units >= stack_count {
        detach_from_source(ctx, source_id, src, true)?;
        let mut container = ctx
            .objects
            .get(container_id)
            .ok_or(MoveError::NotContainer)?
            .clone();
        container.add_to_list_property("contents", source_id.clone());
        ctx.objects.insert(container_id.clone(), container);

        let mut placed = item;
        placed.location = Some(container_id.clone());
        placed.set_carried_slot(None);
        ctx.objects.insert(source_id.clone(), placed);
        return Ok(source_id.clone());
    }

    let new_id = split_stack_id(source_id, ctx.objects);
    let mut placed = item.clone();
    placed.id = new_id.clone();
    placed.set_stack_count(units);
    placed.location = Some(container_id.clone());
    placed.set_carried_slot(None);

    let mut source = item;
    source.set_stack_count(stack_count - units);
    ctx.objects.insert(source_id.clone(), source);

    let mut container = ctx
        .objects
        .get(container_id)
        .ok_or(MoveError::NotContainer)?
        .clone();
    container.add_to_list_property("contents", new_id.clone());
    ctx.objects.insert(container_id.clone(), container);
    ctx.objects.insert(new_id.clone(), placed);
    Ok(new_id)
}

fn place_new_stack(
    ctx: &mut MoveContext<'_>,
    source_id: &ObjectId,
    src: &LocationRef,
    dst: &LocationRef,
    units: u32,
    current_primary: &ObjectId,
) -> Result<ObjectId, MoveError> {
    let Some(source) = ctx.objects.get(source_id) else {
        return Ok(current_primary.clone());
    };
    let stack_count = if source.is_stackable() {
        source.stack_count()
    } else {
        1
    };

    match dst {
        LocationRef::Container(container_id, _) => {
            let placed_id =
                place_new_stack_in_container(ctx, source_id, src, container_id, units)?;
            Ok(if current_primary == source_id {
                placed_id
            } else {
                current_primary.clone()
            })
        }
        LocationRef::Room(_) | LocationRef::Inventory(_) | LocationRef::BodySlot(_, _) => {
            if units < stack_count {
                let (_, split_id) = partial_stack_transfer(ctx, source_id, src, dst, units)?;
                Ok(if current_primary == source_id {
                    split_id
                } else {
                    current_primary.clone()
                })
            } else {
                remove_from_source(ctx, source_id, src)?;
                apply_destination(ctx, source_id, dst)?;
                Ok(source_id.clone())
            }
        }
        LocationRef::Nowhere => {
            remove_from_source(ctx, source_id, src)?;
            apply_destination(ctx, source_id, dst)?;
            Ok(source_id.clone())
        }
    }
}

fn execute_stack_transfer_plan(
    ctx: &mut MoveContext<'_>,
    source_id: &ObjectId,
    src: &LocationRef,
    dst: &LocationRef,
    plan: &StackTransferPlan,
) -> Result<(u32, ObjectId), MoveError> {
    let total = plan.total_units();
    if total == 0 {
        return Err(MoveError::InvalidTarget(
            "You must move at least one.".into(),
        ));
    }

    let mut primary_id = source_id.clone();

    if plan.merge_units > 0 {
        let merge_id = plan
            .merge_target
            .as_ref()
            .ok_or(MoveError::InvalidTarget("No merge target.".into()))?;
        merge_stack_units(ctx, source_id, merge_id, plan.merge_units, src)?;
        primary_id = merge_id.clone();
    }

    if plan.new_stack_units > 0 {
        primary_id = place_new_stack(
            ctx,
            source_id,
            src,
            dst,
            plan.new_stack_units,
            &primary_id,
        )?;
    }

    Ok((total, primary_id))
}

fn stack_transfer_failure(
    item: &Object,
    dst: &LocationRef,
    objects: &HashMap<ObjectId, Object>,
    weight_cap: u32,
) -> MoveError {
    if let LocationRef::Container(container_id, _) = dst {
        if let Some(container) = objects.get(container_id) {
            return fit_failure_reason(container, item, objects);
        }
    }
    inventory_transfer_failure(item, weight_cap)
}

/// Split `units` from a stack at `src` and place the new stack at `dst`.
/// Returns units moved and the new stack object's id.
fn partial_stack_transfer(
    ctx: &mut MoveContext<'_>,
    item_id: &ObjectId,
    src: &LocationRef,
    dst: &LocationRef,
    units: u32,
) -> Result<(u32, ObjectId), MoveError> {
    let _ = src;
    let item = ctx
        .objects
        .get(item_id)
        .ok_or_else(|| MoveError::NotFound(item_id.to_string()))?
        .clone();
    if !item.is_stackable() {
        return Err(MoveError::InvalidTarget(
            "You can only split stackable items.".into(),
        ));
    }
    let stack_count = item.stack_count();
    let units = units.min(stack_count);
    if units == 0 {
        return Err(MoveError::InvalidTarget(
            "You must move at least one.".into(),
        ));
    }
    if units >= stack_count {
        return Err(MoveError::InvalidTarget(
            "Partial transfer requires a smaller quantity.".into(),
        ));
    }

    let new_id = split_stack_id(item_id, ctx.objects);
    let mut placed = item.clone();
    placed.id = new_id.clone();
    placed.set_stack_count(units);

    let mut source = item;
    source.set_stack_count(stack_count - units);
    ctx.objects.insert(item_id.clone(), source);

    match dst {
        LocationRef::Room(room_id) => {
            placed.location = Some(room_id.clone());
            placed.set_carried_slot(None);
            ctx.objects.insert(new_id.clone(), placed);
        }
        LocationRef::Inventory(player_id) => {
            ctx.objects.insert(new_id.clone(), placed);
            apply_grasp_to_player(ctx, player_id, &new_id)?;
        }
        LocationRef::BodySlot(holder, slot) => {
            placed.location = Some(holder.clone());
            placed.set_carried_slot(Some(slot));
            ctx.objects.insert(new_id.clone(), placed);
            let mut holder_obj = ctx.objects.get(holder).ok_or(MoveError::NotCarried)?.clone();
            holder_obj.set_body_slot(slot, Some(new_id.clone()));
            ctx.objects.insert(holder.clone(), holder_obj);
        }
        _ => {
            return Err(MoveError::InvalidTarget(
                "Partial stack moves support room and inventory only.".into(),
            ));
        }
    }

    Ok((units, new_id))
}

/// Remove an item from its source location. `full_detach` clears body slots; partial keeps them.
fn detach_from_source(
    ctx: &mut MoveContext<'_>,
    item_id: &ObjectId,
    src: &LocationRef,
    full_detach: bool,
) -> Result<(), MoveError> {
    match src {
        LocationRef::Container(container_id, _) => {
            let mut container = ctx
                .objects
                .get(container_id)
                .ok_or(MoveError::NotContainer)?
                .clone();
            container.remove_from_list_property("contents", item_id);
            ctx.objects.insert(container_id.clone(), container);
        }
        LocationRef::BodySlot(holder, _) if full_detach => {
            clear_creature_slots_for_item(holder, item_id, ctx.objects);
        }
        LocationRef::Inventory(holder) if full_detach => {
            clear_creature_slots_for_item(holder, item_id, ctx.objects);
        }
        _ => {}
    }
    Ok(())
}

fn remove_from_source(
    ctx: &mut MoveContext<'_>,
    obj_id: &ObjectId,
    src: &LocationRef,
) -> Result<(), MoveError> {
    match src {
        LocationRef::Room(_) => {
            // Ground items have no parent list to update.
            Ok(())
        }
        LocationRef::BodySlot(holder, _) => {
            let holder_obj = ctx
                .objects
                .get(holder)
                .ok_or(MoveError::NotCarried)?
                .clone();
            if !holder_obj
                .body_slots()
                .values()
                .any(|id| id == obj_id)
            {
                return Err(MoveError::NotAtSource);
            }
            clear_creature_slots_for_item(holder, obj_id, ctx.objects);
            Ok(())
        }
        LocationRef::Inventory(holder) => {
            clear_creature_slots_for_item(holder, obj_id, ctx.objects);
            Ok(())
        }
        LocationRef::Container(container_id, _) => {
            let mut container = ctx
                .objects
                .get(container_id)
                .ok_or(MoveError::NotContainer)?
                .clone();
            if !container.container_contents().contains(obj_id) {
                return Err(MoveError::NotAtSource);
            }
            container.remove_from_list_property("contents", obj_id);
            ctx.objects.insert(container_id.clone(), container);
            Ok(())
        }
        LocationRef::Nowhere => Ok(()),
    }
}

fn apply_destination(
    ctx: &mut MoveContext<'_>,
    obj_id: &ObjectId,
    dst: &LocationRef,
) -> Result<(), MoveError> {
    match dst {
        LocationRef::Room(room_id) => {
            let mut item = ctx
                .objects
                .get(obj_id)
                .ok_or_else(|| MoveError::NotFound(obj_id.to_string()))?
                .clone();
            item.location = Some(room_id.clone());
            item.set_carried_slot(None);
            ctx.objects.insert(obj_id.clone(), item);
            let creature_ids: Vec<ObjectId> = ctx
                .objects
                .values()
                .filter(|o| o.has_creature_role())
                .map(|o| o.id.clone())
                .collect();
            for creature_id in creature_ids {
                clear_creature_slots_for_item(&creature_id, obj_id, ctx.objects);
            }
            Ok(())
        }
        LocationRef::Container(container_id, _) => {
            // Handled by `move_object` via `place_in_container`.
            let _ = container_id;
            Ok(())
        }
        LocationRef::BodySlot(holder, slot) => {
            let holder_obj = ctx
                .objects
                .get(holder)
                .ok_or(MoveError::NotCarried)?
                .clone();
            if holder_obj.body_slot_item(slot).is_some() {
                return Err(MoveError::SlotFull(slot.clone()));
            }
            let mut holder_obj = holder_obj;
            holder_obj.set_body_slot(slot, Some(obj_id.clone()));
            ctx.objects.insert(holder.clone(), holder_obj);

            let mut item = ctx
                .objects
                .get(obj_id)
                .ok_or_else(|| MoveError::NotFound(obj_id.to_string()))?
                .clone();
            item.location = Some(holder.clone());
            item.set_carried_slot(Some(slot));
            ctx.objects.insert(obj_id.clone(), item);
            Ok(())
        }
        LocationRef::Inventory(holder) => {
            apply_grasp_to_player(ctx, holder, obj_id)?;
            Ok(())
        }
        LocationRef::Nowhere => {
            let mut item = ctx
                .objects
                .get(obj_id)
                .ok_or_else(|| MoveError::NotFound(obj_id.to_string()))?
                .clone();
            item.location = None;
            item.set_carried_slot(None);
            ctx.objects.insert(obj_id.clone(), item);
            Ok(())
        }
    }
}

fn validate_player_carry_weight(
    item: &Object,
    item_id: &ObjectId,
    dst: &LocationRef,
    units: u32,
    objects: &HashMap<ObjectId, Object>,
) -> Result<(), MoveError> {
    let Some(player_id) = player_weight_bearer(dst, objects) else {
        return Ok(());
    };
    if is_in_player_possession(&player_id, item_id, objects) {
        return Ok(());
    }
    let Some(player) = objects.get(&player_id) else {
        return Ok(());
    };
    let added = transfer_weight(item, objects, units);
    if would_exceed_player_max_weight(player, objects, added) {
        return Err(MoveError::TooHeavy(item.name.clone()));
    }
    Ok(())
}

fn verify_at_source(
    obj_id: &ObjectId,
    src: &LocationRef,
    objects: &HashMap<ObjectId, Object>,
) -> Result<(), MoveError> {
    let actual = resolve_location(obj_id, objects).ok_or(MoveError::NotAtSource)?;
    match (src, &actual) {
        (LocationRef::Room(expected), LocationRef::Room(actual)) if expected == actual => Ok(()),
        (LocationRef::Inventory(expected), LocationRef::Inventory(actual))
        | (LocationRef::Inventory(expected), LocationRef::BodySlot(actual, _))
            if expected == actual =>
        {
            Ok(())
        }
        // Two-handed items may appear in multiple grasp slots; any slot on the holder counts.
        (
            LocationRef::BodySlot(expected_holder, _),
            LocationRef::BodySlot(actual_holder, _),
        ) if expected_holder == actual_holder => Ok(()),
        (LocationRef::Container(expected, _), LocationRef::Container(actual, _))
            if expected == actual =>
        {
            Ok(())
        }
        (LocationRef::Nowhere, LocationRef::Nowhere) => Ok(()),
        _ => Err(MoveError::NotAtSource),
    }
}

/// Move an object from `src` to `dst` with validation and optional dirty tracking.
///
/// `requested_units` limits stackable transfers; `None` moves the full stack.
pub fn move_object(
    ctx: &mut MoveContext<'_>,
    obj_id: &ObjectId,
    src: LocationRef,
    dst: LocationRef,
    requested_units: Option<u32>,
) -> Result<MoveResult, MoveError> {
    if let (LocationRef::Container(c, _), LocationRef::Container(d, _)) = (&src, &dst) {
        if c == d {
            return Err(MoveError::SelfContainment);
        }
    }
    if let LocationRef::Container(c, _) = &dst {
        if c == obj_id {
            return Err(MoveError::SelfContainment);
        }
    }

    verify_at_source(obj_id, &src, ctx.objects)?;

    if let LocationRef::Container(container_id, _) = &src {
        let container = ctx
            .objects
            .get(container_id)
            .ok_or(MoveError::NotContainer)?;
        if container.container_is_locked() {
            return Err(MoveError::ContainerLocked(container.name.clone()));
        }
        if !container.container_is_open() {
            return Err(MoveError::ContainerClosed(container.name.clone()));
        }
    }
    if let LocationRef::Container(container_id, _) = &dst {
        let container = ctx
            .objects
            .get(container_id)
            .ok_or(MoveError::NotContainer)?;
        if container.container_is_locked() {
            return Err(MoveError::ContainerLocked(container.name.clone()));
        }
        if !container.container_is_open() {
            return Err(MoveError::ContainerClosed(container.name.clone()));
        }
    }

    if let Some(holder_id) = src.holder_id() {
        prune_creature_body_slots(holder_id, ctx.objects);
    }
    if let Some(holder_id) = dst.holder_id() {
        if dst.holder_id() != src.holder_id() {
            prune_creature_body_slots(holder_id, ctx.objects);
        }
    }

    let item = ctx
        .objects
        .get(obj_id)
        .ok_or_else(|| MoveError::NotFound(obj_id.to_string()))?
        .clone();

    let free_grasp_slot = if let LocationRef::Inventory(player_id) = &dst {
        let anatomy = ctx.anatomy.ok_or(MoveError::NoBodyPlan)?;
        let player = ctx
            .objects
            .get(player_id)
            .ok_or(MoveError::NotCarried)?
            .clone();
        let body_plan = player_body_plan(&player, anatomy)?;
        grasp_slot_available(&player, &item, body_plan, ctx.objects, Some(obj_id))
    } else {
        false
    };

    let mut plan = compute_stack_transfer_plan(
        &item,
        obj_id,
        &dst,
        ctx.objects,
        ctx.anatomy,
        requested_units,
        free_grasp_slot,
    )?;

    if let LocationRef::Inventory(player_id) = &dst {
        let player = ctx.objects.get(player_id).ok_or(MoveError::NotCarried)?;
        let weight_cap = max_carry_units_for_weight(player, &item, ctx.objects);
        cap_stack_transfer_plan_to_weight(&mut plan, weight_cap);
    }

    if plan.is_empty() {
        let weight_cap = if let LocationRef::Inventory(player_id) = &dst {
            max_carry_units_for_weight(
                ctx.objects.get(player_id).ok_or(MoveError::NotCarried)?,
                &item,
                ctx.objects,
            )
        } else {
            u32::MAX
        };
        return Err(stack_transfer_failure(&item, &dst, ctx.objects, weight_cap));
    }

    validate_player_carry_weight(&item, obj_id, &dst, plan.total_units(), ctx.objects)?;

    let (units_transferred, split_object_id) = if item.is_stackable() {
        let (transferred, primary_id) =
            execute_stack_transfer_plan(ctx, obj_id, &src, &dst, &plan)?;
        let extra = if primary_id != *obj_id {
            Some(primary_id)
        } else {
            None
        };
        (Some(transferred), extra)
    } else {
        execute_stack_transfer_plan(ctx, obj_id, &src, &dst, &plan)?;
        (Some(1), None)
    };

    let mut touched = vec![obj_id.clone()];
    if let Some(split_id) = split_object_id {
        touched.push(split_id);
    }
    if let Some(id) = src.holder_id() {
        touched.push(id.clone());
    }
    if let Some(id) = dst.holder_id() {
        if !touched.contains(id) {
            touched.push(id.clone());
        }
    }
    for obj in ctx.objects.values() {
        if obj.location.as_ref() == dst.holder_id() {
            if !touched.contains(&obj.id) {
                touched.push(obj.id.clone());
            }
        }
    }

    let event = MoveEvent {
        object_id: obj_id.clone(),
        source: src.clone(),
        destination: dst.clone(),
    };
    ctx.hooks.fire_on_move(&event);

    let creatures_to_prune: Vec<ObjectId> = touched
        .iter()
        .filter(|id| {
            ctx.objects
                .get(*id)
                .is_some_and(|o| o.has_creature_role())
        })
        .cloned()
        .collect();
    for creature_id in creatures_to_prune {
        prune_creature_body_slots(&creature_id, ctx.objects);
    }

    ctx.mark_dirty(touched);

    Ok(MoveResult {
        object_id: obj_id.clone(),
        source: src,
        destination: dst,
        units_transferred,
    })
}

/// Convenience: move into a room from any current location.
pub fn move_to_room(
    ctx: &mut MoveContext<'_>,
    obj_id: &ObjectId,
    room_id: &ObjectId,
    requested_units: Option<u32>,
) -> Result<MoveResult, MoveError> {
    let src = resolve_location(obj_id, ctx.objects).ok_or(MoveError::NotAtSource)?;
    move_object(
        ctx,
        obj_id,
        src,
        LocationRef::Room(room_id.clone()),
        requested_units,
    )
}

/// Convenience: move into a player's grasp slots (inventory).
pub fn move_to_grasp(
    ctx: &mut MoveContext<'_>,
    obj_id: &ObjectId,
    player_id: &ObjectId,
    requested_units: Option<u32>,
) -> Result<MoveResult, MoveError> {
    let src = resolve_location(obj_id, ctx.objects).ok_or(MoveError::NotAtSource)?;
    move_object(
        ctx,
        obj_id,
        src,
        LocationRef::Inventory(player_id.clone()),
        requested_units,
    )
}

/// Convenience: move into a player's inventory (grasp slots).
pub fn move_to_inventory(
    ctx: &mut MoveContext<'_>,
    obj_id: &ObjectId,
    player_id: &ObjectId,
    requested_units: Option<u32>,
) -> Result<MoveResult, MoveError> {
    move_to_grasp(ctx, obj_id, player_id, requested_units)
}

fn apply_grasp_to_player(
    ctx: &mut MoveContext<'_>,
    player_id: &ObjectId,
    obj_id: &ObjectId,
) -> Result<(), MoveError> {
    let anatomy = ctx.anatomy.ok_or(MoveError::NoBodyPlan)?;
    let player = ctx
        .objects
        .get(player_id)
        .ok_or(MoveError::NotCarried)?
        .clone();
    let plan = player_body_plan(&player, anatomy)?;
    place_in_grasp_slots(player_id, obj_id, plan, ctx.objects)?;
    Ok(())
}

/// Convenience: move into a container, optionally limiting stackable quantity.
pub fn move_to_container(
    ctx: &mut MoveContext<'_>,
    obj_id: &ObjectId,
    container_id: &ObjectId,
    requested_units: Option<u32>,
) -> Result<MoveResult, MoveError> {
    if let (Some(0), _) = (requested_units, ()) {
        return Err(MoveError::InvalidTarget(
            "You must put at least one.".into(),
        ));
    }

    let src = resolve_location(obj_id, ctx.objects).ok_or(MoveError::NotAtSource)?;

    if let LocationRef::Container(c, _) = &src {
        if c == container_id {
            return Err(MoveError::SelfContainment);
        }
    }
    if container_id == obj_id {
        return Err(MoveError::SelfContainment);
    }

    move_object(
        ctx,
        obj_id,
        src,
        LocationRef::Container(container_id.clone(), None),
        requested_units,
    )
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

    async fn setup() -> (
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

        let mut heavy = factory.create_item("anvil", owner.clone()).await.unwrap();
        heavy.name = "Anvil".to_string();
        heavy.set_property_int("weight", 100);
        heavy.set_property_int("volume", 50);
        heavy.location = Some(room_id.clone());

        let mut backpack = factory
            .create_container_with_spec(
                "backpack",
                owner.clone(),
                crate::object::ContainerSpec {
                    capacity: 5,
                    max_weight: Some(30),
                    max_volume: Some(20),
                    wearable: true,
                    wear_slot: Some("torso".to_string()),
            ..crate::object::ContainerSpec::default()
                },
                None,
            )
            .await
            .unwrap();
        backpack.name = "Backpack".to_string();
        backpack.location = Some(room_id.clone());

        let mut objects = HashMap::new();
        objects.insert(player.id.clone(), player.clone());
        objects.insert(room_id.clone(), room);
        objects.insert(coin.id.clone(), coin.clone());
        objects.insert(heavy.id.clone(), heavy);
        objects.insert(backpack.id.clone(), backpack.clone());

        (anatomy, owner, room_id, objects)
    }

    #[tokio::test]
    async fn move_from_room_to_inventory() {
        let (anatomy, player_id, room_id, mut objects) = setup().await;
        let coin_id = objects
            .values()
            .find(|o| o.name == "Gold Coin")
            .unwrap()
            .id
            .clone();

        let mut dirty = DirtyTracker::default();
        let mut ctx = MoveContext {
            objects: &mut objects,
            anatomy: Some(&anatomy),
            hooks: MoveHooks::default(),
            dirty: Some(&mut dirty),
        };

        move_object(
            &mut ctx,
            &coin_id,
            LocationRef::Room(room_id.clone()),
            LocationRef::Inventory(player_id.clone()),
            None,
        )
        .unwrap();

        let player = objects.get(&player_id).unwrap();
        assert!(
            player.body_slot_item("left_hand").is_some()
                || player.body_slot_item("right_hand").is_some()
        );
        assert!(!dirty.is_empty());
    }

    #[tokio::test]
    async fn partial_stack_take_from_room() {
        let (anatomy, player_id, room_id, mut objects) = setup().await;

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
        let bars_id = bars.id.clone();
        objects.insert(bars_id.clone(), bars);

        let mut ctx = MoveContext {
            objects: &mut objects,
            anatomy: Some(&anatomy),
            hooks: MoveHooks::default(),
            dirty: None,
        };

        let result = move_object(
            &mut ctx,
            &bars_id,
            LocationRef::Room(room_id.clone()),
            LocationRef::Inventory(player_id.clone()),
            Some(3),
        )
        .unwrap();

        assert_eq!(result.units_transferred, Some(3));
        let ground = objects.get(&bars_id).unwrap();
        assert_eq!(ground.stack_count(), 7);
        assert_eq!(ground.location.as_ref(), Some(&room_id));
    }

    #[tokio::test]
    async fn move_rejects_item_heavier_than_player_max_weight() {
        let (anatomy, player_id, room_id, mut objects) = setup().await;

        let mut boulder = objects
            .values()
            .find(|o| o.name == "Anvil")
            .unwrap()
            .clone();
        boulder.id = ObjectId::new("item:boulder-001");
        boulder.name = "boulder".to_string();
        boulder.set_property_int("weight", 200);
        boulder.location = Some(room_id.clone());
        objects.insert(boulder.id.clone(), boulder.clone());

        let mut ctx = MoveContext {
            objects: &mut objects,
            anatomy: Some(&anatomy),
            hooks: MoveHooks::default(),
            dirty: None,
        };

        let err = move_object(
            &mut ctx,
            &boulder.id,
            LocationRef::Room(room_id.clone()),
            LocationRef::Inventory(player_id.clone()),
            None,
        )
        .unwrap_err();

        assert_eq!(err, MoveError::TooHeavy("boulder".to_string()));
        assert_eq!(
            objects.get(&boulder.id).unwrap().location.as_ref(),
            Some(&room_id)
        );
    }

    #[tokio::test]
    async fn move_rejects_overweight_container() {
        let (anatomy, player_id, room_id, mut objects) = setup().await;
        let heavy_id = objects
            .values()
            .find(|o| o.name == "Anvil")
            .unwrap()
            .id
            .clone();
        let backpack_id = objects
            .values()
            .find(|o| o.name == "Backpack")
            .unwrap()
            .id
            .clone();

        let mut ctx = MoveContext {
            objects: &mut objects,
            anatomy: Some(&anatomy),
            hooks: MoveHooks::default(),
            dirty: None,
        };

        move_object(
            &mut ctx,
            &heavy_id,
            LocationRef::Room(room_id.clone()),
            LocationRef::Inventory(player_id.clone()),
            None,
        )
        .unwrap();

        let err = move_object(
            &mut ctx,
            &heavy_id,
            LocationRef::Inventory(player_id.clone()),
            LocationRef::Container(backpack_id, None),
            None,
        )
        .unwrap_err();
        assert_eq!(err, MoveError::WeightExceeded);
    }

    #[tokio::test]
    async fn partial_stack_put_respects_max_weight() {
        let (anatomy, player_id, _, mut objects) = setup().await;

        let mut purse = objects
            .values()
            .find(|o| o.name == "Backpack")
            .unwrap()
            .clone();
        purse.name = "purse".to_string();
        purse.set_property_int("capacity", 3);
        purse.set_property_int("max_weight", 10);
        purse.set_property_list("contents", vec![]);

        let mut coins = objects
            .values()
            .find(|o| o.name == "Gold Coin")
            .unwrap()
            .clone();
        coins.name = "coins".to_string();
        coins.set_property_int("weight", 1);
        coins.set_property_int("volume", 1);
        coins.apply_stackable_role(&crate::object::StackableSpec {
            count: 20,
            max_stack: 99,
        });
        coins.location = Some(player_id.clone());

        let purse_id = purse.id.clone();
        let coins_id = coins.id.clone();

        let mut player = objects.get(&player_id).unwrap().clone();
        player.set_body_slot("right_hand", Some(coins_id.clone()));
        player.set_body_slot("torso", Some(purse_id.clone()));
        objects.insert(player_id.clone(), player);
        objects.insert(purse_id.clone(), purse);
        objects.insert(coins_id.clone(), coins);

        let mut ctx = MoveContext {
            objects: &mut objects,
            anatomy: Some(&anatomy),
            hooks: MoveHooks::default(),
            dirty: None,
        };

        let result = move_object(
            &mut ctx,
            &coins_id,
            LocationRef::BodySlot(player_id.clone(), "right_hand".to_string()),
            LocationRef::Container(purse_id.clone(), None),
            None,
        )
        .unwrap();

        assert_eq!(result.units_transferred, Some(10));
        let purse = objects.get(&purse_id).unwrap();
        assert_eq!(purse.container_contents().len(), 1);
        let in_purse = objects.get(&purse.container_contents()[0]).unwrap();
        assert_eq!(in_purse.stack_count(), 10);

        let remainder = objects.get(&coins_id).unwrap();
        assert_eq!(remainder.stack_count(), 10);
        assert_eq!(remainder.location.as_ref(), Some(&player_id));
    }

    #[tokio::test]
    async fn on_move_hook_fires() {
        let (anatomy, player_id, room_id, mut objects) = setup().await;
        let coin_id = objects
            .values()
            .find(|o| o.name == "Gold Coin")
            .unwrap()
            .id
            .clone();

        let fired = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let fired_clone = fired.clone();
        let mut ctx = MoveContext {
            objects: &mut objects,
            anatomy: Some(&anatomy),
            hooks: MoveHooks {
                on_move: Some(Box::new(move |_| {
                    fired_clone.store(true, std::sync::atomic::Ordering::SeqCst);
                })),
            },
            dirty: None,
        };

        move_object(
            &mut ctx,
            &coin_id,
            LocationRef::Room(room_id),
            LocationRef::Inventory(player_id),
            None,
        )
        .unwrap();

        assert!(fired.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn drop_merges_into_ground_stack() {
        let (anatomy, player_id, room_id, mut objects) = setup().await;

        let mut ground = Object {
            id: ObjectId::new("item:bars-ground"),
            name: "gold bar".to_string(),
            aliases: Vec::new(),
            location: Some(room_id.clone()),
            prototype: Some(ObjectId::new("item:bar-proto")),
            owner: player_id.clone(),
            permissions: crate::object::PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        ground.apply_stackable_role(&crate::object::StackableSpec {
            count: 5,
            max_stack: 99,
        });

        let mut held = ground.clone();
        held.id = ObjectId::new("item:bars-held");
        held.apply_stackable_role(&crate::object::StackableSpec {
            count: 7,
            max_stack: 99,
        });
        held.location = Some(player_id.clone());
        let held_id = held.id.clone();

        let mut player = objects.get(&player_id).unwrap().clone();
        player.set_body_slot("right_hand", Some(held_id.clone()));
        objects.insert(player_id.clone(), player);
        objects.insert(ground.id.clone(), ground);
        objects.insert(held_id.clone(), held);

        let mut ctx = MoveContext {
            objects: &mut objects,
            anatomy: Some(&anatomy),
            hooks: MoveHooks::default(),
            dirty: None,
        };

        let result = move_to_room(&mut ctx, &held_id, &room_id, None).unwrap();
        assert_eq!(result.units_transferred, Some(7));
        assert!(objects.get(&held_id).is_none());

        let merged = objects.get(&ObjectId::new("item:bars-ground")).unwrap();
        assert_eq!(merged.stack_count(), 12);
        assert_eq!(merged.location.as_ref(), Some(&room_id));

        let player = objects.get(&player_id).unwrap();
        assert!(player.body_slot_item("right_hand").is_none());
        assert!(player.body_slot_item("left_hand").is_none());
    }

    #[tokio::test]
    async fn drop_two_handed_item_clears_both_grasp_slots() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence);
        let (anatomy, player_id, room_id, mut objects) = setup().await;

        let mut sword = factory
            .create_item("greatsword", player_id.clone())
            .await
            .unwrap();
        sword.name = "Greatsword".to_string();
        sword.set_property_string("hand_slot", "both");
        sword.location = Some(player_id.clone());
        sword.set_carried_slot(Some("left_hand"));
        let sword_id = sword.id.clone();

        let mut player = objects.get(&player_id).unwrap().clone();
        player.set_body_slot("left_hand", Some(sword_id.clone()));
        player.set_body_slot("right_hand", Some(sword_id.clone()));
        objects.insert(player_id.clone(), player);
        objects.insert(sword_id.clone(), sword);

        let mut ctx = MoveContext {
            objects: &mut objects,
            anatomy: Some(&anatomy),
            hooks: MoveHooks::default(),
            dirty: None,
        };

        move_to_room(&mut ctx, &sword_id, &room_id, None).unwrap();

        let player = objects.get(&player_id).unwrap();
        assert!(player.body_slot_item("left_hand").is_none());
        assert!(player.body_slot_item("right_hand").is_none());
        assert_eq!(objects.get(&sword_id).unwrap().location.as_ref(), Some(&room_id));
    }

    #[tokio::test]
    async fn move_to_unlimited_weight_container_accepts_heavy_stack() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence);
        let (anatomy, player_id, _, mut objects) = setup().await;

        let chest = factory
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

        let mut bars = factory
            .create_stackable_item("gold bar", player_id.clone(), None, 40)
            .await
            .unwrap();
        bars.set_property_int("weight", 25);
        bars.location = Some(player_id.clone());

        let chest_id = chest.id.clone();
        let bars_id = bars.id.clone();

        let mut player = objects.get(&player_id).unwrap().clone();
        player.set_body_slot("right_hand", Some(bars_id.clone()));
        player.set_body_slot("torso", Some(chest_id.clone()));
        objects.insert(player_id.clone(), player);
        objects.insert(chest_id.clone(), chest);
        objects.insert(bars_id.clone(), bars);

        let mut ctx = MoveContext {
            objects: &mut objects,
            anatomy: Some(&anatomy),
            hooks: MoveHooks::default(),
            dirty: None,
        };

        let src = resolve_location(&bars_id, ctx.objects).unwrap();
        let result = move_object(
            &mut ctx,
            &bars_id,
            src,
            LocationRef::Container(chest_id.clone(), None),
            None,
        )
        .unwrap();

        assert_eq!(result.units_transferred, Some(40));
        let stored_id = objects.get(&chest_id).unwrap().container_contents()[0].clone();
        assert_eq!(objects.get(&stored_id).unwrap().stack_count(), 40);
    }
}
