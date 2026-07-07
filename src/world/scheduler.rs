//! Central event scheduler — shared ticks for periodic/timer subscribers.

use std::collections::HashMap;

use crate::object::{Object, ObjectId};

fn tick_property(event: &str) -> String {
    format!("scheduler_tick_{event}")
}

/// Read the current tick for `scope_id` + `event` without advancing.
pub fn current_tick(
    scope_id: &ObjectId,
    event: &str,
    objects: &HashMap<ObjectId, Object>,
) -> u64 {
    objects
        .get(scope_id)
        .and_then(|scope| scope.get_int_property(&tick_property(event)))
        .unwrap_or(0)
        .max(0) as u64
}

/// Advance and return the tick for `scope_id` + `event`.
pub fn advance_tick(
    scope_id: &ObjectId,
    event: &str,
    objects: &mut HashMap<ObjectId, Object>,
) -> u64 {
    let tick = current_tick(scope_id, event, objects) + 1;
    if let Some(scope) = objects.get_mut(scope_id) {
        scope.set_property_int(&tick_property(event), tick as i64);
    }
    tick
}

/// Whether a periodic subscriber should fire on this `tick` (every `interval` ticks).
pub fn periodic_fires(tick: u64, interval: u32) -> bool {
    let interval = u64::from(interval.max(1));
    tick > 0 && tick.is_multiple_of(interval)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::PermissionFlags;

    #[test]
    fn advance_and_periodic_interval() {
        let room_id = ObjectId::new("area:test-001");
        let mut objects = HashMap::from([(
            room_id.clone(),
            Object {
                id: room_id.clone(),
                name: "Test".to_string(),
                aliases: Vec::new(),
                location: None,
                prototype: None,
                owner: ObjectId::new("player:admin-001"),
                permissions: PermissionFlags::EVERYONE,
                properties: HashMap::new(),
                verbs: HashMap::new(),
                event_handlers: HashMap::new(),
                is_deleted: false,
                deleted_at: None,
            },
        )]);

        assert_eq!(advance_tick(&room_id, "on_enter", &mut objects), 1);
        assert_eq!(advance_tick(&room_id, "on_enter", &mut objects), 2);
        assert_eq!(current_tick(&room_id, "on_enter", &objects), 2);
        assert!(!periodic_fires(1, 3));
        assert!(!periodic_fires(2, 3));
        assert!(periodic_fires(3, 3));
    }
}