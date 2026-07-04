//! Shared helpers for held/worn gear lists (look self, examine self).

use std::collections::{HashMap, HashSet};

use crate::mudl::BodyPlan;
use crate::object::{Object, ObjectId};

fn grasp_slot_sort_key(name: &str) -> u8 {
    match name {
        "right_hand" => 0,
        "left_hand" => 1,
        _ => 2,
    }
}

/// Display name for gear on the body (stack count when relevant).
pub fn gear_item_name(obj: &Object) -> String {
    if obj.is_stackable() && obj.stack_count() > 1 {
        format!("{} {}", obj.stack_count(), obj.name)
    } else {
        obj.name.clone()
    }
}

/// Held grasp items and worn gear (deduped, sorted).
pub fn collect_gear_lists(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
    plan: &BodyPlan,
) -> (Vec<String>, Vec<String>) {
    let mut holding = Vec::new();
    let mut seen_hold = HashSet::new();
    let mut grasp_slots = plan.grasp_slots();
    grasp_slots.sort_by_key(|slot| grasp_slot_sort_key(&slot.name));

    for slot in grasp_slots {
        let Some(item_id) = player.body_slot_item(&slot.name) else {
            continue;
        };
        if !seen_hold.insert(item_id.clone()) {
            continue;
        }
        let Some(obj) = objects.get(&item_id) else {
            continue;
        };
        if obj.is_active() {
            holding.push(gear_item_name(obj));
        }
    }

    let mut wearing = Vec::new();
    let mut seen_wear = HashSet::new();
    for slot in plan.wear_slots() {
        let Some(item_id) = player.body_slot_item(&slot.name) else {
            continue;
        };
        if !seen_wear.insert(item_id.clone()) {
            continue;
        }
        let Some(obj) = objects.get(&item_id) else {
            continue;
        };
        if obj.is_active() {
            wearing.push(gear_item_name(obj));
        }
    }

    (holding, wearing)
}

/// Body slots currently occupied vs total defined in the anatomy plan.
pub fn occupied_body_slots(player: &Object, plan: &BodyPlan) -> u32 {
    plan.slots
        .iter()
        .filter(|slot| player.body_slot_item(&slot.name).is_some())
        .count() as u32
}