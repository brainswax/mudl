//! Exit graph validation and traversal checks for navigable places.

use std::collections::HashMap;

use crate::object::{Object, ObjectId, Value};
use crate::world::navigation::{normalize_direction, resolve_exit};

/// Pick a scatter destination from `scatter_to` on `from` (deterministic per player).
pub fn pick_scatter_destination(
    from: &Object,
    player_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> Option<ObjectId> {
    let prop = from.get_property("scatter_to")?;
    let Value::List(items) = &prop.value else {
        return None;
    };
    let candidates: Vec<ObjectId> = items
        .iter()
        .filter_map(|value| {
            let Value::ObjectRef(id) = value else {
                return None;
            };
            objects
                .get(id)
                .filter(|place| place.is_active() && place.is_location())
                .map(|_| id.clone())
        })
        .collect();
    if candidates.is_empty() {
        return None;
    }
    let mut hash = 0usize;
    for byte in player_id.as_str().bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(byte as usize);
    }
    for byte in from.id.as_str().bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(byte as usize);
    }
    Some(candidates[hash % candidates.len()].clone())
}

/// When entering `target`, redirect to `loop_to` if the place defines one (silent maze loop).
pub fn apply_loop_entry(target_id: &ObjectId, objects: &HashMap<ObjectId, Object>) -> ObjectId {
    let Some(place) = objects.get(target_id) else {
        return target_id.clone();
    };
    let Some(prop) = place.get_property("loop_to") else {
        return target_id.clone();
    };
    let Value::ObjectRef(loop_id) = &prop.value else {
        return target_id.clone();
    };
    if objects
        .get(loop_id)
        .filter(|place| place.is_active() && place.is_location())
        .is_some()
    {
        loop_id.clone()
    } else {
        target_id.clone()
    }
}

/// Apply scatter exit redirection when leaving `from` along `direction`.
pub fn apply_scatter_exit(
    from: &Object,
    direction: &str,
    resolved_target: &ObjectId,
    player_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> ObjectId {
    let scatter_dir = from
        .get_property("scatter_direction")
        .and_then(|prop| {
            if let Value::String(dir) = &prop.value {
                normalize_direction(dir)
            } else {
                None
            }
        })
        .unwrap_or("out");
    let Some(dir) = normalize_direction(direction) else {
        return resolved_target.clone();
    };
    if dir != scatter_dir {
        return resolved_target.clone();
    }
    pick_scatter_destination(from, player_id, objects).unwrap_or_else(|| resolved_target.clone())
}

/// Canonical opposite direction for paired exits (north↔south, in↔out, etc.).
pub fn reverse_direction(direction: &str) -> Option<&'static str> {
    match normalize_direction(direction)? {
        "north" => Some("south"),
        "south" => Some("north"),
        "east" => Some("west"),
        "west" => Some("east"),
        "northeast" => Some("southwest"),
        "southwest" => Some("northeast"),
        "northwest" => Some("southeast"),
        "southeast" => Some("northwest"),
        "up" => Some("down"),
        "down" => Some("up"),
        "in" => Some("out"),
        "out" => Some("in"),
        _ => None,
    }
}

/// Whether `target` is an active navigable place reachable from `from` via `direction`.
pub fn can_traverse_exit(
    from: &Object,
    direction: &str,
    target_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> bool {
    if !from.is_active() || !from.is_location() {
        return false;
    }
    let exits = from.get_exits();
    let Some((_, resolved_target)) = resolve_exit(&exits, direction) else {
        return false;
    };
    if resolved_target != target_id {
        return false;
    }
    objects
        .get(target_id)
        .is_some_and(|target| target.is_active() && target.is_location())
}

/// Validate that a room has a parent area/room and areas are not improperly nested.
pub fn validate_place_hierarchy(
    place: &Object,
    objects: &HashMap<ObjectId, Object>,
) -> Result<(), String> {
    if !place.is_active() || !place.is_location() {
        return Ok(());
    }
    if place.is_room() {
        let parent = place
            .parent_place(objects)
            .ok_or_else(|| format!("Room '{}' ({}) has no parent place", place.name, place.id))?;
        if !parent.is_area() && !parent.is_room() {
            return Err(format!(
                "Room '{}' parent '{}' is not an area or room",
                place.name, parent.name
            ));
        }
    } else if place.is_area() {
        if let Some(parent) = place.parent_place(objects) {
            if !parent.is_area() {
                return Err(format!(
                    "Area '{}' is nested under non-area '{}'",
                    place.name, parent.name
                ));
            }
        }
    }
    Ok(())
}

/// Validate exit targets exist and point to active navigable places.
pub fn validate_place_exits(
    place: &Object,
    objects: &HashMap<ObjectId, Object>,
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    for (direction, target_id) in place.get_exits() {
        match objects.get(&target_id) {
            None => errors.push(format!(
                "{} ({}) exit '{}' points to missing object {}",
                place.name, place.id, direction, target_id
            )),
            Some(target) if !target.is_active() => errors.push(format!(
                "{} ({}) exit '{}' points to deleted place {}",
                place.name, place.id, direction, target_id
            )),
            Some(target) if !target.is_location() => errors.push(format!(
                "{} ({}) exit '{}' points to non-place {}",
                place.name, place.id, direction, target_id
            )),
            _ => {}
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Validate reciprocal exits where an opposite direction is defined.
pub fn validate_reciprocal_exits(
    place: &Object,
    objects: &HashMap<ObjectId, Object>,
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    for (direction, target_id) in place.get_exits() {
        let Some(reverse) = reverse_direction(&direction) else {
            continue;
        };
        let Some(target) = objects.get(&target_id) else {
            continue;
        };
        let target_exits = target.get_exits();
        if let Some((_, back_id)) = resolve_exit(&target_exits, reverse) {
            if back_id != &place.id {
                errors.push(format!(
                    "{} exit '{}' → {}, but {} '{}' points elsewhere ({})",
                    place.name, direction, target.name, target.name, reverse, back_id
                ));
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Validate every navigable place in the object graph.
pub fn validate_world_places(objects: &HashMap<ObjectId, Object>) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    for place in objects
        .values()
        .filter(|o| o.is_active() && o.is_location())
    {
        if let Err(msg) = validate_place_hierarchy(place, objects) {
            errors.push(msg);
        }
        if let Err(exit_errors) = validate_place_exits(place, objects) {
            errors.extend(exit_errors);
        }
        if let Err(reciprocal_errors) = validate_reciprocal_exits(place, objects) {
            errors.extend(reciprocal_errors);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::PermissionFlags;

    fn bare_place(id: &str, name: &str, parent: Option<ObjectId>) -> Object {
        Object {
            id: ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: parent,
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
    fn reverse_direction_pairs() {
        assert_eq!(reverse_direction("north"), Some("south"));
        assert_eq!(reverse_direction("in"), Some("out"));
        assert_eq!(reverse_direction("enter"), Some("out"));
    }

    #[test]
    fn can_traverse_requires_active_location_target() {
        let area_id = ObjectId::new("area:hall-001");
        let room_id = ObjectId::new("room:bed-001");
        let mut area = bare_place("area:hall-001", "Hall", None);
        area.add_exit("west", room_id.clone());
        let room = bare_place("room:bed-001", "Bedroom", Some(area_id.clone()));
        let mut objects = HashMap::new();
        objects.insert(area.id.clone(), area.clone());
        objects.insert(room.id.clone(), room);

        assert!(can_traverse_exit(&area, "west", &room_id, &objects));
        assert!(!can_traverse_exit(&area, "east", &room_id, &objects));
    }

    #[test]
    fn validate_room_requires_parent_area() {
        let orphan = bare_place("room:orphan-001", "Orphan", None);
        let objects = HashMap::from([(orphan.id.clone(), orphan.clone())]);
        let err = validate_place_hierarchy(&orphan, &objects).unwrap_err();
        assert!(err.contains("no parent place"));
    }

    #[test]
    fn validate_reciprocal_exits_allows_one_way_exits() {
        let area_id = ObjectId::new("area:hall-001");
        let room_id = ObjectId::new("room:bed-001");
        let mut area = bare_place("area:hall-001", "Hall", None);
        area.add_exit("west", room_id.clone());
        let room = bare_place("room:bed-001", "Bedroom", Some(area_id.clone()));
        let objects = HashMap::from([
            (area.id.clone(), area.clone()),
            (room.id.clone(), room.clone()),
        ]);
        validate_reciprocal_exits(&area, &objects).unwrap();
    }

    #[test]
    fn validate_reciprocal_exits_detects_mismatched_return() {
        let _a_id = ObjectId::new("area:a-001");
        let b_id = ObjectId::new("area:b-001");
        let c_id = ObjectId::new("area:c-001");
        let mut a = bare_place("area:a-001", "A", None);
        a.add_exit("north", b_id.clone());
        let mut b = bare_place("area:b-001", "B", None);
        b.add_exit("south", c_id.clone());
        let objects = HashMap::from([(a.id.clone(), a.clone()), (b.id.clone(), b.clone())]);
        let errors = validate_reciprocal_exits(&a, &objects).unwrap_err();
        assert!(errors[0].contains("points elsewhere"));
    }

    #[test]
    fn pick_scatter_destination_is_deterministic_per_player() {
        let player = ObjectId::new("player:admin-001");
        let mut heart = bare_place("area:heart-001", "Heart", None);
        let void_id = ObjectId::new("area:void-001");
        let path_id = ObjectId::new("area:path-001");
        heart.add_property(crate::object::Property {
            name: "scatter_to".to_string(),
            value: Value::List(vec![
                Value::ObjectRef(void_id.clone()),
                Value::ObjectRef(path_id.clone()),
            ]),
            permissions: crate::object::PermissionFlags::EVERYONE,
            behavior: None,
        });
        let void = bare_place("area:void-001", "Void", None);
        let path = bare_place("area:path-001", "Path", None);
        let objects = HashMap::from([
            (heart.id.clone(), heart.clone()),
            (void.id.clone(), void),
            (path.id.clone(), path),
        ]);
        let first = pick_scatter_destination(&heart, &player, &objects).unwrap();
        let second = pick_scatter_destination(&heart, &player, &objects).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn apply_loop_entry_redirects_to_configured_place() {
        let entry_id = ObjectId::new("area:entry-001");
        let dead_id = ObjectId::new("area:dead-001");
        let mut dead = bare_place("area:dead-001", "Dead End", None);
        dead.add_property(crate::object::Property {
            name: "loop_to".to_string(),
            value: Value::ObjectRef(entry_id.clone()),
            permissions: crate::object::PermissionFlags::EVERYONE,
            behavior: None,
        });
        let entry = bare_place("area:entry-001", "Entry", None);
        let objects = HashMap::from([(dead.id.clone(), dead.clone()), (entry.id.clone(), entry)]);
        assert_eq!(apply_loop_entry(&dead_id, &objects), entry_id);
        assert_eq!(apply_loop_entry(&entry_id, &objects), entry_id);
    }

    #[test]
    fn apply_scatter_exit_only_on_configured_direction() {
        let player = ObjectId::new("player:hero-001");
        let mut heart = bare_place("area:heart-001", "Heart", None);
        let spill = ObjectId::new("area:spill-001");
        let void_id = ObjectId::new("area:void-001");
        heart.add_property(crate::object::Property {
            name: "scatter_to".to_string(),
            value: Value::List(vec![Value::ObjectRef(void_id.clone())]),
            permissions: crate::object::PermissionFlags::EVERYONE,
            behavior: None,
        });
        heart.add_property(crate::object::Property {
            name: "scatter_direction".to_string(),
            value: Value::String("out".to_string()),
            permissions: crate::object::PermissionFlags::EVERYONE,
            behavior: None,
        });
        let void = bare_place("area:void-001", "Void", None);
        let objects = HashMap::from([(heart.id.clone(), heart.clone()), (void.id.clone(), void)]);
        assert_eq!(
            apply_scatter_exit(&heart, "out", &spill, &player, &objects),
            void_id
        );
        assert_eq!(
            apply_scatter_exit(&heart, "south", &spill, &player, &objects),
            spill
        );
    }

    #[test]
    fn validate_world_places_accepts_reciprocal_graph() {
        let area_id = ObjectId::new("area:hall-001");
        let room_id = ObjectId::new("room:bed-001");
        let mut area = bare_place("area:hall-001", "Hall", None);
        area.add_exit("west", room_id.clone());
        let mut room = bare_place("room:bed-001", "Bedroom", Some(area_id.clone()));
        room.add_exit("east", area_id.clone());
        let objects = HashMap::from([
            (area.id.clone(), area.clone()),
            (room.id.clone(), room.clone()),
        ]);
        validate_world_places(&objects).unwrap();
    }
}
