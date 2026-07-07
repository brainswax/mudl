//! Player movement — builder-defined exit names and aliases only.

use std::collections::HashMap;

use crate::object::{Object, ObjectId};

use super::exit_index::{normalize_exit_input, ExitIndex};

/// Parse `go <exit...>` movement input (lowercased; resolution happens against the room index).
pub fn movement_input(input: &str) -> String {
    normalize_exit_input(input)
}

/// Parse movement from a command line: `go around`, `around`, etc.
pub fn movement_direction_from_line(verb: &str, args: &[&str]) -> Option<String> {
    if verb == "go" {
        if args.is_empty() {
            return None;
        }
        return Some(movement_input(&args.join(" ")));
    }
    None
}

/// Parse movement including standalone exit verbs when they match the room index.
pub fn movement_from_line(verb: &str, args: &[&str], index: Option<&ExitIndex>) -> Option<String> {
    if verb == "go" {
        if args.is_empty() {
            return None;
        }
        let input = movement_input(&args.join(" "));
        if let Some(idx) = index {
            if let Some((name, _)) = idx.resolve(&input) {
                return Some(name.to_string());
            }
        }
        return Some(input);
    }
    if !args.is_empty() {
        return None;
    }
    let input = normalize_exit_input(verb);
    index
        .and_then(|idx| idx.resolve(&input).map(|(name, _)| name.to_string()))
}

/// Resolve player movement input against a place's exit index.
pub fn resolve_exit<'a>(
    index: &'a ExitIndex,
    input: &str,
) -> Option<(&'a str, &'a ObjectId)> {
    index.resolve(input)
}

/// Resolve against a raw exit map (no aliases) — for legacy call sites during migration.
pub fn resolve_exit_map<'a>(
    exits: &'a HashMap<String, ObjectId>,
    input: &str,
) -> Option<(&'a str, &'a ObjectId)> {
    let key = normalize_exit_input(input);
    for (name, target) in exits {
        if name.eq_ignore_ascii_case(&key) {
            return Some((name.as_str(), target));
        }
    }
    None
}

/// Build an exit index for `place`.
pub fn exit_index(place: &Object) -> ExitIndex {
    ExitIndex::from_place(place)
}

/// All exit direction labels for a location, sorted for display consistency.
pub fn exit_directions(room: &Object) -> Vec<String> {
    ExitIndex::from_place(room)
        .exit_names()
        .into_iter()
        .map(|s| s.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::PermissionFlags;

    fn sample_place() -> Object {
        let mut place = Object {
            id: ObjectId::new("area:rear-001"),
            name: "Rear".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new("player:admin-001"),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        };
        place.add_exit("around", ObjectId::new("area:front-001"));
        place.set_exit_alias("path", "around");
        place
    }

    #[test]
    fn movement_direction_from_line_parses_go_and_standalone_input() {
        assert_eq!(
            movement_direction_from_line("go", &["around"]),
            Some("around".to_string())
        );
        assert_eq!(movement_direction_from_line("around", &[]), None);
        assert_eq!(movement_direction_from_line("look", &[]), None);
    }

    #[test]
    fn movement_from_line_recognizes_standalone_exit_when_index_matches() {
        let index = ExitIndex::from_place(&sample_place());
        assert_eq!(
            movement_from_line("around", &[], Some(&index)),
            Some("around".to_string())
        );
        assert_eq!(
            movement_from_line("path", &[], Some(&index)),
            Some("around".to_string())
        );
        assert_eq!(movement_from_line("look", &[], Some(&index)), None);
        assert_eq!(
            movement_from_line("go", &["path"], Some(&index)),
            Some("around".to_string())
        );
    }

    #[test]
    fn resolve_exit_uses_aliases() {
        let index = ExitIndex::from_place(&sample_place());
        let front = ObjectId::new("area:front-001");
        assert_eq!(
            resolve_exit(&index, "path").map(|(n, id)| (n, id.clone())),
            Some(("around", front))
        );
    }
}