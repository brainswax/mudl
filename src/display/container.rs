//! Container and stackable item presentation for look/examine.

use std::collections::HashMap;

use crate::object::{
    format_weight_amount, is_unlimited_weight, Object, ObjectId,
};

/// Player-facing label for an item, with stack count when stackable.
pub fn format_stackable_label(item: &Object) -> String {
    if item.is_stackable() && item.stack_count() > 1 {
        format!("{} {}", item.stack_count(), item.name)
    } else {
        item.name.clone()
    }
}

/// Labels for each object in a container's `contents` list.
pub fn container_content_labels(
    container: &Object,
    objects: &HashMap<ObjectId, Object>,
) -> Vec<String> {
    let mut labels = Vec::new();
    for id in container.container_contents() {
        let Some(item) = objects.get(&id) else {
            continue;
        };
        if !item.is_active() {
            continue;
        }
        labels.push(format_stackable_label(item));
    }
    labels
}

/// Player-mode line for container contents (e.g. "Inside the purse: 20 coins").
pub fn format_inside_container(container: &Object, objects: &HashMap<ObjectId, Object>) -> String {
    let labels = container_content_labels(container, objects);
    let name = container.name.to_lowercase();
    if labels.is_empty() {
        return format!("The {name} is empty.");
    }
    format!("Inside the {name}: {}", labels.join(", "))
}

fn container_used_slots(container: &Object, objects: &HashMap<ObjectId, Object>) -> u32 {
    container
        .container_contents()
        .into_iter()
        .filter(|id| objects.get(id).is_some_and(|obj| obj.is_active()))
        .count() as u32
}

fn format_container_capacity_summary(
    container: &Object,
    objects: &HashMap<ObjectId, Object>,
) -> String {
    let name = container.name.to_lowercase();
    let used = container_used_slots(container, objects);
    let max_slots = container.container_capacity();
    let capacity_part = format!("a capacity of {used}/{max_slots}");
    let contents_w = container.contents_weight(objects);

    let weight_part = match container.container_max_weight() {
        Some(max) if is_unlimited_weight(max) => format!(
            "is carrying {}/unlimited weight",
            format_weight_amount(contents_w)
        ),
        Some(max) => format!(
            "is carrying {}/{} weight",
            format_weight_amount(contents_w),
            format_weight_amount(max as f64)
        ),
        None if contents_w > 0.0 => format!(
            "is carrying {} weight",
            format_weight_amount(contents_w)
        ),
        None => String::new(),
    };

    if weight_part.is_empty() {
        format!("The {name} has {capacity_part}.")
    } else {
        format!("The {name} has {capacity_part} and {weight_part}.")
    }
}

/// Player `examine` output for a container: contents first, then capacity/weight.
pub fn format_examine_container_player(
    container: &Object,
    objects: &HashMap<ObjectId, Object>,
) -> String {
    vec![
        format_inside_container(container, objects),
        format_container_capacity_summary(container, objects),
    ]
    .join("\n")
}

/// Builder-mode summary of container contents.
pub fn format_container_contents_builder(
    container: &Object,
    objects: &HashMap<ObjectId, Object>,
) -> String {
    let labels = container_content_labels(container, objects);
    if labels.is_empty() {
        "Contents: (empty)".to_string()
    } else {
        format!("Contents: {}", labels.join(", "))
    }
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
            owner: ObjectId::new("player:admin-001"),
            permissions: PermissionFlags::OWNER,
            properties: Default::default(),
            verbs: Default::default(),
            event_handlers: Default::default(),
            is_deleted: false,
            deleted_at: None,
        }
    }

    #[test]
    fn stackable_label_includes_count() {
        let mut coins = bare("item:coins-001", "coins");
        coins.apply_stackable_role(&crate::object::StackableSpec {
            count: 20,
            max_stack: 99,
        });
        assert_eq!(format_stackable_label(&coins), "20 coins");
    }

    #[test]
    fn stackable_label_singular_omits_count() {
        let mut coin = bare("item:coin-001", "coin");
        coin.apply_stackable_role(&crate::object::StackableSpec {
            count: 1,
            max_stack: 99,
        });
        assert_eq!(format_stackable_label(&coin), "coin");
    }

    #[test]
    fn inside_empty_container() {
        let mut backpack = bare("item:backpack-001", "backpack");
        backpack.apply_container_role(&crate::object::ContainerSpec {
            capacity: 20,
            max_weight: Some(100),
            max_volume: None,
            wearable: true,
            wear_slot: Some("torso".to_string()),
        });

        let line = format_inside_container(&backpack, &HashMap::new());
        assert_eq!(line, "The backpack is empty.");
    }

    #[test]
    fn examine_container_shows_contents_then_capacity() {
        let mut backpack = bare("item:backpack-001", "backpack");
        backpack.apply_container_role(&crate::object::ContainerSpec {
            capacity: 20,
            max_weight: Some(100),
            max_volume: None,
            wearable: true,
            wear_slot: Some("torso".to_string()),
        });
        let mut coins = bare("item:coins-001", "coins");
        coins.set_property_int("weight", 1);
        coins.apply_stackable_role(&crate::object::StackableSpec {
            count: 20,
            max_stack: 99,
        });
        backpack.set_property_list("contents", vec![coins.id.clone()]);

        let mut objects = HashMap::new();
        objects.insert(coins.id.clone(), coins);

        let output = format_examine_container_player(&backpack, &objects);
        assert_eq!(
            output,
            "Inside the backpack: 20 coins\n\
             The backpack has a capacity of 1/20 and is carrying 20/100 weight."
        );
    }

    #[test]
    fn inside_container_lists_stackables() {
        let mut purse = bare("item:purse-001", "purse");
        purse.apply_container_role(&crate::object::ContainerSpec {
            capacity: 3,
            max_weight: Some(10),
            max_volume: None,
            wearable: true,
            wear_slot: Some("torso".to_string()),
        });
        let mut coins = bare("item:coins-001", "coins");
        coins.apply_stackable_role(&crate::object::StackableSpec {
            count: 20,
            max_stack: 99,
        });
        purse.set_property_list("contents", vec![coins.id.clone()]);

        let mut objects = HashMap::new();
        objects.insert(coins.id.clone(), coins);

        let line = format_inside_container(&purse, &objects);
        assert_eq!(line, "Inside the purse: 20 coins");
    }
}
