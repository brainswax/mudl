//! Unified stack merge planning and helpers for all move destinations.

use std::collections::{HashMap, HashSet};

use crate::mudl::{AnatomyRegistry, BodyPlan};
use crate::object::{LocationRef, Object, ObjectId};
use crate::world::move_manager::MoveError;

/// How a stackable transfer should be applied at a destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackTransferPlan {
    /// Existing stack at the destination to absorb units into.
    pub merge_target: Option<ObjectId>,
    pub merge_units: u32,
    /// Units that require a new stack object or container slot at the destination.
    pub new_stack_units: u32,
}

impl StackTransferPlan {
    pub fn total_units(&self) -> u32 {
        self.merge_units.saturating_add(self.new_stack_units)
    }

    pub fn is_empty(&self) -> bool {
        self.total_units() == 0
    }

    fn from_container_fit(fit: ContainerFit) -> Self {
        if fit.merge_target.is_some() {
            Self {
                merge_target: fit.merge_target,
                merge_units: fit.units,
                new_stack_units: 0,
            }
        } else {
            Self {
                merge_target: None,
                merge_units: 0,
                new_stack_units: fit.units,
            }
        }
    }

    fn from_inventory_fit(fit: InventoryFit) -> Self {
        Self {
            merge_target: fit.merge_target,
            merge_units: fit.merge_units,
            new_stack_units: fit.new_stack_units,
        }
    }
}

/// Container capacity result (kept for callers that need container-specific fit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerFit {
    pub units: u32,
    pub merge_target: Option<ObjectId>,
}

/// Inventory grasp capacity result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryFit {
    pub merge_target: Option<ObjectId>,
    pub merge_units: u32,
    pub new_stack_units: u32,
}

impl InventoryFit {
    pub fn total_units(&self) -> u32 {
        self.merge_units.saturating_add(self.new_stack_units)
    }
}

/// Key used to decide whether two stackables can merge.
pub fn stack_merge_key(item: &Object) -> String {
    if let Some(proto) = &item.prototype {
        return format!("proto:{}", proto.as_str());
    }
    crate::object::id_base_from_display_name(&item.name)
}

pub fn stacks_can_merge(a: &Object, b: &Object) -> bool {
    a.is_stackable() && b.is_stackable() && stack_merge_key(a) == stack_merge_key(b)
}

/// Generate a unique ID for a stack split sibling.
pub fn split_stack_id(source: &ObjectId, objects: &HashMap<ObjectId, Object>) -> ObjectId {
    let base = source.as_str();
    for n in 1..=0xfff {
        let candidate = ObjectId::new(format!("{base}-s{n:03x}"));
        if !objects.contains_key(&candidate) {
            return candidate;
        }
    }
    ObjectId::new(format!("{base}-s{:x}", objects.len()))
}

fn effective_stack_count(item: &Object) -> u32 {
    if item.is_stackable() {
        item.stack_count()
    } else {
        1
    }
}

fn units_requested(item: &Object, requested_units: Option<u32>) -> u32 {
    let stack_count = effective_stack_count(item);
    match requested_units {
        None => stack_count,
        Some(0) => 0,
        Some(n) => n.min(stack_count),
    }
}

fn merge_room_in_stack(target: &Object) -> u32 {
    target.max_stack().saturating_sub(target.stack_count())
}

/// Find a mergeable stack on the ground in `room_id` (excluding `source_id`).
pub fn find_mergeable_stack_in_room(
    room_id: &ObjectId,
    item: &Object,
    source_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> Option<ObjectId> {
    if !item.is_stackable() {
        return None;
    }
    let item_key = stack_merge_key(item);
    objects
        .values()
        .filter(|o| {
            o.is_active()
                && o.is_stackable()
                && o.id != *source_id
                && o.location.as_ref() == Some(room_id)
                && !o.has_creature_role()
                && !o.is_container()
                && !o.is_location()
                && stack_merge_key(o) == item_key
                && merge_room_in_stack(o) > 0
        })
        .min_by_key(|o| o.stack_count())
        .map(|o| o.id.clone())
}

/// Find a mergeable stack inside `container`.
pub fn find_mergeable_stack_in_container(
    container: &Object,
    item: &Object,
    objects: &HashMap<ObjectId, Object>,
) -> Option<ObjectId> {
    if !item.is_stackable() {
        return None;
    }
    let item_key = stack_merge_key(item);
    for id in container.container_contents() {
        let Some(existing) = objects.get(&id) else {
            continue;
        };
        if existing.is_stackable()
            && stack_merge_key(existing) == item_key
            && merge_room_in_stack(existing) > 0
        {
            return Some(id);
        }
    }
    None
}

/// Find a mergeable stack held in the player's grasp slots.
pub fn find_mergeable_stack_in_grasp(
    player: &Object,
    item: &Object,
    plan: &BodyPlan,
    objects: &HashMap<ObjectId, Object>,
) -> Option<ObjectId> {
    if !item.is_stackable() {
        return None;
    }
    let item_key = stack_merge_key(item);
    let mut seen = HashSet::new();
    for slot in plan.grasp_slots() {
        let Some(item_id) = player.body_slot_item(&slot.name) else {
            continue;
        };
        if !seen.insert(item_id.clone()) {
            continue;
        }
        let Some(existing) = objects.get(&item_id) else {
            continue;
        };
        if existing.is_stackable()
            && stack_merge_key(existing) == item_key
            && merge_room_in_stack(existing) > 0
        {
            return Some(item_id);
        }
    }
    None
}

/// Backward-compatible alias.
pub fn find_mergeable_stack(
    container: &Object,
    item: &Object,
    objects: &HashMap<ObjectId, Object>,
) -> Option<ObjectId> {
    find_mergeable_stack_in_container(container, item, objects)
}

fn compute_room_plan(
    item: &Object,
    source_id: &ObjectId,
    room_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
    requested_units: Option<u32>,
) -> StackTransferPlan {
    if !item.is_stackable() {
        return StackTransferPlan {
            merge_target: None,
            merge_units: 0,
            new_stack_units: 1,
        };
    }

    let units_requested = units_requested(item, requested_units);
    let merge_target = find_mergeable_stack_in_room(room_id, item, source_id, objects);
    let merge_room = merge_target
        .as_ref()
        .and_then(|id| objects.get(id))
        .map(merge_room_in_stack)
        .unwrap_or(0);

    let merge_units = if merge_room > 0 && merge_target.is_some() {
        units_requested.min(merge_room)
    } else {
        0
    };
    let new_stack_units = units_requested.saturating_sub(merge_units);

    StackTransferPlan {
        merge_target,
        merge_units,
        new_stack_units,
    }
}

fn compute_body_slot_plan(
    item: &Object,
    holder: &Object,
    slot: &str,
    requested_units: Option<u32>,
) -> StackTransferPlan {
    let can_place = holder.body_slot_item(slot).is_none();
    if !item.is_stackable() {
        return StackTransferPlan {
            merge_target: None,
            merge_units: 0,
            new_stack_units: if can_place { 1 } else { 0 },
        };
    }
    let units = if can_place {
        units_requested(item, requested_units).min(item.max_stack())
    } else {
        0
    };
    StackTransferPlan {
        merge_target: None,
        merge_units: 0,
        new_stack_units: units,
    }
}

/// Compute how many units of `item` can fit into `container`.
pub fn compute_container_fit(
    container: &Object,
    item: &Object,
    objects: &HashMap<ObjectId, Object>,
    requested_units: Option<u32>,
) -> Result<ContainerFit, MoveError> {
    if !container.is_container() {
        return Err(MoveError::NotContainer);
    }
    if container.container_is_locked() {
        return Err(MoveError::ContainerLocked(container.name.clone()));
    }
    if !container.container_is_open() {
        return Err(MoveError::ContainerClosed(container.name.clone()));
    }
    if !container.container_accepts_item(item) {
        return Err(MoveError::TypeNotAllowed {
            container: container.name.to_lowercase(),
            allowed: container.container_allowed_types().unwrap_or_default(),
        });
    }

    let merge_target = find_mergeable_stack_in_container(container, item, objects);
    let stack_count = effective_stack_count(item);
    let unit_w = item.unit_weight().max(0.0);
    let unit_v = item.unit_volume().max(0.0);

    let mut max_units = stack_count;

    if merge_target.is_none() {
        let slots_used = container.container_contents().len() as u32;
        let slots_free = container.container_capacity().saturating_sub(slots_used);
        if slots_free == 0 {
            return Ok(ContainerFit {
                units: 0,
                merge_target: None,
            });
        }
        if item.is_stackable() {
            max_units = max_units.min(item.max_stack());
        }
    } else if let Some(ref target_id) = merge_target {
        if let Some(target) = objects.get(target_id) {
            max_units = max_units.min(merge_room_in_stack(target));
        }
    }

    if let Some(max_w) = container.container_max_weight() {
        if crate::object::weight_limit_applies(Some(max_w)) {
            let room = max_w as f64 - container.contents_weight(objects);
            if unit_w > 0.0 {
                max_units = max_units.min((room / unit_w).floor() as u32);
            }
        }
    } else if unit_w < 0.0 {
        max_units = 0;
    }

    if let Some(max_v) = container.container_max_volume() {
        let room = max_v as f64 - container.contents_volume(objects);
        if unit_v > 0.0 {
            max_units = max_units.min((room / unit_v).floor() as u32);
        }
    } else if unit_v < 0.0 {
        max_units = 0;
    }

    if let Some(req) = requested_units {
        if req == 0 {
            max_units = 0;
        } else {
            max_units = max_units.min(req.min(stack_count));
        }
    }

    Ok(ContainerFit {
        units: max_units,
        merge_target,
    })
}

/// Compute how many units of `item` can move into a player's grasp.
pub fn compute_inventory_fit(
    player: &Object,
    item: &Object,
    plan: &BodyPlan,
    objects: &HashMap<ObjectId, Object>,
    requested_units: Option<u32>,
    free_grasp_slot: bool,
) -> InventoryFit {
    if !item.is_stackable() {
        return InventoryFit {
            merge_target: None,
            merge_units: 0,
            new_stack_units: if free_grasp_slot { 1 } else { 0 },
        };
    }

    let units_requested = units_requested(item, requested_units);
    let merge_target = find_mergeable_stack_in_grasp(player, item, plan, objects);
    let merge_room = merge_target
        .as_ref()
        .and_then(|id| objects.get(id))
        .map(merge_room_in_stack)
        .unwrap_or(0);

    let new_stack_room = if free_grasp_slot { item.max_stack() } else { 0 };

    let merge_units = if merge_room > 0 && merge_target.is_some() {
        units_requested.min(merge_room)
    } else {
        0
    };
    let remaining = units_requested.saturating_sub(merge_units);
    let new_stack_units = if remaining > 0 && new_stack_room > 0 {
        remaining.min(new_stack_room)
    } else {
        0
    };

    InventoryFit {
        merge_target,
        merge_units,
        new_stack_units,
    }
}

/// Plan a stackable transfer into `dst` (merge first, then new stack if needed).
pub fn compute_stack_transfer_plan(
    item: &Object,
    source_id: &ObjectId,
    dst: &LocationRef,
    objects: &HashMap<ObjectId, Object>,
    anatomy: Option<&AnatomyRegistry>,
    requested_units: Option<u32>,
    free_grasp_slot: bool,
) -> Result<StackTransferPlan, MoveError> {
    if !item.is_stackable() {
        return Ok(match dst {
            LocationRef::Container(container_id, _) => {
                let container = objects
                    .get(container_id)
                    .ok_or(MoveError::NotContainer)?
                    .clone();
                let fit = compute_container_fit(&container, item, objects, requested_units)?;
                StackTransferPlan::from_container_fit(fit)
            }
            LocationRef::Inventory(_) => StackTransferPlan {
                merge_target: None,
                merge_units: 0,
                new_stack_units: if free_grasp_slot { 1 } else { 0 },
            },
            LocationRef::Room(_) => StackTransferPlan {
                merge_target: None,
                merge_units: 0,
                new_stack_units: 1,
            },
            LocationRef::BodySlot(holder, slot) => {
                let holder = objects.get(holder).ok_or(MoveError::NotCarried)?;
                compute_body_slot_plan(item, holder, slot, requested_units)
            }
            LocationRef::Nowhere => StackTransferPlan {
                merge_target: None,
                merge_units: 0,
                new_stack_units: effective_stack_count(item),
            },
        });
    }

    Ok(match dst {
        LocationRef::Room(room_id) => {
            compute_room_plan(item, source_id, room_id, objects, requested_units)
        }
        LocationRef::Container(container_id, _) => {
            let container = objects
                .get(container_id)
                .ok_or(MoveError::NotContainer)?
                .clone();
            let fit = compute_container_fit(&container, item, objects, requested_units)?;
            StackTransferPlan::from_container_fit(fit)
        }
        LocationRef::Inventory(player_id) => {
            let anatomy = anatomy.ok_or(MoveError::NoBodyPlan)?;
            let player = objects.get(player_id).ok_or(MoveError::NotCarried)?;
            let body_plan = anatomy
                .body_plan(&player.body_plan_name().ok_or(MoveError::NoBodyPlan)?)
                .ok_or(MoveError::NoBodyPlan)?;
            StackTransferPlan::from_inventory_fit(compute_inventory_fit(
                player,
                item,
                body_plan,
                objects,
                requested_units,
                free_grasp_slot,
            ))
        }
        LocationRef::BodySlot(holder, slot) => {
            let holder = objects.get(holder).ok_or(MoveError::NotCarried)?;
            compute_body_slot_plan(item, holder, slot, requested_units)
        }
        LocationRef::Nowhere => StackTransferPlan {
            merge_target: None,
            merge_units: 0,
            new_stack_units: units_requested(item, requested_units),
        },
    })
}

/// Cap a transfer plan to a weight-based unit limit (prefers merging first).
pub fn cap_stack_transfer_plan_to_weight(plan: &mut StackTransferPlan, max_units: u32) {
    if plan.total_units() <= max_units {
        return;
    }
    plan.merge_units = plan.merge_units.min(max_units);
    let remaining = max_units.saturating_sub(plan.merge_units);
    plan.new_stack_units = plan.new_stack_units.min(remaining);
}

/// Backward-compatible alias.
pub fn cap_inventory_fit_to_weight(fit: &mut InventoryFit, max_units: u32) {
    fit.merge_units = fit.merge_units.min(max_units);
    let remaining = max_units.saturating_sub(fit.merge_units);
    fit.new_stack_units = fit.new_stack_units.min(remaining);
}

/// Classify why an item does not fit in a container.
pub fn fit_failure_reason(
    container: &Object,
    item: &Object,
    objects: &HashMap<ObjectId, Object>,
) -> MoveError {
    if container.container_is_locked() {
        return MoveError::ContainerLocked(container.name.clone());
    }
    if !container.container_is_open() {
        return MoveError::ContainerClosed(container.name.clone());
    }
    if !container.container_accepts_item(item) {
        return MoveError::TypeNotAllowed {
            container: container.name.to_lowercase(),
            allowed: container.container_allowed_types().unwrap_or_default(),
        };
    }

    let fit = match compute_container_fit(container, item, objects, None) {
        Ok(fit) => fit,
        Err(err) => return err,
    };

    if fit.merge_target.is_none()
        && container.container_contents().len() >= container.container_capacity() as usize
    {
        return MoveError::ContainerFull;
    }

    let unit_w = item.unit_weight().max(1.0);
    let unit_v = item.unit_volume().max(1.0);

    if let Some(max_w) = container.container_max_weight() {
        if crate::object::weight_limit_applies(Some(max_w)) {
            let room = max_w as f64 - container.contents_weight(objects);
            if room < unit_w {
                return MoveError::WeightExceeded;
            }
        }
    }

    if let Some(max_v) = container.container_max_volume() {
        let room = max_v as f64 - container.contents_volume(objects);
        if room < unit_v {
            return MoveError::VolumeExceeded;
        }
    }

    if let Some(ref target_id) = fit.merge_target {
        if let Some(target) = objects.get(target_id) {
            if merge_room_in_stack(target) == 0 {
                return MoveError::ContainerFull;
            }
        }
    }

    MoveError::ContainerFull
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::PermissionFlags;

    fn bare(id: &str, name: &str) -> Object {
        Object {
            id: ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new("player:hero-001"),
            permissions: PermissionFlags::OWNER,
            properties: Default::default(),
            verbs: Default::default(),
            event_handlers: Default::default(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        }
    }

    fn coins(count: u32) -> Object {
        let mut c = bare("item:coins-001", "Gold Coins");
        c.set_property_int("weight", 1);
        c.set_property_int("volume", 1);
        c.apply_stackable_role(&crate::object::StackableSpec {
            count,
            max_stack: 99,
        });
        c
    }

    #[test]
    fn room_plan_merges_into_ground_stack() {
        let room_id = ObjectId::new("room:test-001");
        let mut ground = coins(5);
        ground.id = ObjectId::new("item:ground-001");
        ground.location = Some(room_id.clone());

        let mut incoming = coins(7);
        incoming.id = ObjectId::new("item:held-001");
        incoming.prototype = ground.prototype.clone();

        let mut objects = HashMap::new();
        objects.insert(ground.id.clone(), ground);

        let plan = compute_room_plan(&incoming, &incoming.id, &room_id, &objects, None);
        assert_eq!(plan.merge_target, Some(ObjectId::new("item:ground-001")));
        assert_eq!(plan.merge_units, 7);
        assert_eq!(plan.new_stack_units, 0);
    }

    fn purse() -> Object {
        let mut p = bare("item:purse-001", "purse");
        p.apply_container_role(&crate::object::ContainerSpec {
            capacity: 3,
            max_weight: Some(10),
            max_volume: None,
            wearable: true,
            wear_slot: Some("torso".to_string()),
            ..crate::object::ContainerSpec::default()
        });
        p
    }

    #[test]
    fn empty_purse_fits_partial_stack_by_weight() {
        let purse = purse();
        let coins = coins(20);
        let objects = HashMap::new();

        let fit = compute_container_fit(&purse, &coins, &objects, None).unwrap();
        assert_eq!(fit.units, 10);
        assert_eq!(fit.merge_target, None);
    }

    #[test]
    fn merge_into_existing_container_stack() {
        let mut purse = purse();
        let existing = coins(3);
        purse.set_property_list("contents", vec![existing.id.clone()]);

        let mut incoming = coins(20);
        incoming.prototype = Some(ObjectId::new("item:coin-proto-001"));
        let mut existing = existing;
        existing.prototype = Some(ObjectId::new("item:coin-proto-001"));

        let mut objects = HashMap::new();
        objects.insert(existing.id.clone(), existing);

        let fit = compute_container_fit(&purse, &incoming, &objects, None).unwrap();
        assert_eq!(fit.merge_target, Some(ObjectId::new("item:coins-001")));
        assert_eq!(fit.units, 7);
    }

    #[test]
    fn inventory_fit_merges_into_held_stack() {
        use crate::mudl::load_module;

        let anatomy = load_module("modules/default")
            .unwrap()
            .active_world()
            .unwrap()
            .anatomy
            .clone();
        let plan = anatomy.body_plan("human").unwrap();

        let mut player = bare("player:hero-001", "Hero");
        let mut held = coins(5);
        held.name = "gold bar".to_string();
        held.id = ObjectId::new("item:held-001");
        player.set_body_slot("right_hand", Some(held.id.clone()));

        let mut incoming = coins(10);
        incoming.name = "gold bar".to_string();
        incoming.id = ObjectId::new("item:ground-001");
        incoming.prototype = held.prototype.clone();

        let mut objects = HashMap::new();
        objects.insert(held.id.clone(), held);

        let fit = compute_inventory_fit(&player, &incoming, plan, &objects, None, false);
        assert_eq!(fit.merge_target, Some(ObjectId::new("item:held-001")));
        assert_eq!(fit.merge_units, 10);
    }

    #[test]
    fn room_plan_splits_merge_and_new_pile() {
        let room_id = ObjectId::new("room:test-001");
        let mut ground = coins(90);
        ground.id = ObjectId::new("item:ground-001");
        ground.location = Some(room_id.clone());
        ground.apply_stackable_role(&crate::object::StackableSpec {
            count: 90,
            max_stack: 99,
        });

        let mut incoming = coins(15);
        incoming.id = ObjectId::new("item:held-001");
        incoming.prototype = ground.prototype.clone();

        let mut objects = HashMap::new();
        objects.insert(ground.id.clone(), ground);

        let plan = compute_room_plan(&incoming, &incoming.id, &room_id, &objects, None);
        assert_eq!(plan.merge_units, 9);
        assert_eq!(plan.new_stack_units, 6);
    }
}
