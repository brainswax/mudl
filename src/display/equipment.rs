//! Shared helpers for held/worn gear lists (look self, examine self).

use std::collections::{HashMap, HashSet};

use crate::display::stackable::stack_quantity_phrase;
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
    stack_quantity_phrase(obj)
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

    let grasp_names: HashSet<&str> = grasp_slots.iter().map(|s| s.name.as_str()).collect();

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

    // Fallback: items marked carried in grasp slots but missing from body_slots.
    let mut fallback: Vec<(u8, String)> = Vec::new();
    for (item_id, obj) in objects {
        if !obj.is_active() || obj.location.as_ref() != Some(&player.id) {
            continue;
        }
        let Some(slot) = obj.carried_slot() else {
            continue;
        };
        if !grasp_names.contains(slot.as_str()) || !seen_hold.insert(item_id.clone()) {
            continue;
        }
        fallback.push((grasp_slot_sort_key(&slot), gear_item_name(obj)));
    }
    fallback.sort_by_key(|(key, _)| *key);
    holding.extend(fallback.into_iter().map(|(_, name)| name));

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mudl::load_module;
    use crate::object::{ContainerSpec, PermissionFlags, StackableSpec};

    fn anatomy() -> crate::mudl::AnatomyRegistry {
        load_module("modules/default")
            .unwrap()
            .active_world()
            .unwrap()
            .anatomy
            .clone()
    }

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

    #[test]
    fn collect_gear_lists_omits_missing_or_grounded_items() {
        let anatomy = anatomy();
        let plan = anatomy.body_plan("human").unwrap();

        let mut player = bare("player:hero-001", "Hero");
        player.init_creature_role(anatomy.player_template("default").unwrap());

        let mut backpack = bare("item:backpack-001", "backpack");
        backpack.apply_container_role(&ContainerSpec {
            capacity: 5,
            max_weight: None,
            max_volume: None,
            wearable: true,
            wear_slot: Some("torso".to_string()),
            ..crate::object::ContainerSpec::default()
        });
        backpack.location = Some(ObjectId::new("room:ground-001"));

        let mut bars = bare("item:bars-001", "gold bar");
        bars.apply_stackable_role(&StackableSpec {
            count: 6,
            max_stack: 99,
        });
        bars.location = Some(player.id.clone());

        player.set_body_slot("torso", Some(backpack.id.clone()));
        player.set_body_slot("right_hand", Some(bars.id.clone()));

        let mut objects = HashMap::new();
        objects.insert(bars.id.clone(), bars);
        objects.insert(player.id.clone(), player.clone());
        // backpack intentionally omitted from objects map

        let (holding, wearing) = collect_gear_lists(&player, &objects, plan);
        assert_eq!(holding, vec!["6 gold bars"]);
        assert!(wearing.is_empty());
        assert!(!holding
            .iter()
            .chain(wearing.iter())
            .any(|s| s.contains("unknown")));
    }

    #[test]
    fn collect_gear_lists_never_emits_unknown_for_stale_wear_slot() {
        let anatomy = anatomy();
        let plan = anatomy.body_plan("human").unwrap();

        let mut player = bare("player:hero-001", "Hero");
        player.init_creature_role(anatomy.player_template("default").unwrap());
        player.set_body_slot("torso", Some(ObjectId::new("item:missing-cloak-001")));

        let objects = HashMap::from([(player.id.clone(), player.clone())]);

        let (holding, wearing) = collect_gear_lists(&player, &objects, plan);
        assert!(holding.is_empty());
        assert!(wearing.is_empty());
    }
}
