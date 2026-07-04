//! Container and stackable item presentation for look/examine.

use std::collections::HashMap;

use crate::object::{
    format_weight_amount, is_unlimited_weight, Object, ObjectId,
};

use super::grammar::join_natural_list;
pub use super::stackable::format_stackable_label;

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

/// In-character `look` at a container (contents only, no stats).
///
/// Example: `The backpack contains 20 coins.`
pub fn format_look_container_player(
    container: &Object,
    objects: &HashMap<ObjectId, Object>,
) -> String {
    let labels = container_content_labels(container, objects);
    let name = container.name.to_lowercase();
    if labels.is_empty() {
        return format!("The {name} is empty.");
    }
    format!(
        "The {name} contains {}.",
        join_natural_list(&labels)
    )
}

/// Legacy alias — prefer [`format_look_container_player`].
pub fn format_inside_container(container: &Object, objects: &HashMap<ObjectId, Object>) -> String {
    format_look_container_player(container, objects)
}

fn container_used_slots(container: &Object, objects: &HashMap<ObjectId, Object>) -> u32 {
    container
        .container_contents()
        .into_iter()
        .filter(|id| objects.get(id).is_some_and(|obj| obj.is_active()))
        .count() as u32
}

/// In-character `examine` at a container — one short paragraph, IRC-friendly.
///
/// Example: `The backpack contains 20 coins and has a capacity of 1/20. It is carrying 13/100 weight.`
pub fn format_examine_container_player(
    container: &Object,
    objects: &HashMap<ObjectId, Object>,
) -> String {
    let name = container.name.to_lowercase();
    let labels = container_content_labels(container, objects);
    let used = container_used_slots(container, objects);
    let max_slots = container.container_capacity();
    let contents_w = container.contents_weight(objects);

    let opener = if labels.is_empty() {
        format!("The {name} is empty")
    } else {
        format!(
            "The {name} contains {}",
            join_natural_list(&labels)
        )
    };

    let mut text = format!("{opener} and has a capacity of {used}/{max_slots}.");

    if let Some(max) = container.container_max_weight() {
        let weight_line = if is_unlimited_weight(max) {
            format!(
                "It is carrying {}/unlimited weight.",
                format_weight_amount(contents_w)
            )
        } else {
            format!(
                "It is carrying {}/{} weight.",
                format_weight_amount(contents_w),
                format_weight_amount(max as f64)
            )
        };
        text.push(' ');
        text.push_str(&weight_line);
    } else if contents_w > 0.0 {
        text.push(' ');
        text.push_str(&format!(
            "It is carrying {} weight.",
            format_weight_amount(contents_w)
        ));
    }

    text
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

        let line = format_look_container_player(&backpack, &HashMap::new());
        assert_eq!(line, "The backpack is empty.");
    }

    #[test]
    fn look_container_uses_contains_phrasing() {
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

        assert_eq!(
            format_look_container_player(&purse, &objects),
            "The purse contains 20 coins."
        );
    }

    #[test]
    fn examine_container_natural_paragraph() {
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
            "The backpack contains 20 coins and has a capacity of 1/20. It is carrying 20/100 weight."
        );
    }
}
