//! Player possession, body slots, and carried-gear graph operations.
//!
//! Centralizes logic previously spread across `Object`, `display::resolve`, `inventory`,
//! and `move_manager`. Persistence still stores `body_slots` / `carried_slot` on objects;
//! this module owns interpretation and mutation of that graph.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::mudl::BodyPlan;
use crate::object::{Object, ObjectId};

/// Errors from grasp-slot placement and possession mutations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PossessionError {
    HandsFull,
    NotCarried,
    NotFound(String),
}

// --- Body slot property access ---

/// All body-slot assignments on a creature (slot name → item id).
pub fn body_slots(creature: &Object) -> HashMap<String, ObjectId> {
    creature.get_object_map_property("body_slots")
}

/// Item occupying a named body slot, if any.
pub fn body_slot_item(creature: &Object, slot: &str) -> Option<ObjectId> {
    body_slots(creature).get(slot).cloned()
}

/// Assign or clear a body slot entry.
pub fn set_body_slot(creature: &mut Object, slot: &str, item: Option<ObjectId>) {
    let mut slots = body_slots(creature);
    if let Some(id) = item {
        slots.insert(slot.to_string(), id);
    } else {
        slots.remove(slot);
    }
    creature.set_property_map("body_slots", slots);
}

/// Remove every body-slot reference to `item_id`.
pub fn clear_item_from_body_slots(creature: &mut Object, item_id: &ObjectId) {
    let slots: HashMap<String, ObjectId> = body_slots(creature)
        .into_iter()
        .filter(|(_, id)| id != item_id)
        .collect();
    creature.set_property_map("body_slots", slots);
}

/// Deduplicated item ids referenced by any body slot on this creature.
pub fn carried_body_items(creature: &Object) -> Vec<ObjectId> {
    let mut seen = Vec::new();
    for id in body_slots(creature).values() {
        if !seen.contains(id) {
            seen.push(id.clone());
        }
    }
    seen
}

// --- Validation ---

/// Whether `item_id` is a valid occupant of a body slot on this creature.
pub fn body_slot_item_valid(
    creature: &Object,
    item_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> bool {
    let Some(item) = objects.get(item_id) else {
        return false;
    };
    if !item.is_active() {
        return false;
    }
    let holder_id = &creature.id;
    let Some(loc) = &item.location else {
        return false;
    };
    if objects.get(loc).is_some_and(|o| o.is_location()) {
        return false;
    }
    if loc == holder_id {
        return true;
    }
    item_reachable_in_carried_gear(creature, item_id, objects)
}

fn item_reachable_in_carried_gear(
    creature: &Object,
    item_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> bool {
    let Some(item) = objects.get(item_id) else {
        return false;
    };
    let Some(mut loc) = item.location.clone() else {
        return false;
    };
    let mut visited = HashSet::new();
    while visited.insert(loc.clone()) {
        if loc == creature.id {
            return true;
        }
        if body_slots(creature).values().any(|id| id == &loc) {
            return true;
        }
        let Some(holder) = objects.get(&loc) else {
            return false;
        };
        let Some(next) = holder.location.clone() else {
            return false;
        };
        loc = next;
    }
    false
}

/// Drop body slot entries that point at missing, ground, or otherwise un-carried items.
pub fn prune_stale_body_slots(creature: &mut Object, objects: &HashMap<ObjectId, Object>) {
    let stale: Vec<String> = body_slots(creature)
        .into_iter()
        .filter_map(|(slot, item_id)| {
            if body_slot_item_valid(creature, &item_id, objects) {
                None
            } else {
                Some(slot)
            }
        })
        .collect();
    for slot in stale {
        set_body_slot(creature, &slot, None);
    }
}

/// Prune stale slots on a creature object in the world map.
pub fn prune_creature_body_slots(
    creature_id: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
) {
    let Some(creature) = objects.get(creature_id).cloned() else {
        return;
    };
    if !creature.has_creature_role() {
        return;
    }
    let mut creature = creature;
    prune_stale_body_slots(&mut creature, objects);
    objects.insert(creature_id.clone(), creature);
}

/// Clear all body-slot references to `item_id` on a creature in the world map.
pub fn clear_creature_slots_for_item(
    creature_id: &ObjectId,
    item_id: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
) {
    let Some(mut creature) = objects.get(creature_id).cloned() else {
        return;
    };
    clear_item_from_body_slots(&mut creature, item_id);
    objects.insert(creature_id.clone(), creature);
}

// --- Possession graph queries ---

/// Whether `item_id` is on the player's body or inside a carried/worn container (BFS).
pub fn is_in_player_possession(
    player_id: &ObjectId,
    item_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> bool {
    if item_id == player_id {
        return true;
    }

    let Some(player) = objects.get(player_id) else {
        return false;
    };

    if body_slots(player).values().any(|id| id == item_id) {
        return true;
    }

    let mut queue: VecDeque<ObjectId> = carried_body_items(player).into_iter().collect();
    let mut visited = HashMap::new();

    while let Some(container_id) = queue.pop_front() {
        if visited.contains_key(&container_id) {
            continue;
        }
        visited.insert(container_id.clone(), ());

        let Some(container) = objects.get(&container_id) else {
            continue;
        };
        if !container.is_container() {
            continue;
        }

        for content_id in container.container_contents() {
            if &content_id == item_id {
                return true;
            }
            if objects
                .get(&content_id)
                .is_some_and(|obj| obj.is_container())
            {
                queue.push_back(content_id);
            }
        }
    }

    false
}

/// Alias for [`is_in_player_possession`] used by inventory commands.
pub fn is_carried_by(
    player_id: &ObjectId,
    item_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> bool {
    is_in_player_possession(player_id, item_id, objects)
}

// --- Grasp slot placement ---

/// Whether a grasp slot can accept a new item (empty or pointing at a stale reference).
pub fn grasp_slot_free(
    player: &Object,
    slot: &str,
    objects: &HashMap<ObjectId, Object>,
) -> bool {
    match body_slot_item(player, slot) {
        None => true,
        Some(item_id) => !body_slot_item_valid(player, &item_id, objects),
    }
}

/// Select grasp slots for an item according to `hand_slot` preference and anatomy.
pub fn select_grasp_slots(
    player: &Object,
    item: &Object,
    plan: &BodyPlan,
    objects: &HashMap<ObjectId, Object>,
) -> Result<(Vec<String>, Option<String>), PossessionError> {
    let hand_pref = item.hand_slot();
    let preference = hand_pref.as_deref().unwrap_or("right");
    let grasp_names: Vec<String> = plan.grasp_slots().iter().map(|s| s.name.clone()).collect();

    let (target_slots, carried_label) = if preference == "both" {
        let left = "left_hand";
        let right = "right_hand";
        if !grasp_slot_free(player, left, objects) || !grasp_slot_free(player, right, objects) {
            return Err(PossessionError::HandsFull);
        }
        (
            vec![left.to_string(), right.to_string()],
            Some(left.to_string()),
        )
    } else if preference == "left" {
        if !grasp_slot_free(player, "left_hand", objects) {
            return Err(PossessionError::HandsFull);
        }
        (vec!["left_hand".to_string()], Some("left_hand".to_string()))
    } else if grasp_slot_free(player, "right_hand", objects) {
        (
            vec!["right_hand".to_string()],
            Some("right_hand".to_string()),
        )
    } else if grasp_slot_free(player, "left_hand", objects) {
        (vec!["left_hand".to_string()], Some("left_hand".to_string()))
    } else {
        return Err(PossessionError::HandsFull);
    };

    for slot in &grasp_names {
        if target_slots.contains(slot) && !grasp_slot_free(player, slot, objects) {
            return Err(PossessionError::HandsFull);
        }
    }

    Ok((target_slots, carried_label))
}

/// Whether the player has grasp capacity for `item` under `plan`.
pub fn grasp_slot_available(
    player: &Object,
    item: &Object,
    plan: &BodyPlan,
    objects: &HashMap<ObjectId, Object>,
) -> bool {
    select_grasp_slots(player, item, plan, objects).is_ok()
}

/// Place `item_id` into grasp slots on `player_id`, updating location and `carried_slot`.
pub fn place_in_grasp_slots(
    player_id: &ObjectId,
    item_id: &ObjectId,
    plan: &BodyPlan,
    objects: &mut HashMap<ObjectId, Object>,
) -> Result<Vec<String>, PossessionError> {
    let item = objects.get(item_id).ok_or(PossessionError::NotCarried)?.clone();
    let player = objects.get(player_id).ok_or(PossessionError::NotCarried)?;
    let (target_slots, carried_label) = select_grasp_slots(player, &item, plan, objects)?;

    let mut player = objects.get(player_id).ok_or(PossessionError::NotCarried)?.clone();
    for slot in &target_slots {
        set_body_slot(&mut player, slot, Some(item_id.clone()));
    }
    objects.insert(player_id.clone(), player);

    let mut item = objects
        .get(item_id)
        .ok_or_else(|| PossessionError::NotFound(item_id.to_string()))?
        .clone();
    item.location = Some(player_id.clone());
    item.set_carried_slot(carried_label.as_deref().or(Some(target_slots[0].as_str())));
    objects.insert(item_id.clone(), item);

    Ok(target_slots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{PermissionFlags, StackableSpec};

    fn bare(id: &str, name: &str) -> Object {
        Object {
            id: ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new("player:hero-001"),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        }
    }

    #[test]
    fn prune_stale_body_slots_removes_ground_referenced_items() {
        let owner = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("room:void-001");

        let mut player = bare("player:hero-001", "Hero");
        player.location = Some(room_id.clone());

        let mut bars = bare("item:gold-bar-001", "gold bar");
        bars.apply_stackable_role(&StackableSpec {
            count: 6,
            max_stack: 99,
        });
        bars.location = Some(room_id.clone());
        let bars_id = bars.id.clone();

        set_body_slot(&mut player, "right_hand", Some(bars_id.clone()));

        let objects = HashMap::from([
            (owner.clone(), player.clone()),
            (bars_id.clone(), bars),
        ]);

        assert!(!body_slot_item_valid(&player, &bars_id, &objects));

        prune_stale_body_slots(&mut player, &objects);
        assert!(body_slot_item(&player, "right_hand").is_none());
    }

    #[test]
    fn is_in_player_possession_finds_nested_container_contents() {
        let player_id = ObjectId::new("player:hero-001");

        let mut bag = bare("item:bag-001", "bag");
        bag.apply_container_role(&crate::object::ContainerSpec::default());

        let mut pouch = bare("item:pouch-001", "pouch");
        pouch.apply_container_role(&crate::object::ContainerSpec::default());

        let mut gem = bare("item:gem-001", "gem");
        let gem_id = gem.id.clone();
        gem.location = Some(pouch.id.clone());
        pouch.set_property_list("contents", vec![gem_id.clone()]);
        pouch.location = Some(bag.id.clone());
        bag.set_property_list("contents", vec![pouch.id.clone()]);

        let mut player = bare("player:hero-001", "Hero");
        set_body_slot(&mut player, "right_hand", Some(bag.id.clone()));

        let objects = HashMap::from([
            (player_id.clone(), player),
            (bag.id.clone(), bag),
            (pouch.id.clone(), pouch),
            (gem_id.clone(), gem),
        ]);

        assert!(is_in_player_possession(&player_id, &gem_id, &objects));
    }
}