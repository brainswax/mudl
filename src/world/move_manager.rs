//! Centralized object movement with capacity/weight/volume validation and event hooks.

use std::collections::HashMap;

use crate::mudl::{AnatomyRegistry, BodyPlan};
use crate::object::LocationRef;
use crate::object::{Object, ObjectId};
use crate::world::container_fit::{
    compute_container_fit, fit_failure_reason, split_stack_id, ContainerFit,
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
    VolumeExceeded,
    NotContainer,
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
            Self::VolumeExceeded => write!(f, "That would take up too much space."),
            Self::NotContainer => write!(f, "That isn't a container."),
            Self::NotWearable => write!(f, "You can't wear that."),
            Self::InvalidTarget(msg) => write!(f, "{msg}"),
            Self::NoBodyPlan => write!(f, "You have no body plan."),
            Self::SelfContainment => write!(f, "You can't put something inside itself."),
        }
    }
}

impl std::error::Error for MoveError {}

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
            MoveError::NotWearable => Self::NotWearable,
            MoveError::InvalidTarget(m) => Self::InvalidTarget(m),
            MoveError::NoBodyPlan => Self::NoBodyPlan,
            MoveError::SelfContainment => {
                Self::InvalidTarget("You can't put something inside itself.".into())
            }
            MoveError::WeightExceeded => Self::InvalidTarget("That would be too heavy.".into()),
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

fn grasp_slot_free(player: &Object, slot: &str) -> bool {
    player.body_slot_item(slot).is_none()
}

fn place_in_container(
    ctx: &mut MoveContext<'_>,
    item_id: &ObjectId,
    container_id: &ObjectId,
    src: &LocationRef,
    fit: &ContainerFit,
) -> Result<u32, MoveError> {
    let item = ctx
        .objects
        .get(item_id)
        .ok_or_else(|| MoveError::NotFound(item_id.to_string()))?
        .clone();
    let stack_count = if item.is_stackable() {
        item.stack_count()
    } else {
        1
    };
    let units = fit.units.min(stack_count);
    if units == 0 {
        let container = ctx
            .objects
            .get(container_id)
            .ok_or(MoveError::NotContainer)?
            .clone();
        return Err(fit_failure_reason(&container, &item, ctx.objects));
    }

    let consumes_source = units >= stack_count;

    if let Some(ref merge_id) = fit.merge_target {
        let target = ctx
            .objects
            .get(merge_id)
            .ok_or_else(|| MoveError::NotFound(merge_id.to_string()))?
            .clone();
        let mut target = target;
        target.set_stack_count(target.stack_count() + units);
        ctx.objects.insert(merge_id.clone(), target);

        if consumes_source {
            detach_from_source(ctx, item_id, src, true)?;
            ctx.objects.remove(item_id);
        } else {
            let mut source = item;
            source.set_stack_count(stack_count - units);
            ctx.objects.insert(item_id.clone(), source);
        }
        return Ok(units);
    }

    if consumes_source {
        detach_from_source(ctx, item_id, src, true)?;

        let mut container = ctx
            .objects
            .get(container_id)
            .ok_or(MoveError::NotContainer)?
            .clone();
        container.add_to_list_property("contents", item_id.clone());
        ctx.objects.insert(container_id.clone(), container);

        let mut item = item;
        item.location = Some(container_id.clone());
        item.set_carried_slot(None);
        ctx.objects.insert(item_id.clone(), item);
        return Ok(units);
    }

    // Partial split: remainder stays at source (e.g. in hand), new stack in container.
    let new_id = split_stack_id(item_id, ctx.objects);
    let mut placed = item.clone();
    placed.id = new_id.clone();
    placed.set_stack_count(units);
    placed.location = Some(container_id.clone());
    placed.set_carried_slot(None);

    let mut source = item;
    source.set_stack_count(stack_count - units);
    ctx.objects.insert(item_id.clone(), source);

    let mut container = ctx
        .objects
        .get(container_id)
        .ok_or(MoveError::NotContainer)?
        .clone();
    container.add_to_list_property("contents", new_id.clone());
    ctx.objects.insert(container_id.clone(), container);
    ctx.objects.insert(new_id, placed);

    Ok(units)
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
        LocationRef::BodySlot(holder, slot) if full_detach => {
            let mut holder_obj = ctx
                .objects
                .get(holder)
                .ok_or(MoveError::NotCarried)?
                .clone();
            holder_obj.set_body_slot(slot, None);
            ctx.objects.insert(holder.clone(), holder_obj);
        }
        LocationRef::Inventory(_) if full_detach => {
            for obj in ctx.objects.values_mut() {
                if obj.has_creature_role() {
                    obj.clear_item_from_body_slots(item_id);
                }
            }
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
        LocationRef::BodySlot(holder, slot) => {
            let mut holder_obj = ctx
                .objects
                .get(holder)
                .ok_or(MoveError::NotCarried)?
                .clone();
            if holder_obj.body_slot_item(slot).as_ref() != Some(obj_id) {
                return Err(MoveError::NotAtSource);
            }
            holder_obj.set_body_slot(slot, None);
            ctx.objects.insert(holder.clone(), holder_obj);
            Ok(())
        }
        LocationRef::Inventory(holder) => {
            let mut holder_obj = ctx
                .objects
                .get(holder)
                .ok_or(MoveError::NotCarried)?
                .clone();
            holder_obj.clear_item_from_body_slots(obj_id);
            ctx.objects.insert(holder.clone(), holder_obj);
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

fn place_in_grasp_slots(
    player_id: &ObjectId,
    item_id: &ObjectId,
    plan: &BodyPlan,
    objects: &mut HashMap<ObjectId, Object>,
) -> Result<Vec<String>, MoveError> {
    let item = objects.get(item_id).ok_or(MoveError::NotCarried)?.clone();
    let player = objects.get(player_id).ok_or(MoveError::NotCarried)?;
    let hand_pref = item.hand_slot();
    let preference = hand_pref.as_deref().unwrap_or("right");

    let grasp_names: Vec<String> = plan.grasp_slots().iter().map(|s| s.name.clone()).collect();

    let (target_slots, carried_label) = if preference == "both" {
        let left = "left_hand";
        let right = "right_hand";
        if !grasp_slot_free(player, left) || !grasp_slot_free(player, right) {
            return Err(MoveError::HandsFull);
        }
        (
            vec![left.to_string(), right.to_string()],
            Some(left.to_string()),
        )
    } else if preference == "left" {
        if !grasp_slot_free(player, "left_hand") {
            return Err(MoveError::HandsFull);
        }
        (vec!["left_hand".to_string()], Some("left_hand".to_string()))
    } else if grasp_slot_free(player, "right_hand") {
        (
            vec!["right_hand".to_string()],
            Some("right_hand".to_string()),
        )
    } else if grasp_slot_free(player, "left_hand") {
        (vec!["left_hand".to_string()], Some("left_hand".to_string()))
    } else {
        return Err(MoveError::HandsFull);
    };

    for slot in &grasp_names {
        if target_slots.contains(slot) && !grasp_slot_free(player, slot) {
            return Err(MoveError::HandsFull);
        }
    }

    let mut player = objects.get(player_id).ok_or(MoveError::NotCarried)?.clone();
    for slot in &target_slots {
        player.set_body_slot(slot, Some(item_id.clone()));
    }
    objects.insert(player_id.clone(), player);

    let mut item = objects
        .get(item_id)
        .ok_or(MoveError::NotFound(item_id.to_string()))?
        .clone();
    item.location = Some(player_id.clone());
    item.set_carried_slot(carried_label.as_deref().or(Some(target_slots[0].as_str())));
    objects.insert(item_id.clone(), item);

    Ok(target_slots)
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
            let anatomy = ctx.anatomy.ok_or(MoveError::NoBodyPlan)?;
            let player = ctx
                .objects
                .get(holder)
                .ok_or(MoveError::NotCarried)?
                .clone();
            let plan = player_body_plan(&player, anatomy)?;
            place_in_grasp_slots(holder, obj_id, plan, ctx.objects)?;
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
        (
            LocationRef::BodySlot(expected_holder, expected_slot),
            LocationRef::BodySlot(actual_holder, actual_slot),
        ) if expected_holder == actual_holder && expected_slot == actual_slot => Ok(()),
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
pub fn move_object(
    ctx: &mut MoveContext<'_>,
    obj_id: &ObjectId,
    src: LocationRef,
    dst: LocationRef,
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

    let item = ctx
        .objects
        .get(obj_id)
        .ok_or_else(|| MoveError::NotFound(obj_id.to_string()))?
        .clone();

    let units_transferred = if let LocationRef::Container(container_id, _) = &dst {
        let container = ctx
            .objects
            .get(container_id)
            .ok_or(MoveError::NotContainer)?
            .clone();
        let fit = compute_container_fit(&container, &item, ctx.objects)?;
        Some(place_in_container(ctx, obj_id, container_id, &src, &fit)?)
    } else {
        remove_from_source(ctx, obj_id, &src)?;
        apply_destination(ctx, obj_id, &dst)?;
        None
    };

    let mut touched = vec![obj_id.clone()];
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
            touched.push(obj.id.clone());
        }
    }

    let event = MoveEvent {
        object_id: obj_id.clone(),
        source: src.clone(),
        destination: dst.clone(),
    };
    ctx.hooks.fire_on_move(&event);
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
) -> Result<MoveResult, MoveError> {
    let src = resolve_location(obj_id, ctx.objects).ok_or(MoveError::NotAtSource)?;
    move_object(ctx, obj_id, src, LocationRef::Room(room_id.clone()))
}

/// Convenience: move into a player's inventory (grasp slots).
pub fn move_to_inventory(
    ctx: &mut MoveContext<'_>,
    obj_id: &ObjectId,
    player_id: &ObjectId,
) -> Result<MoveResult, MoveError> {
    let src = resolve_location(obj_id, ctx.objects).ok_or(MoveError::NotAtSource)?;
    move_object(ctx, obj_id, src, LocationRef::Inventory(player_id.clone()))
}

/// Convenience: move into a container.
pub fn move_to_container(
    ctx: &mut MoveContext<'_>,
    obj_id: &ObjectId,
    container_id: &ObjectId,
) -> Result<MoveResult, MoveError> {
    let src = resolve_location(obj_id, ctx.objects).ok_or(MoveError::NotAtSource)?;
    move_object(
        ctx,
        obj_id,
        src,
        LocationRef::Container(container_id.clone(), None),
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
        )
        .unwrap();

        let err = move_object(
            &mut ctx,
            &heavy_id,
            LocationRef::Inventory(player_id.clone()),
            LocationRef::Container(backpack_id, None),
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
        )
        .unwrap();

        assert!(fired.load(std::sync::atomic::Ordering::SeqCst));
    }
}
