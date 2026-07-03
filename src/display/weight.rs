//! Player- and builder-facing weight presentation.

use std::collections::HashMap;

use crate::object::{player_carried_weight, Object, ObjectId};

/// In-game weight line for `examine` (player mode).
pub fn format_weight_examine_player(obj: &Object, objects: &HashMap<ObjectId, Object>) -> Option<String> {
    if obj.is_container() {
        let current = obj.contents_weight(objects);
        let name = obj.name.to_lowercase();
        if let Some(max) = obj.container_max_weight() {
            return Some(format!("The {name} weighs {current}/{max}."));
        }
        let total = obj.total_weight(objects);
        if total > 0 {
            return Some(format!("The {name} weighs {total}."));
        }
        return None;
    }

    if obj.is_stackable() && obj.stack_count() > 1 {
        return Some(format!("They weigh {}.", obj.weight()));
    }

    let w = obj.weight();
    if w > 1 || (w > 0 && obj.get_int_property("weight").is_some()) {
        return Some(format!("It weighs {w}."));
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
        if let Some(max) = obj.container_max_weight() {
            lines.push(format!("Contents weight: {contents}/{max}"));
        } else {
            lines.push(format!("Contents weight: {contents}"));
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
    }

    lines
}

/// Summary line for `examine self` when the player is carrying weight.
pub fn format_carried_weight_summary(
    player: &Object,
    objects: &HashMap<ObjectId, Object>,
) -> Option<String> {
    let total = player_carried_weight(player, objects);
    if total > 0 {
        Some(format!("You are carrying {total} weight in total."))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{ContainerSpec, PermissionFlags, StackableSpec};

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
    fn player_examine_container_shows_current_max() {
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
        assert_eq!(line, "The purse weighs 2/10.");
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
}