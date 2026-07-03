//! Brief carried/worn summaries for `look self`.

use std::collections::{HashMap, HashSet};

use crate::display::format_stackable_label;
use crate::mudl::AnatomyRegistry;
use crate::object::{Object, ObjectId};

fn item_label(obj: &Object) -> String {
    format_stackable_label(obj).to_lowercase()
}

fn grasp_slot_sort_key(name: &str) -> u8 {
    match name {
        "right_hand" => 0,
        "left_hand" => 1,
        _ => 2,
    }
}

/// Short `look self` line: held grasp items and worn gear, no slots or container contents.
///
/// Example: `You are holding: purse, coins. Wearing: backpack.`
pub fn format_look_self_summary(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> String {
    let plan_name = player
        .body_plan_name()
        .unwrap_or_else(|| "human".to_string());
    let Some(plan) = anatomy.body_plan(&plan_name) else {
        return "You aren't carrying anything.".to_string();
    };

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
        };
        let Some(obj) = objects.get(&item_id) else {
            continue;
        };
        if obj.is_active() {
            holding.push(item_label(obj));
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
        };
        let Some(obj) = objects.get(&item_id) else {
            continue;
        };
        if obj.is_active() {
            wearing.push(item_label(obj));
        }
    }

    let mut parts = Vec::new();
    if !holding.is_empty() {
        parts.push(format!("You are holding: {}.", holding.join(", ")));
    }
    if !wearing.is_empty() {
        parts.push(format!("Wearing: {}.", wearing.join(", ")));
    }

    if parts.is_empty() {
        "You aren't carrying anything.".to_string()
    } else {
        parts.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mudl::load_module;
    use crate::object::{ContainerSpec, PermissionFlags, StackableSpec};

    fn anatomy() -> AnatomyRegistry {
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
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        }
    }

    #[test]
    fn look_self_lists_holding_and_wearing_without_slots() {
        let anatomy = anatomy();
        let mut player = bare("player:hero-001", "Hero");
        player.init_creature_role(anatomy.player_template("default").unwrap());

        let mut purse = bare("item:purse-001", "purse");
        purse.apply_container_role(&ContainerSpec {
            capacity: 3,
            max_weight: Some(10),
            max_volume: None,
            wearable: false,
            wear_slot: None,
        });

        let mut coins = bare("item:coins-001", "coins");
        coins.apply_stackable_role(&StackableSpec {
            count: 20,
            max_stack: 99,
        });
        coins.location = Some(purse.id.clone());
        purse.set_property_list("contents", vec![coins.id.clone()]);

        let mut backpack = bare("item:backpack-001", "backpack");
        backpack.apply_container_role(&ContainerSpec {
            capacity: 5,
            max_weight: None,
            max_volume: None,
            wearable: true,
            wear_slot: Some("torso".to_string()),
        });

        player.set_body_slot("right_hand", Some(purse.id.clone()));
        player.set_body_slot("left_hand", Some(coins.id.clone()));
        player.set_body_slot("torso", Some(backpack.id.clone()));

        let mut objects = HashMap::new();
        objects.insert(coins.id.clone(), coins);
        objects.insert(purse.id.clone(), purse);
        objects.insert(backpack.id.clone(), backpack);
        objects.insert(player.id.clone(), player.clone());

        let summary = format_look_self_summary(&player, &objects, &anatomy);
        assert_eq!(
            summary,
            "You are holding: purse, 20 coins. Wearing: backpack."
        );
        assert!(!summary.contains("right_hand"));
        assert!(!summary.contains("inside"));
    }

    #[test]
    fn look_self_dedupes_two_handed_item() {
        let anatomy = anatomy();
        let mut player = bare("player:hero-001", "Hero");
        player.init_creature_role(anatomy.player_template("default").unwrap());

        let sword = bare("item:sword-001", "sword");
        let sword_id = sword.id.clone();
        player.set_body_slot("left_hand", Some(sword_id.clone()));
        player.set_body_slot("right_hand", Some(sword_id));

        let mut objects = HashMap::new();
        objects.insert(sword.id.clone(), sword);
        objects.insert(player.id.clone(), player.clone());

        let summary = format_look_self_summary(&player, &objects, &anatomy);
        assert_eq!(summary, "You are holding: sword.");
    }
}