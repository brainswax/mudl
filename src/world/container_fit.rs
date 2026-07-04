//! Container capacity, weight, and volume calculations for stackable items.

use std::collections::{HashMap, HashSet};

use crate::mudl::BodyPlan;
use crate::object::{Object, ObjectId};
use crate::world::move_manager::MoveError;

/// How many units of an item can be placed into a container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerFit {
    /// Units that can be transferred (0 = does not fit).
    pub units: u32,
    /// Existing stack in the container to merge into, if any.
    pub merge_target: Option<ObjectId>,
}

/// How many units of a stackable can move into a player's grasp (merge and/or new stack).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryFit {
    pub merge_target: Option<ObjectId>,
    /// Units to add to an existing held stack.
    pub merge_units: u32,
    /// Units that need a free grasp slot as a separate stack.
    pub new_stack_units: u32,
}

impl InventoryFit {
    pub fn total_units(&self) -> u32 {
        self.merge_units.saturating_add(self.new_stack_units)
    }
}

/// Find a held stackable in the player's grasp slots that can merge with `item`.
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
        if existing.is_stackable() && stack_merge_key(existing) == item_key {
            return Some(item_id);
        }
    }
    None
}

/// Compute how many units of `item` can be taken into the player's grasp.
///
/// `free_grasp_slot` is whether a new stack could occupy an empty hand slot.
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

    let stack_count = effective_stack_count(item);
    let units_requested = match requested_units {
        None => stack_count,
        Some(0) => 0,
        Some(n) => n.min(stack_count),
    };

    let merge_target = find_mergeable_stack_in_grasp(player, item, plan, objects);
    let merge_room = merge_target
        .as_ref()
        .and_then(|id| objects.get(id))
        .map(|t| t.max_stack().saturating_sub(t.stack_count()))
        .unwrap_or(0);

    let new_stack_room = if free_grasp_slot {
        item.max_stack()
    } else {
        0
    };

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

/// Cap an inventory fit to a weight-based unit limit (prefers merging first).
pub fn cap_inventory_fit_to_weight(fit: &mut InventoryFit, max_units: u32) {
    if fit.total_units() <= max_units {
        return;
    }
    fit.merge_units = fit.merge_units.min(max_units);
    let remaining = max_units.saturating_sub(fit.merge_units);
    fit.new_stack_units = fit.new_stack_units.min(remaining);
}

/// Find a stackable item in `container` that can merge with `item`.
pub fn find_mergeable_stack(
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
        if existing.is_stackable() && stack_merge_key(existing) == item_key {
            return Some(id);
        }
    }
    None
}

/// Key used to decide whether two stackables can merge.
pub fn stack_merge_key(item: &Object) -> String {
    if let Some(proto) = &item.prototype {
        return format!("proto:{}", proto.as_str());
    }
    crate::object::id_base_from_display_name(&item.name)
}

/// Compute how many units of `item` can fit into `container`.
///
/// `requested_units` caps the transfer (e.g. `put 10 coins`); `None` means as many as fit.
pub fn compute_container_fit(
    container: &Object,
    item: &Object,
    objects: &HashMap<ObjectId, Object>,
    requested_units: Option<u32>,
) -> Result<ContainerFit, MoveError> {
    if !container.is_container() {
        return Err(MoveError::NotContainer);
    }

    let merge_target = find_mergeable_stack(container, item, objects);
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
            let room = target.max_stack().saturating_sub(target.stack_count());
            max_units = max_units.min(room);
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

/// Classify why an item does not fit (for error messages).
pub fn fit_failure_reason(
    container: &Object,
    item: &Object,
    objects: &HashMap<ObjectId, Object>,
) -> MoveError {
    let fit = compute_container_fit(container, item, objects, None).unwrap_or(ContainerFit {
        units: 0,
        merge_target: None,
    });

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
            if target.stack_count() >= target.max_stack() {
                return MoveError::ContainerFull;
            }
        }
    }

    MoveError::ContainerFull
}

fn effective_stack_count(item: &Object) -> u32 {
    if item.is_stackable() {
        item.stack_count()
    } else {
        1
    }
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
            is_deleted: false,
            deleted_at: None,
        }
    }

    fn purse() -> Object {
        let mut p = bare("item:purse-001", "purse");
        p.apply_container_role(&crate::object::ContainerSpec {
            capacity: 3,
            max_weight: Some(10),
            max_volume: None,
            wearable: true,
            wear_slot: Some("torso".to_string()),
        });
        p
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
    fn empty_purse_fits_partial_stack_by_weight() {
        let purse = purse();
        let coins = coins(20);
        let objects = HashMap::new();

        let fit = compute_container_fit(&purse, &coins, &objects, None).unwrap();
        assert_eq!(fit.units, 10);
        assert_eq!(fit.merge_target, None);
    }

    #[test]
    fn empty_purse_fits_small_stack_whole() {
        let purse = purse();
        let coins = coins(5);
        let objects = HashMap::new();

        let fit = compute_container_fit(&purse, &coins, &objects, None).unwrap();
        assert_eq!(fit.units, 5);
    }

    #[test]
    fn merge_into_existing_stack() {
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
        assert_eq!(fit.units, 7); // max_weight 10 - 3 existing = 7 room
    }

    #[test]
    fn unlimited_max_weight_fits_entire_stack() {
        let mut bag = bare("item:bag-001", "bag");
        bag.apply_container_role(&crate::object::ContainerSpec {
            capacity: 2,
            max_weight: Some(crate::object::UNLIMITED_WEIGHT),
            max_volume: None,
            wearable: false,
            wear_slot: None,
        });
        let coins = coins(50);
        let objects = HashMap::new();

        let fit = compute_container_fit(&bag, &coins, &objects, None).unwrap();
        assert_eq!(fit.units, 50);
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
        assert_eq!(fit.new_stack_units, 0);
    }

    #[test]
    fn inventory_fit_splits_merge_and_new_stack() {
        use crate::mudl::load_module;

        let anatomy = load_module("modules/default")
            .unwrap()
            .active_world()
            .unwrap()
            .anatomy
            .clone();
        let plan = anatomy.body_plan("human").unwrap();

        let mut player = bare("player:hero-001", "Hero");

        let mut held = coins(90);
        held.name = "gold bar".to_string();
        held.id = ObjectId::new("item:held-001");
        held.apply_stackable_role(&crate::object::StackableSpec {
            count: 90,
            max_stack: 99,
        });
        player.set_body_slot("right_hand", Some(held.id.clone()));

        let mut incoming = coins(15);
        incoming.name = "gold bar".to_string();
        incoming.id = ObjectId::new("item:ground-001");
        incoming.prototype = held.prototype.clone();

        let mut objects = HashMap::new();
        objects.insert(held.id.clone(), held);

        let fit = compute_inventory_fit(&player, &incoming, plan, &objects, None, true);
        assert_eq!(fit.merge_units, 9);
        assert_eq!(fit.new_stack_units, 6);
        assert_eq!(fit.total_units(), 15);
    }

    #[test]
    fn requested_quantity_caps_fit() {
        let purse = purse();
        let coins = coins(20);
        let objects = HashMap::new();

        let fit = compute_container_fit(&purse, &coins, &objects, Some(10)).unwrap();
        assert_eq!(fit.units, 10);
    }
}
