//! Player-facing output for the `read` command.

use crate::object::Object;

/// Text content to display when the player reads an object.
pub fn effective_read_text(obj: &Object) -> Option<String> {
    let written = obj.write_text().filter(|text| !text.trim().is_empty());
    if written.is_some() {
        return written;
    }
    obj.read_text().filter(|text| !text.trim().is_empty())
}

/// Natural message after a successful `read`.
pub fn format_read_message(obj: &Object) -> Option<String> {
    let text = effective_read_text(obj)?;
    let name = obj.name.to_lowercase();
    Some(format!("You read the {name}:\n\n{text}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{Object, ObjectId, PermissionFlags, ReadableSpec};
    use std::collections::HashMap;

    fn bare(id: &str, name: &str) -> Object {
        Object {
            id: ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new("player:admin-001"),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        }
    }

    #[test]
    fn format_read_message_uses_read_text() {
        let mut note = bare("item:note-001", "folded note");
        note.apply_readable_role(&ReadableSpec {
            text: "Supplies within — mind the dark.".to_string(),
            writable: false,
        });

        assert_eq!(
            format_read_message(&note).unwrap(),
            "You read the folded note:\n\nSupplies within — mind the dark."
        );
    }

    #[test]
    fn effective_read_text_prefers_write_text() {
        let mut letter = bare("item:letter-001", "letter");
        letter.apply_readable_role(&ReadableSpec {
            text: "A blank sheet.".to_string(),
            writable: true,
        });
        letter.set_write_text("Meet me at dusk.");

        assert_eq!(
            effective_read_text(&letter).as_deref(),
            Some("Meet me at dusk.")
        );
    }
}
