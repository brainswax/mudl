//! Container and stackable item presentation for look/examine.

use std::collections::HashMap;

use crate::object::{Object, ObjectId};

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
    if labels.is_empty() {
        return String::new();
    }
    format!(
        "Inside the {}: {}",
        container.name.to_lowercase(),
        labels.join("; ")
    )
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
