//! Gate event handlers (`on_unlock`, `on_open`, etc.) for doors and containers.

use crate::object::Object;

/// Run all handlers registered on `gate` for `event_name` and return player-facing lines.
pub fn run_gate_event_handlers(gate: &Object, event_name: &str) -> Vec<String> {
    gate.event_handlers
        .get(event_name)
        .map(|handlers| {
            handlers
                .iter()
                .filter_map(|behavior| format_gate_behavior_line(gate, &behavior.code))
                .collect()
        })
        .unwrap_or_default()
}

fn format_gate_behavior_line(gate: &Object, code: &str) -> Option<String> {
    let code = code.trim();
    if code.is_empty() {
        return None;
    }
    let display = gate.name.to_lowercase();
    if let Some((verb, text)) = code.split_once(char::is_whitespace) {
        let text = text.trim();
        if !text.is_empty() {
            return match verb.to_ascii_lowercase().as_str() {
                "say" | "narrate" | "message" => Some(text.to_string()),
                "emote" => Some(format!("The {display} {text}")),
                _ => Some(code.to_string()),
            };
        }
    }
    Some(code.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{Behavior, PermissionFlags};
    use std::collections::HashMap;

    fn gate_with_handlers() -> Object {
        let mut gate = Object {
            id: crate::object::ObjectId::new("item:door-001"),
            name: "Wooden Door".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: crate::object::ObjectId::new("player:admin-001"),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        gate.add_event_handler(
            "on_unlock".to_string(),
            Behavior {
                code: "narrate The lock clicks free.".to_string(),
                permissions: PermissionFlags::EVERYONE,
            },
        );
        gate.add_event_handler(
            "on_open".to_string(),
            Behavior {
                code: "emote swings open".to_string(),
                permissions: PermissionFlags::EVERYONE,
            },
        );
        gate
    }

    #[test]
    fn gate_events_fire_in_handler_order() {
        let gate = gate_with_handlers();
        let unlock = run_gate_event_handlers(&gate, "on_unlock");
        assert_eq!(unlock, vec!["The lock clicks free."]);

        let open = run_gate_event_handlers(&gate, "on_open");
        assert_eq!(open, vec!["The wooden door swings open"]);
    }

    #[test]
    fn unknown_event_returns_empty() {
        let gate = gate_with_handlers();
        assert!(run_gate_event_handlers(&gate, "on_close").is_empty());
    }
}
