//! Brief carried/worn summaries for `look self`.

use std::collections::{HashMap, HashSet};

use crate::mudl::AnatomyRegistry;
use crate::object::{Object, ObjectId};

fn grasp_slot_sort_key(name: &str) -> u8 {
    match name {
        "right_hand" => 0,
        "left_hand" => 1,
        _ => 2,
    }
}

/// Display name for a held or worn item (no weight or slot detail).
fn look_item_name(obj: &Object) -> String {
    if obj.is_stackable() && obj.stack_count() > 1 {
        format!("{} {}", obj.stack_count(), obj.name)
    } else {
        obj.name.clone()
    }
}

fn indefinite_article(word: &str) -> &'static str {
    match word.chars().next().map(|c| c.to_ascii_lowercase()) {
        Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    }
}

/// Join item names: first gets a/an, rest are bare, all chained with "and".
///
/// `["Rusty Sword", "Wooden Sword"]` → `a Rusty Sword and Wooden Sword`
fn phrase_with_leading_article(items: &[String]) -> String {
    match items.len() {
        0 => String::new(),
        1 => format!("{} {}", indefinite_article(&items[0]), items[0]),
        _ => {
            let first = format!("{} {}", indefinite_article(&items[0]), items[0]);
            format!("{first} and {}", items[1..].join(" and "))
        }
    }
}

/// Natural `look self` sentence: held grasp items and worn gear only.
///
/// Example: `You are holding a Rusty Sword and Wooden Sword and wearing a backpack.`
pub fn format_look_self_summary(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> String {
    let plan_name = player
        .body_plan_name()
        .unwrap_or_else(|| "human".to_string());
    let Some(plan) = anatomy.body_plan(&plan_name) else {
        return "You aren't holding or wearing anything.".to_string();
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
        }
        let Some(obj) = objects.get(&item_id) else {
            continue;
        };
        if obj.is_active() {
            holding.push(look_item_name(obj));
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
            wearing.push(look_item_name(obj));
        }
    }

    match (holding.is_empty(), wearing.is_empty()) {
        (true, true) => "You aren't holding or wearing anything.".to_string(),
        (false, true) => format!(
            "You are holding {}.",
            phrase_with_leading_article(&holding)
        ),
        (true, false) => format!(
            "You are wearing {}.",
            phrase_with_leading_article(&wearing)
        ),
        (false, false) => format!(
            "You are holding {} and wearing {}.",
            phrase_with_leading_article(&holding),
            phrase_with_leading_article(&wearing)
        ),
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
    fn look_self_holding_and_wearing_natural_sentence() {
        let anatomy = anatomy();
        let mut player = bare("player:hero-001", "Hero");
        player.init_creature_role(anatomy.player_template("default").unwrap());

        let mut rusty = bare("item:rusty-001", "Rusty Sword");
        let mut wooden = bare("item:wooden-001", "Wooden Sword");
        let mut backpack = bare("item:backpack-001", "backpack");
        backpack.apply_container_role(&ContainerSpec {
            capacity: 5,
            max_weight: None,
            max_volume: None,
            wearable: true,
            wear_slot: Some("torso".to_string()),
        });

        player.set_body_slot("right_hand", Some(rusty.id.clone()));
        player.set_body_slot("left_hand", Some(wooden.id.clone()));
        player.set_body_slot("torso", Some(backpack.id.clone()));

        let mut objects = HashMap::new();
        objects.insert(rusty.id.clone(), rusty);
        objects.insert(wooden.id.clone(), wooden);
        objects.insert(backpack.id.clone(), backpack);
        objects.insert(player.id.clone(), player.clone());

        let summary = format_look_self_summary(&player, &objects, &anatomy);
        assert_eq!(
            summary,
            "You are holding a Rusty Sword and Wooden Sword and wearing a backpack."
        );
    }

    #[test]
    fn look_self_lists_held_items_not_nested_contents() {
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

        player.set_body_slot("right_hand", Some(purse.id.clone()));

        let mut objects = HashMap::new();
        objects.insert(coins.id.clone(), coins);
        objects.insert(purse.id.clone(), purse);
        objects.insert(player.id.clone(), player.clone());

        let summary = format_look_self_summary(&player, &objects, &anatomy);
        assert_eq!(summary, "You are holding a purse.");
        assert!(!summary.contains("20 coins"));
        assert!(!summary.contains("right_hand"));
    }

    #[test]
    fn look_self_dedupes_two_handed_item() {
        let anatomy = anatomy();
        let mut player = bare("player:hero-001", "Hero");
        player.init_creature_role(anatomy.player_template("default").unwrap());

        let sword = bare("item:sword-001", "Iron Sword");
        let sword_id = sword.id.clone();
        player.set_body_slot("left_hand", Some(sword_id.clone()));
        player.set_body_slot("right_hand", Some(sword_id));

        let mut objects = HashMap::new();
        objects.insert(sword.id.clone(), sword);
        objects.insert(player.id.clone(), player.clone());

        let summary = format_look_self_summary(&player, &objects, &anatomy);
        assert_eq!(summary, "You are holding an Iron Sword.");
    }

    #[test]
    fn look_self_uses_an_before_vowel() {
        assert_eq!(
            phrase_with_leading_article(&["apple".to_string()]),
            "an apple"
        );
        assert_eq!(
            phrase_with_leading_article(&["Rusty Sword".to_string(), "apple".to_string()]),
            "a Rusty Sword and apple"
        );
    }
}