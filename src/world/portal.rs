//! Exit portals — doors, windows, and teleporters that link locations.

use std::collections::HashMap;

use crate::object::{Object, ObjectId, PortalKind};

/// Why passage through a passable portal is blocked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortalBlock {
    Closed,
    Locked,
}

/// Back-compat alias for door-centric call sites.
pub type DoorBlock = PortalBlock;

/// Passable portal (door, open gate, teleporter) on `direction`, if any.
pub fn passable_portal_for_direction<'a>(
    room_id: &ObjectId,
    direction: &str,
    objects: &'a HashMap<ObjectId, Object>,
) -> Option<&'a Object> {
    portal_for_direction(room_id, direction, objects).filter(|portal| portal.portal_passable())
}

/// Whether a passable portal blocks movement along an exit to `target_id`.
///
/// Non-passable portals (windows, viewports) are ignored so map exits and scenic
/// portals can share a direction label.
pub fn passable_portal_blocks_passage(
    room_id: &ObjectId,
    direction: &str,
    target_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> Option<PortalBlock> {
    let portal = passable_portal_for_direction(room_id, direction, objects)?;
    if !portal_permits_exit(portal, target_id) {
        return None;
    }
    portal_passage_block(portal)
}

/// Find the portal in `room_id` aligned with `direction`, if any.
pub fn portal_for_direction<'a>(
    room_id: &ObjectId,
    direction: &str,
    objects: &'a HashMap<ObjectId, Object>,
) -> Option<&'a Object> {
    let direction = direction.trim().to_ascii_lowercase();
    objects.values().find(|obj| {
        obj.is_active()
            && obj.is_portal()
            && obj.location.as_ref() == Some(room_id)
            && obj
                .portal_direction()
                .is_some_and(|d| d.eq_ignore_ascii_case(&direction))
    })
}

/// All portals placed in a room (doors, windows, teleports).
pub fn portals_in_room<'a>(
    room_id: &ObjectId,
    objects: &'a HashMap<ObjectId, Object>,
) -> Vec<&'a Object> {
    let mut portals: Vec<&Object> = objects
        .values()
        .filter(|obj| {
            obj.is_active()
                && obj.is_portal()
                && obj.location.as_ref() == Some(room_id)
        })
        .collect();
    portals.sort_by(|a, b| {
        a.portal_direction()
            .unwrap_or_default()
            .cmp(&b.portal_direction().unwrap_or_default())
            .then_with(|| a.name.cmp(&b.name))
    });
    portals
}

/// Back-compat wrapper — same as [`portal_for_direction`].
pub fn door_for_direction<'a>(
    room_id: &ObjectId,
    direction: &str,
    objects: &'a HashMap<ObjectId, Object>,
) -> Option<&'a Object> {
    portal_for_direction(room_id, direction, objects)
}

/// Whether a passable portal blocks movement and why.
pub fn portal_passage_block(portal: &Object) -> Option<PortalBlock> {
    if !portal.is_portal() || !portal.portal_passable() {
        return None;
    }
    if portal.gate_is_locked() {
        return Some(PortalBlock::Locked);
    }
    if !portal.gate_is_open() {
        return Some(PortalBlock::Closed);
    }
    None
}

/// Back-compat wrapper.
pub fn door_passage_block(portal: &Object) -> Option<PortalBlock> {
    portal_passage_block(portal)
}

/// Verify the exit target matches the portal destination when one guards the direction.
pub fn portal_permits_exit(portal: &Object, target_id: &ObjectId) -> bool {
    portal
        .portal_destination()
        .is_none_or(|dest| &dest == target_id)
}

/// Back-compat wrapper.
pub fn door_permits_exit(portal: &Object, target_id: &ObjectId) -> bool {
    portal_permits_exit(portal, target_id)
}

/// Short label for portal kind in player messages.
pub fn portal_kind_label(portal: &Object) -> &'static str {
    portal
        .portal_kind()
        .map(PortalKind::label)
        .unwrap_or("portal")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{DoorSpec, PermissionFlags, PortalSpec};

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
    fn portal_for_direction_finds_door_in_room() {
        let room = ObjectId::new("area:front-001");
        let dest = ObjectId::new("area:inside-001");
        let mut door = bare("item:door-001", "Wooden Door");
        door.location = Some(room.clone());
        door.apply_door_role(&DoorSpec {
            direction: "in".to_string(),
            destination: "inside".to_string(),
            open: false,
            lock_id: None,
            locked: false,
        });
        door.set_portal_destination(dest);

        let mut objects = HashMap::new();
        objects.insert(door.id.clone(), door);

        let found = portal_for_direction(&room, "in", &objects).unwrap();
        assert!(found.is_door());
        assert_eq!(portal_passage_block(found), Some(PortalBlock::Closed));
    }

    #[test]
    fn non_passable_window_does_not_block_map_exit_on_same_direction() {
        let room_id = ObjectId::new("area:hall-001");
        let pantry_id = ObjectId::new("room:pantry-001");
        let rear_id = ObjectId::new("area:rear-001");
        let mut hall = bare("area:hall-001", "Hall");
        hall.add_exit("east", pantry_id.clone());

        let mut window = bare("item:window-001", "Window");
        window.location = Some(room_id.clone());
        window.apply_portal_role(&PortalSpec {
            kind: crate::object::PortalKind::Window,
            direction: "east".to_string(),
            destination: "rear".to_string(),
            open: false,
            lock_id: None,
            locked: false,
            passable: None,
            transparent: None,
        });
        window.set_portal_destination(rear_id);

        let objects = HashMap::from([
            (hall.id.clone(), hall),
            (window.id.clone(), window),
        ]);

        assert!(passable_portal_blocks_passage(&room_id, "east", &pantry_id, &objects).is_none());
    }

    #[test]
    fn window_is_not_passable_but_allows_view_when_transparent() {
        let room = ObjectId::new("area:inside-001");
        let dest = ObjectId::new("area:yard-001");
        let mut window = bare("item:window-001", "Small Window");
        window.location = Some(room.clone());
        window.apply_portal_role(&PortalSpec {
            kind: PortalKind::Window,
            direction: "east".to_string(),
            destination: "yard".to_string(),
            open: false,
            lock_id: None,
            locked: false,
            passable: None,
            transparent: None,
        });
        window.set_portal_destination(dest);

        let mut objects = HashMap::new();
        objects.insert(window.id.clone(), window.clone());

        let found = portal_for_direction(&room, "east", &objects).unwrap();
        assert!(found.is_window());
        assert_eq!(portal_passage_block(found), None);
        assert!(found.portal_allows_view());
    }

    #[test]
    fn locked_window_blocks_view_and_passage() {
        let room = ObjectId::new("area:inside-001");
        let mut window = bare("item:window-001", "Small Window");
        window.location = Some(room.clone());
        window.apply_portal_role(&PortalSpec {
            kind: PortalKind::Window,
            direction: "east".to_string(),
            destination: "yard".to_string(),
            open: false,
            lock_id: Some("shutters".to_string()),
            locked: true,
            passable: None,
            transparent: None,
        });

        let mut objects = HashMap::new();
        objects.insert(window.id.clone(), window);

        let found = portal_for_direction(&room, "east", &objects).unwrap();
        assert!(!found.portal_allows_view());
        assert_eq!(portal_passage_block(found), None);
    }

    #[test]
    fn teleport_portal_blocks_when_closed() {
        let room = ObjectId::new("area:hub-001");
        let mut portal = bare("item:portal-001", "Shimmering Portal");
        portal.location = Some(room.clone());
        portal.apply_portal_role(&PortalSpec {
            kind: PortalKind::Teleport,
            direction: "portal".to_string(),
            destination: "elsewhere".to_string(),
            open: false,
            lock_id: None,
            locked: false,
            passable: None,
            transparent: None,
        });

        let mut objects = HashMap::new();
        objects.insert(portal.id.clone(), portal);

        let found = portal_for_direction(&room, "portal", &objects).unwrap();
        assert_eq!(portal_passage_block(found), Some(PortalBlock::Closed));
    }
}