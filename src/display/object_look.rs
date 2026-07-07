//! In-character `look` output for objects (no leading name line).

use std::collections::HashMap;

use crate::object::{Object, ObjectId};

use super::container::format_look_container_player;
use super::stackable::format_look_stackable_sentence;

/// Brief `look` at a non-container item — description or a short natural sentence.
pub fn format_look_item_player(obj: &Object) -> String {
    if let Some(desc) = obj.get_description() {
        return desc;
    }

    format_look_stackable_sentence(obj)
}

/// Brief `look` at any object (container, item, etc.).
pub fn format_look_object_player(obj: &Object, objects: &HashMap<ObjectId, Object>) -> String {
    if obj.is_container() {
        format_look_container_player(obj, objects)
    } else {
        format_look_item_player(obj)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn look_item_without_description_uses_natural_sentence() {
        let mut coins = bare("item:coins-001", "coins");
        coins.apply_stackable_role(&StackableSpec {
            count: 20,
            max_stack: 99,
        });
        assert_eq!(format_look_item_player(&coins), "There are 20 coins.");
    }

    #[test]
    fn look_gold_bar_stack_pluralizes_name() {
        let mut bar = bare("item:bar-001", "gold bar");
        bar.apply_stackable_role(&StackableSpec {
            count: 10,
            max_stack: 99,
        });
        assert_eq!(format_look_item_player(&bar), "There are 10 gold bars.");
    }
}
