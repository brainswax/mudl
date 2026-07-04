//! Player- and builder-facing weight presentation.

use std::collections::HashMap;

use crate::object::{
    format_weight_amount, is_unlimited_weight, player_carried_weight, Object, ObjectId,
};

fn player_capacity_message(max: i64) -> String {
    if is_unlimited_weight(max) {
        "You have unlimited carrying capacity.".to_string()
    } else {
        format!("You can carry up to {max} weight.")
    }
}

fn container_capacity_message(name: &str, max: i64) -> String {
    if is_unlimited_weight(max) {
        format!("The {name} has unlimited carrying capacity.")
    } else {
        format!("The {name} can hold up to {max} weight.")
    }
}

/// In-game weight line for `examine` (player mode).
pub fn format_weight_examine_player(obj: &Object, objects: &HashMap<ObjectId, Object>) -> Option<String> {
    if obj.object_type() == "player" {
        let mut parts = Vec::new();
        let carried = player_carried_weight(obj, objects);
        if carried > 0.0 {
            parts.push(format!("You are carrying {carried} weight."));
        }
        if let Some(max) = obj.get_int_property("max_weight") {
            parts.push(player_capacity_message(max));
        }
        return if parts.is_empty() {
            None
        } else {
            Some(parts.join(" "))
        };
    }

    if obj.is_container() {
        let current = obj.contents_weight(objects);
        let name = obj.name.to_lowercase();
        let mut parts = Vec::new();
        if current > 0.0 || obj.container_max_weight().is_some() {
            parts.push(format!(
                "The {name} holds {} weight.",
                format_weight_amount(current)
            ));
        }
        if let Some(max) = obj.container_max_weight() {
            parts.push(container_capacity_message(&name, max));
        }
        return if parts.is_empty() {
            None
        } else {
            Some(parts.join(" "))
        };
    }

    if obj.is_stackable() && obj.stack_count() > 1 {
        return Some(format!("They weigh {}.", format_weight_amount(obj.weight())));
    }

    let w = obj.weight();
    if w > 1.0 || (w > 0.0 && obj.get_numeric_property("weight").is_some()) {
        return Some(format!("It weighs {}.", format_weight_amount(w)));
    }

    None
}

/// Builder weight lines for `@examine`.
pub fn format_weight_examine_builder(
    obj: &Object,
    objects: &HashMap<ObjectId, Object>,
) -> Vec<String> {
    let mut lines = Vec::new();

    if obj.is_stackable() {
        lines.push(format!(
            "Weight: {} ({} × {})",
            obj.weight(),
            obj.unit_weight(),
            obj.stack_count()
        ));
    } else {
        lines.push(format!("Weight: {}", obj.weight()));
    }

    if obj.is_container() {
        let contents = obj.contents_weight(objects);
        match obj.container_max_weight() {
            Some(max) if is_unlimited_weight(max) => {
                lines.push(format!("Contents weight: {contents}"));
                lines.push("Max weight: unlimited".to_string());
            }
            Some(max) => {
                lines.push(format!("Contents weight: {contents}/{max}"));
            }
            None => lines.push(format!("Contents weight: {contents}")),
        }
        let total = obj.total_weight(objects);
        if total != obj.weight() {
            lines.push(format!("Total weight: {total}"));
        }
    }

    if obj.object_type() == "player" {
        lines.push(format!(
            "Carried weight: {}",
            player_carried_weight(obj, objects)
        ));
        if let Some(max) = obj.get_int_property("max_weight") {
            if is_unlimited_weight(max) {
                lines.push("Max weight: unlimited".to_string());
            } else {
                lines.push(format!("Max weight: {max}"));
            }
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mudl::load_module;
    use crate::object::{ContainerSpec, PermissionFlags, StackableSpec, UNLIMITED_WEIGHT};

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
    fn player_examine_container_shows_capacity_message() {
        let mut purse = bare("item:purse-001", "purse");
        purse.set_property_int("weight", 1);
        purse.apply_container_role(&ContainerSpec {
            capacity: 3,
            max_weight: Some(10),
            max_volume: None,
            wearable: false,
            wear_slot: None,
        });

        let mut coins = bare("item:coins-001", "coins");
        coins.set_property_int("weight", 1);
        coins.apply_stackable_role(&StackableSpec {
            count: 2,
            max_stack: 99,
        });
        purse.set_property_list("contents", vec![coins.id.clone()]);

        let mut objects = HashMap::new();
        objects.insert(coins.id.clone(), coins);
        objects.insert(purse.id.clone(), purse.clone());

        let line = format_weight_examine_player(&purse, &objects).unwrap();
        assert!(line.contains("The purse holds 2 weight."));
        assert!(line.contains("The purse can hold up to 10 weight."));
    }

    #[test]
    fn player_examine_shows_carry_capacity() {
        let anatomy = load_module("modules/default")
            .unwrap()
            .active_world()
            .unwrap()
            .anatomy
            .clone();
        let mut player = bare("player:hero-001", "Hero");
        player.init_creature_role(anatomy.player_template("default").unwrap());

        let line = format_weight_examine_player(&player, &HashMap::new()).unwrap();
        assert_eq!(line, "You can carry up to 100 weight.");
    }

    #[test]
    fn unlimited_container_examine_message() {
        let mut bag = bare("item:bag-001", "bag");
        bag.apply_container_role(&ContainerSpec {
            capacity: 10,
            max_weight: Some(UNLIMITED_WEIGHT),
            max_volume: None,
            wearable: false,
            wear_slot: None,
        });

        let line = format_weight_examine_player(&bag, &HashMap::new()).unwrap();
        assert!(line.contains("The bag holds 0 weight."));
        assert!(line.contains("unlimited carrying capacity"));
    }

    #[test]
    fn builder_examine_shows_stack_breakdown() {
        let mut coins = bare("item:coins-001", "coins");
        coins.set_property_int("weight", 2);
        coins.apply_stackable_role(&StackableSpec {
            count: 10,
            max_stack: 99,
        });

        let objects = HashMap::new();
        let lines = format_weight_examine_builder(&coins, &objects);
        assert_eq!(lines[0], "Weight: 20 (2 × 10)");
    }

    #[test]
    fn builder_player_shows_max_weight() {
        let anatomy = load_module("modules/default")
            .unwrap()
            .active_world()
            .unwrap()
            .anatomy
            .clone();
        let mut player = bare("player:hero-001", "Hero");
        player.init_creature_role(anatomy.player_template("default").unwrap());

        let lines = format_weight_examine_builder(&player, &HashMap::new());
        assert!(lines.iter().any(|l| l == "Max weight: 100"));
    }
}