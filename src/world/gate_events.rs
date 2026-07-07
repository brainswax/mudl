//! Gate event handlers (`on_unlock`, `on_open`, etc.) for doors and containers.

use crate::object::Object;
use crate::world::events::run_event_handlers_on;

/// Run all handlers registered on `gate` for `event_name` and return player-facing lines.
pub fn run_gate_event_handlers(gate: &Object, event_name: &str) -> Vec<String> {
    run_event_handlers_on(gate, event_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::PermissionFlags;
    use crate::world::events::attach_triggers;
    use crate::mudl::TriggerDef;
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
        attach_triggers(
            &mut gate,
            &[
                TriggerDef {
                    event: "on_unlock".to_string(),
                    code: "narrate The lock clicks free.".to_string(),
                },
                TriggerDef {
                    event: "on_open".to_string(),
                    code: "emote swings open".to_string(),
                },
            ],
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