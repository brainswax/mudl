//! Door objects that gate movement between locations.

use std::collections::HashMap;

use crate::object::{Object, ObjectId};

/// Why passage through a door is blocked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DoorBlock {
    Closed,
    Locked,
}

/// Find the door in `room_id` guarding `direction`, if any.
pub fn door_for_direction<'a>(
    room_id: &ObjectId,
    direction: &str,
    objects: &'a HashMap<ObjectId, Object>,
) -> Option<&'a Object> {
    let direction = direction.trim().to_ascii_lowercase();
    objects.values().find(|obj| {
        obj.is_active()
            && obj.is_door()
            && obj.location.as_ref() == Some(room_id)
            && obj
                .door_direction()
                .is_some_and(|d| d.eq_ignore_ascii_case(&direction))
    })
}

/// Whether a door blocks passage and why.
pub fn door_passage_block(door: &Object) -> Option<DoorBlock> {
    if !door.is_door() {
        return None;
    }
    if door.gate_is_locked() {
        return Some(DoorBlock::Locked);
    }
    if !door.gate_is_open() {
        return Some(DoorBlock::Closed);
    }
    None
}

/// Verify the exit target matches the door's destination when a door guards the direction.
pub fn door_permits_exit(
    door: &Object,
    target_id: &ObjectId,
) -> bool {
    door.door_destination()
        .is_none_or(|dest| &dest == target_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{DoorSpec, PermissionFlags};

    fn bare(id: &str) -> Object {
        Object {
            id: ObjectId::new(id),
            name: "Wooden Door".to_string(),
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
    fn door_for_direction_finds_door_in_room() {
        let room = ObjectId::new("area:front-001");
        let dest = ObjectId::new("area:inside-001");
        let mut door = bare("item:door-001");
        door.location = Some(room.clone());
        door.apply_door_role(&DoorSpec {
            direction: "in".to_string(),
            destination: "inside".to_string(),
            open: false,
            lock_id: None,
            locked: false,
        });
        door.set_door_destination(dest);

        let mut objects = HashMap::new();
        objects.insert(door.id.clone(), door);

        let found = door_for_direction(&room, "in", &objects).unwrap();
        assert!(found.is_door());
        assert_eq!(door_passage_block(found), Some(DoorBlock::Closed));
    }
}