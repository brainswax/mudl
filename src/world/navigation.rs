//! Player movement: direction normalization and exit resolution.

use std::collections::HashMap;

use crate::object::{Object, ObjectId};

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

/// Normalize player movement input to a canonical compass direction, or pass through a custom exit label.
pub fn movement_input(input: &str) -> String {
    let trimmed = input.trim();
    if let Some(canonical) = normalize_direction(trimmed) {
        canonical.to_string()
    } else {
        trimmed.to_ascii_lowercase()
    }
}

/// Parse movement from a command line: `north`, `go north`, `go around`, `enter`, etc.
///
/// Recognizes compass aliases and `go <label>`. Custom exit names are passed through lowercased
/// for resolution against the current room's exit map.
pub fn movement_direction_from_line(verb: &str, args: &[&str]) -> Option<String> {
    if verb == "go" {
        if args.is_empty() {
            return None;
        }
        return Some(movement_input(&args.join(" ")));
    }
    normalize_direction(verb).map(|dir| dir.to_string())
}

/// Parse movement including standalone custom exit verbs (`around`, `rear`, …).
///
/// When `exits` is provided, a lone verb matching an exit key is treated as movement.
pub fn movement_from_line(
    verb: &str,
    args: &[&str],
    exits: Option<&HashMap<String, ObjectId>>,
) -> Option<String> {
    if let Some(dir) = movement_direction_from_line(verb, args) {
        return Some(dir);
    }
    if !args.is_empty() {
        return None;
    }
    let custom = verb.trim().to_ascii_lowercase();
    if exits.is_some_and(|map| map.keys().any(|key| key.eq_ignore_ascii_case(&custom))) {
        return Some(custom);
    }
    None
}

/// Resolve an exit direction against a room's exit map (case-insensitive keys, alias support).
pub fn resolve_exit<'a>(
    exits: &'a HashMap<String, ObjectId>,
    direction: &str,
) -> Option<(&'a str, &'a ObjectId)> {
    let input = direction.trim();
    if let Some(canonical) = normalize_direction(input) {
        if let Some(target) = exits.get(canonical) {
            return Some((canonical, target));
        }
    }
    for (key, target) in exits {
        if key.eq_ignore_ascii_case(input) {
            return Some((key.as_str(), target));
        }
    }
    None
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
            Some("north".to_string())
        );
        assert_eq!(
            movement_direction_from_line("go", &["n"]),
            Some("north".to_string())
        );
        assert_eq!(
            movement_direction_from_line("go", &["around"]),
            Some("around".to_string())
        );
        assert_eq!(
            movement_direction_from_line("south", &[]),
            Some("south".to_string())
        );
        assert_eq!(
            movement_direction_from_line("enter", &[]),
            Some("in".to_string())
        );
        assert_eq!(movement_direction_from_line("look", &[]), None);
    }

    #[test]
    fn movement_from_line_recognizes_standalone_custom_exits() {
        let mut exits = HashMap::new();
        exits.insert("around".to_string(), ObjectId::new("area:cottage-front-001"));
        assert_eq!(
            movement_from_line("around", &[], Some(&exits)),
            Some("around".to_string())
        );
        assert_eq!(movement_from_line("look", &[], Some(&exits)), None);
        assert_eq!(
            movement_from_line("go", &["around"], Some(&exits)),
            Some("around".to_string())
        );
    }

    #[test]
    fn resolve_exit_matches_canonical_and_custom_directions() {
        let mut exits = HashMap::new();
        let north_id = ObjectId::new("area:forest-001");
        let front_id = ObjectId::new("area:cottage-front-001");
        exits.insert("north".to_string(), north_id.clone());
        exits.insert("around".to_string(), front_id.clone());
        assert_eq!(
            resolve_exit(&exits, "n").map(|(d, id)| (d, id.clone())),
            Some(("north", north_id))
        );
        assert_eq!(
            resolve_exit(&exits, "Around").map(|(d, id)| (d, id.clone())),
            Some(("around", front_id))
        );
        assert!(resolve_exit(&exits, "east").is_none());
    }
}
