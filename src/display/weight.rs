//! Player- and builder-facing weight presentation.

use std::collections::HashMap;

use crate::display::stackable::{
    format_examine_stack_weight, format_examine_stackable_fallback,
};
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

/// Player `examine` output for a non-container item (no redundant name line).
pub fn format_examine_item_player(obj: &Object) -> String {
    let mut lines = Vec::new();
    if let Some(desc) = obj.get_description() {
        lines.push(desc);
    }
    if let Some(weight) = format_examine_stack_weight(obj) {
        lines.push(weight);
    }
    if lines.is_empty() {
        lines.push(format_examine_stackable_fallback(obj));
    }
    lines.join("\n")
}

/// In-game weight line for legacy call sites (players only).
pub fn format_weight_examine_player(obj: &Object, objects: &HashMap<ObjectId, Object>) -> Option<String> {
    if obj.object_type() != "player" {
        return None;
    }
    let mut parts = Vec::new();
    let carried = player_carried_weight(obj, objects);
    if carried > 0.0 {
        parts.push(format!(
            "You are carrying {} weight.",
            format_weight_amount(carried)
        ));
    }
    if let Some(max) = obj.get_int_property("max_weight") {
        parts.push(player_capacity_message(max));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
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
    fn examine_item_without_description_uses_natural_sentence() {
        let sword = bare("item:sword-001", "Rusty Sword");
        assert_eq!(format_examine_item_player(&sword), "It is a Rusty Sword.");
    }

    #[test]
    fn examine_gold_bar_stack_shows_total_weight() {
        let mut bar = bare("item:bar-001", "gold bar");
        bar.set_property_int("weight", 10);
        bar.apply_stackable_role(&StackableSpec {
            count: 10,
            max_stack: 99,
        });

        assert_eq!(
            format_examine_item_player(&bar),
            "The stack of 10 gold bars weighs 100 in total."
        );
    }

    #[test]
    fn examine_item_shows_description_and_stack_weight() {
        let mut coins = bare("item:coins-001", "coins");
        coins.set_property_int("weight", 1);
        coins.apply_stackable_role(&StackableSpec {
            count: 20,
            max_stack: 99,
        });
        coins.add_property(crate::object::Property {
            name: "description".to_string(),
            value: crate::object::Value::String("Gold coins glint.".to_string()),
            permissions: PermissionFlags::EVERYONE,
            behavior: None,
        });

        let output = format_examine_item_player(&coins);
        assert_eq!(
            output,
            "Gold coins glint.\nThe stack of 20 coins weighs 20 in total."
        );
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