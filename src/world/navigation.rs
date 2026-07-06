//! Player movement: direction normalization and exit resolution.

use std::collections::HashMap;

use crate::object::{ObjectId, Object};

/// Canonical direction name after normalizing player input.
pub fn normalize_direction(input: &str) -> Option<&'static str> {
    match input.trim().to_ascii_lowercase().as_str() {
        "n" | "north" => Some("north"),
        "s" | "south" => Some("south"),
        "e" | "east" => Some("east"),
        "w" | "west" => Some("west"),
        "ne" | "northeast" => Some("northeast"),
        "nw" | "northwest" => Some("northwest"),
        "se" | "southeast" => Some("southeast"),
        "sw" | "southwest" => Some("southwest"),
        "u" | "up" => Some("up"),
        "d" | "down" => Some("down"),
        "in" | "inside" | "enter" => Some("in"),
        "out" | "outside" | "exit" | "leave" => Some("out"),
        _ => None,
    }
}

/// Whether `verb` is a standalone movement command (not `go`).
pub fn is_direction_verb(verb: &str) -> bool {
    normalize_direction(verb).is_some()
}

/// Parse movement from a command line: `north`, `go north`, `enter`, etc.
///
/// Returns the canonical direction name when the line is a movement command.
pub fn movement_direction_from_line(verb: &str, args: &[&str]) -> Option<&'static str> {
    if verb == "go" {
        return args.first().and_then(|arg| normalize_direction(arg));
    }
    normalize_direction(verb)
}

/// Resolve an exit direction against a room's exit map (case-insensitive keys, alias support).
pub fn resolve_exit<'a>(
    exits: &'a HashMap<String, ObjectId>,
    direction: &str,
) -> Option<(&'static str, &'a ObjectId)> {
    let canonical = normalize_direction(direction)?;
    exits
        .get(canonical)
        .map(|target| (canonical, target))
}

/// All exit direction labels for a location, sorted for display consistency.
pub fn exit_directions(room: &Object) -> Vec<String> {
    let mut dirs: Vec<String> = room.get_exits().into_keys().collect();
    dirs.sort_unstable();
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_direction_aliases() {
        assert_eq!(normalize_direction("N"), Some("north"));
        assert_eq!(normalize_direction("enter"), Some("in"));
        assert_eq!(normalize_direction("leave"), Some("out"));
        assert_eq!(normalize_direction("look"), None);
    }

    #[test]
    fn movement_direction_from_line_parses_go_and_standalone() {
        assert_eq!(
            movement_direction_from_line("go", &["north"]),
            Some("north")
        );
        assert_eq!(
            movement_direction_from_line("go", &["n"]),
            Some("north")
        );
        assert_eq!(movement_direction_from_line("south", &[]), Some("south"));
        assert_eq!(movement_direction_from_line("enter", &[]), Some("in"));
        assert_eq!(movement_direction_from_line("look", &[]), None);
    }

    #[test]
    fn resolve_exit_matches_canonical_direction() {
        let mut exits = HashMap::new();
        let north_id = ObjectId::new("area:forest-001");
        exits.insert("north".to_string(), north_id.clone());
        assert_eq!(
            resolve_exit(&exits, "n").map(|(d, id)| (d, id.clone())),
            Some(("north", north_id))
        );
        assert!(resolve_exit(&exits, "east").is_none());
    }
}