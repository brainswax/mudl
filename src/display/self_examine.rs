//! Concise `examine self` output — MOO-style equipment summary without property dumps.

use std::collections::{HashMap, HashSet};

use crate::display::format_stackable_label;
use crate::mudl::{slot_display_name, AnatomyRegistry, BodyPlan};
use crate::object::{format_weight_amount, is_unlimited_weight, player_carried_weight, Object, ObjectId};

fn grasp_slot_sort_key(name: &str) -> u8 {
    match name {
        "right_hand" => 0,
        "left_hand" => 1,
        _ => 2,
    }
}

/// Placement label for equipped items (torso → "back", per common MUD convention).
pub fn equipment_placement_label(slot: &str) -> String {
    match slot {
        "torso" => "back".to_string(),
        other => slot_display_name(other),
    }
}

struct EquippedEntry {
    item_name: String,
    placement: String,
    sort_key: (u8, u8, String),
}

fn equipped_entries(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
    plan: &BodyPlan,
) -> Vec<EquippedEntry> {
    let mut entries = Vec::new();
    let left = player.body_slot_item("left_hand");
    let right = player.body_slot_item("right_hand");

    if let (Some(left_id), Some(right_id)) = (&left, &right) {
        if left_id == right_id {
            if let Some(obj) = objects.get(left_id) {
                if obj.is_active() {
                    entries.push(EquippedEntry {
                        item_name: format_stackable_label(obj),
                        placement: "both hands".to_string(),
                        sort_key: (0, 0, "both".to_string()),
                    });
                }
            }
        }
    }

    let mut grasp_slots = plan.grasp_slots();
    grasp_slots.sort_by_key(|s| grasp_slot_sort_key(&s.name));

    let mut seen_grasp = HashSet::new();
    for slot in grasp_slots {
        let Some(item_id) = player.body_slot_item(&slot.name) else {
            continue;
        };
        if entries.iter().any(|e| e.placement == "both hands") {
            continue;
        }
        if !seen_grasp.insert(item_id.clone()) {
            continue;
        }
        let Some(obj) = objects.get(&item_id) else {
            continue;
        };
        if !obj.is_active() {
            continue;
        }
        entries.push(EquippedEntry {
            item_name: format_stackable_label(obj),
            placement: equipment_placement_label(&slot.name),
            sort_key: (0, grasp_slot_sort_key(&slot.name), slot.name.clone()),
        });
    }

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
        if !obj.is_active() {
            continue;
        };
        entries.push(EquippedEntry {
            item_name: format_stackable_label(obj),
            placement: equipment_placement_label(&slot.name),
            sort_key: (1, 0, slot.name.clone()),
        });
    }

    entries.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));
    entries
}

fn format_equipped_line(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
    plan: &BodyPlan,
) -> String {
    let entries = equipped_entries(player, objects, plan);
    if entries.is_empty() {
        return "Equipped: nothing.".to_string();
    }
    let parts: Vec<String> = entries
        .iter()
        .map(|e| format!("{} ({})", e.item_name, e.placement))
        .collect();
    format!("Equipped: {}.", parts.join(", "))
}

fn format_carrying_line(player: &Object, objects: &HashMap<ObjectId, Object>) -> String {
    let carried = player_carried_weight(player, objects);
    match player.get_int_property("max_weight") {
        Some(max) if is_unlimited_weight(max) => {
            format!(
                "Carrying: {}/unlimited weight.",
                format_weight_amount(carried)
            )
        }
        Some(max) => format!(
            "Carrying: {}/{} weight.",
            format_weight_amount(carried),
            format_weight_amount(max as f64)
        ),
        None => format!("Carrying: {} weight.", format_weight_amount(carried)),
    }
}

/// Concise player self-examination (`examine self`).
///
/// Example:
/// ```text
/// Admin (human)
/// Equipped: Rusty Sword (right hand), Wooden Sword (left hand), backpack (back)
/// Carrying: 12/100 weight.
/// ```
pub fn format_examine_self(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> String {
    let creature = player
        .body_plan_name()
        .unwrap_or_else(|| "human".to_string());

    let mut lines = vec![format!("{} ({})", player.name, creature)];

    if let Some(plan) = anatomy.body_plan(&creature) {
        lines.push(format_equipped_line(player, objects, plan));
    } else {
        lines.push("Equipped: nothing.".to_string());
    }

    lines.push(format_carrying_line(player, objects));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mudl::load_module;
    use crate::object::{ContainerSpec, PermissionFlags};

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
    fn examine_self_shows_concise_equipment_summary() {
        let anatomy = anatomy();
        let mut player = bare("player:hero-001", "Admin");
        player.init_creature_role(anatomy.player_template("default").unwrap());
        player.set_property_int("max_weight", 100);

        let mut rusty = bare("item:rusty-001", "Rusty Sword");
        rusty.set_property_string("hand_slot", "right");

        let mut wooden = bare("item:wooden-001", "Wooden Sword");
        wooden.set_property_string("hand_slot", "left");

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

        let output = format_examine_self(&player, &objects, &anatomy);
        assert_eq!(
            output,
            "Admin (human)\n\
             Equipped: Rusty Sword (right hand), Wooden Sword (left hand), backpack (back).\n\
             Carrying: 3/100 weight."
        );
    }

    #[test]
    fn examine_self_empty_equipment() {
        let anatomy = anatomy();
        let mut player = bare("player:hero-001", "Admin");
        player.init_creature_role(anatomy.player_template("default").unwrap());
        player.set_property_int("max_weight", 100);

        let output = format_examine_self(&player, &HashMap::new(), &anatomy);
        assert!(output.contains("Admin (human)"));
        assert!(output.contains("Equipped: nothing."));
        assert!(output.contains("Carrying: 0/100 weight."));
    }
}