//! Runtime place creation and exit linking for builder commands (`@dig`, `@link`).

use std::collections::HashMap;
use std::fmt;

use crate::object::{Object, ObjectFactory, ObjectId, Value};
use crate::persistence::Persistence;
use crate::world::exit_index::normalize_exit_input;
use crate::world::exit_index::ExitIndex;
use crate::world::exits::{validate_place_exits, validate_place_hierarchy};
use crate::world::navigation::resolve_exit;

/// Options for `@dig`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DigOptions {
    pub place_type: Option<String>,
    pub description: Option<String>,
    pub reciprocal: Option<bool>,
    /// Exit name on the new place pointing back (required for reciprocal links unless `exit_returns` is set).
    pub return_exit: Option<String>,
}

/// Request to dig a new place from an existing one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigRequest {
    pub direction: String,
    pub name: String,
    pub options: DigOptions,
}

/// Errors from place building operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceBuildError {
    NoLocation,
    NotFound(String),
    InvalidDirection(String),
    ExitExists(String),
    InvalidPlaceType(String),
    NotAPlace(String),
    Hierarchy(String),
    Validation(String),
}

impl fmt::Display for PlaceBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoLocation => write!(f, "You have no current location to dig from."),
            Self::NotFound(name) => write!(f, "No place found matching '{name}'."),
            Self::InvalidDirection(dir) => write!(f, "'{dir}' is not a valid direction."),
            Self::ExitExists(dir) => write!(f, "An exit '{dir}' already leads somewhere."),
            Self::InvalidPlaceType(kind) => {
                write!(f, "Unknown place type '{kind}' (use area or room).")
            }
            Self::NotAPlace(name) => write!(f, "'{name}' is not a navigable place."),
            Self::Hierarchy(msg) => write!(f, "{msg}"),
            Self::Validation(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for PlaceBuildError {}

fn exit_label(direction: &str) -> Result<String, PlaceBuildError> {
    let label = normalize_exit_input(direction);
    if label.is_empty() {
        return Err(PlaceBuildError::InvalidDirection(direction.to_string()));
    }
    Ok(label)
}

fn default_place_type(
    from: &Object,
    requested: Option<&str>,
) -> Result<&'static str, PlaceBuildError> {
    if let Some(kind) = requested {
        return match kind.to_ascii_lowercase().as_str() {
            "area" => Ok("area"),
            "room" => Ok("room"),
            other => Err(PlaceBuildError::InvalidPlaceType(other.to_string())),
        };
    }
    Ok(if from.is_room() { "room" } else { "area" })
}

fn parent_for_new_room(from: &Object, place_type: &str) -> Option<ObjectId> {
    if place_type == "room" {
        Some(from.id.clone())
    } else {
        None
    }
}

/// Link a one-way exit from `from` to `to`.
pub fn link_exit(
    from: &mut Object,
    direction: &str,
    to: &Object,
    objects: &HashMap<ObjectId, Object>,
) -> Result<(), PlaceBuildError> {
    let dir = exit_label(direction)?;
    if !to.is_active() || !to.is_location() {
        return Err(PlaceBuildError::NotAPlace(to.name.clone()));
    }
    from.add_exit(&dir, to.id.clone());
    validate_place_exits(from, objects)
        .map_err(|errors| PlaceBuildError::Validation(errors.join("; ")))?;
    Ok(())
}

/// Link exits between two places, optionally adding a reciprocal return exit on `to`.
pub fn link_places(
    from: &mut Object,
    to: &mut Object,
    direction: &str,
    objects: &HashMap<ObjectId, Object>,
    reciprocal: bool,
    return_exit: Option<&str>,
) -> Result<Vec<String>, PlaceBuildError> {
    let dir = exit_label(direction)?;
    link_exit(from, &dir, to, objects)?;

    let mut notes = vec![format!("Linked {} exit '{}' → {}", from.name, dir, to.name)];

    if reciprocal {
        let reverse = if let Some(ret) = return_exit {
            Some(exit_label(ret)?)
        } else if let Some(ret) = from.exit_return_name(&dir) {
            Some(exit_label(&ret)?)
        } else {
            None
        };
        if let Some(reverse) = reverse {
            let target_index = ExitIndex::from_place(to);
            if let Some((_, existing)) = resolve_exit(&target_index, &reverse) {
                if existing != &from.id {
                    notes.push(format!(
                        "Skipped reciprocal '{}' on {} (already points elsewhere)",
                        reverse, to.name
                    ));
                }
            } else {
                to.add_exit(&reverse, from.id.clone());
                from.set_exit_return(&dir, &reverse);
                validate_place_exits(to, objects)
                    .map_err(|errors| PlaceBuildError::Validation(errors.join("; ")))?;
                notes.push(format!(
                    "Linked {} exit '{}' → {}",
                    to.name, reverse, from.name
                ));
            }
        } else {
            notes.push(format!(
                "Skipped reciprocal on {} (set exit_returns or pass --return <exit>)",
                to.name
            ));
        }
    }

    Ok(notes)
}

/// Remove an exit from a place. Returns the former target id when one existed.
pub fn unlink_exit(
    from: &mut Object,
    direction: &str,
) -> Result<Option<ObjectId>, PlaceBuildError> {
    let dir = exit_label(direction)?;
    let exits = from.get_exits();
    let Some(target_id) = exits
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(&dir))
        .map(|(_, id)| id.clone())
    else {
        return Err(PlaceBuildError::Validation(format!(
            "{} has no exit '{}'",
            from.name, dir
        )));
    };
    if let Some(prop) = from.properties.get_mut("exits") {
        if let Value::Map(map) = &mut prop.value {
            let key = map
                .keys()
                .find(|k| k.eq_ignore_ascii_case(&dir))
                .cloned();
            if let Some(key) = key {
                map.remove(&key);
            }
        }
    }
    Ok(Some(target_id))
}

/// Dig a new place in `direction` from `from` and wire exits into the live graph.
pub async fn dig_place<P: Persistence>(
    factory: &ObjectFactory<P>,
    from: &Object,
    owner: ObjectId,
    request: DigRequest,
    objects: &HashMap<ObjectId, Object>,
) -> Result<DigResult, PlaceBuildError> {
    let dir = exit_label(&request.direction)?;
    if from
        .get_exits()
        .keys()
        .any(|name| name.eq_ignore_ascii_case(&dir))
    {
        return Err(PlaceBuildError::ExitExists(dir.clone()));
    }

    let place_type = default_place_type(from, request.options.place_type.as_deref())?;
    let parent = parent_for_new_room(from, place_type);

    let mut new_place = factory
        .create_place(
            place_type,
            &request.name,
            owner,
            request.options.description.as_deref(),
            parent,
        )
        .await
        .map_err(|e| PlaceBuildError::Validation(e.to_string()))?;

    let mut objects_with_new = objects.clone();
    objects_with_new.insert(new_place.id.clone(), new_place.clone());
    validate_place_hierarchy(&new_place, &objects_with_new).map_err(PlaceBuildError::Hierarchy)?;

    let reciprocal = request.options.reciprocal.unwrap_or(true);
    let mut from_updated = from.clone();
    let notes = link_places(
        &mut from_updated,
        &mut new_place,
        &dir,
        &objects_with_new,
        reciprocal,
        request.options.return_exit.as_deref(),
    )?;

    crate::persistence::save_and_sync(factory.persistence(), &mut from_updated)
        .await
        .map_err(|e| PlaceBuildError::Validation(e.to_string()))?;
    crate::persistence::save_and_sync(factory.persistence(), &mut new_place)
        .await
        .map_err(|e| PlaceBuildError::Validation(e.to_string()))?;

    Ok(DigResult {
        new_place,
        from_updated,
        notes,
    })
}

/// Result of a successful `@dig`.
#[derive(Debug, Clone)]
pub struct DigResult {
    pub new_place: Object,
    pub from_updated: Object,
    pub notes: Vec<String>,
}

/// Apply dig results to an object map and return every id that changed.
pub fn apply_dig_result(
    objects: &mut HashMap<ObjectId, Object>,
    result: &DigResult,
) -> Vec<ObjectId> {
    let DigResult {
        new_place,
        from_updated,
        notes: _,
    } = result;
    let dirty = vec![from_updated.id.clone(), new_place.id.clone()];
    objects.insert(from_updated.id.clone(), from_updated.clone());
    objects.insert(new_place.id.clone(), new_place.clone());
    dirty
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{PermissionFlags, Property, Value};
    use crate::persistence::SqlitePersistence;

    fn bare_place(id: &str, name: &str) -> Object {
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
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        }
    }

    #[test]
    fn link_places_adds_reciprocal_exits() {
        let mut a = bare_place("area:a-001", "Clearing");
        let mut b = bare_place("area:b-001", "Forest");
        let objects = HashMap::from([(a.id.clone(), a.clone()), (b.id.clone(), b.clone())]);

        let notes = link_places(&mut a, &mut b, "north", &objects, true, Some("south")).unwrap();
        assert!(notes
            .iter()
            .any(|n| n.contains("Linked Clearing exit 'north'")));
        assert!(notes
            .iter()
            .any(|n| n.contains("Linked Forest exit 'south'")));
        assert_eq!(a.get_exits().get("north"), Some(&b.id));
        assert_eq!(b.get_exits().get("south"), Some(&a.id));
    }

    #[test]
    fn unlink_exit_removes_direction() {
        let mut a = bare_place("area:a-001", "Hall");
        let b_id = ObjectId::new("room:b-001");
        a.add_exit("west", b_id.clone());
        let removed = unlink_exit(&mut a, "west").unwrap();
        assert_eq!(removed, Some(b_id));
        assert!(a.get_exits().is_empty());
    }

    #[tokio::test]
    async fn dig_place_creates_linked_room_with_parent() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence);
        let owner = ObjectId::new("player:admin-001");
        let mut hall = bare_place("area:hall-001", "Main Hall");
        hall.add_property(Property {
            name: "description".to_string(),
            value: Value::String("A hall.".to_string()),
            permissions: PermissionFlags::EVERYONE,
            behavior: None,
        });
        let objects = HashMap::from([(hall.id.clone(), hall.clone())]);

        let result = dig_place(
            &factory,
            &hall,
            owner,
            DigRequest {
                direction: "west".to_string(),
                name: "Pantry".to_string(),
                options: DigOptions {
                    place_type: Some("room".to_string()),
                    description: Some("Shelves and jars.".to_string()),
                    reciprocal: Some(true),
                    return_exit: Some("east".to_string()),
                },
            },
            &objects,
        )
        .await
        .unwrap();

        assert!(result.notes.len() >= 2);
        let pantry_id = result.from_updated.get_exits().get("west").unwrap().clone();
        let pantry = factory.load_object(&pantry_id).await.unwrap().unwrap();
        assert!(pantry.is_room());
        assert_eq!(pantry.name, "Pantry");
        assert_eq!(pantry.location.as_ref(), Some(&hall.id));
        assert_eq!(
            pantry.get_description().as_deref(),
            Some("Shelves and jars.")
        );
        assert_eq!(pantry.get_exits().get("east"), Some(&hall.id));
    }

    #[tokio::test]
    async fn dug_places_persist_and_reload_with_exits() {
        use crate::world::session::hydrate_world;

        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence.clone());
        let owner = ObjectId::new("player:admin-001");
        let mut clearing = bare_place("area:clearing-001", "West Clearing");
        crate::persistence::save_and_sync(factory.persistence(), &mut clearing)
            .await
            .unwrap();
        let objects = HashMap::from([(clearing.id.clone(), clearing.clone())]);

        dig_place(
            &factory,
            &clearing,
            owner,
            DigRequest {
                direction: "north".to_string(),
                name: "Forest Glade".to_string(),
                options: DigOptions {
                    place_type: Some("area".to_string()),
                    description: Some("Sunlight through the trees.".to_string()),
                    reciprocal: Some(true),
                    return_exit: Some("south".to_string()),
                },
            },
            &objects,
        )
        .await
        .unwrap();

        let reloaded = hydrate_world(&persistence).await.unwrap();
        let clearing = reloaded
            .values()
            .find(|o| o.name == "West Clearing")
            .expect("clearing");
        let glade = reloaded
            .values()
            .find(|o| o.name == "Forest Glade")
            .expect("glade");
        assert_eq!(clearing.get_exits().get("north"), Some(&glade.id));
        assert_eq!(glade.get_exits().get("south"), Some(&clearing.id));
        assert!(glade.is_area());
        assert_eq!(
            glade.get_description().as_deref(),
            Some("Sunlight through the trees.")
        );
    }
}
