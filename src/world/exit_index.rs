//! Builder-defined exits — names, aliases, and return links (no hard-coded compass).

use std::collections::HashMap;

use crate::object::{Object, ObjectId};

/// Runtime index of a place's exits for movement, look, and validation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExitIndex {
    /// Canonical exit name (as defined by the builder) → destination.
    exits: HashMap<String, ObjectId>,
    /// Lowercase player input → canonical exit name (includes aliases).
    lookup: HashMap<String, String>,
}

/// Normalize free-text exit input from the player.
pub fn normalize_exit_input(input: &str) -> String {
    input.trim().to_ascii_lowercase()
}

impl ExitIndex {
    /// Build an index from a place's `exits`, `exit_aliases`, and related properties.
    pub fn from_place(place: &Object) -> Self {
        let mut exits = HashMap::new();
        let mut lookup = HashMap::new();

        for (name, target) in place.get_exits() {
            let lower = name.to_ascii_lowercase();
            lookup.insert(lower, name.clone());
            exits.insert(name, target);
        }

        for (alias, exit_name) in place.get_exit_aliases() {
            let alias_lower = alias.to_ascii_lowercase();
            if let Some(canonical) = exits
                .keys()
                .find(|n| n.eq_ignore_ascii_case(&exit_name))
                .cloned()
            {
                lookup.insert(alias_lower, canonical);
            }
        }

        Self { exits, lookup }
    }

    /// Resolve player input to canonical exit name and destination id.
    pub fn resolve(&self, input: &str) -> Option<(&str, &ObjectId)> {
        let key = normalize_exit_input(input);
        let canonical = self.lookup.get(&key)?;
        let target = self.exits.get(canonical)?;
        Some((canonical.as_str(), target))
    }

    /// Whether `input` matches an exit name or alias.
    pub fn contains_input(&self, input: &str) -> bool {
        self.resolve(input).is_some()
    }

    /// Canonical exit names defined on this place, sorted for display.
    pub fn exit_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.exits.keys().map(|s| s.as_str()).collect();
        names.sort_unstable();
        names
    }

    /// Aliases for `exit_name` (excluding the canonical name itself).
    pub fn aliases_for(&self, exit_name: &str) -> Vec<&str> {
        let mut aliases: Vec<&str> = self
            .lookup
            .iter()
            .filter(|(_, canon)| canon.eq_ignore_ascii_case(exit_name))
            .filter(|(alias, canon)| !alias.eq_ignore_ascii_case(canon.as_str()))
            .map(|(alias, _)| alias.as_str())
            .collect();
        aliases.sort_unstable();
        aliases
    }

    /// Underlying exit map (canonical names).
    pub fn exits(&self) -> &HashMap<String, ObjectId> {
        &self.exits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::PermissionFlags;

    fn place_with_exits(exits: &[(&str, &str)], aliases: &[(&str, &str)]) -> Object {
        let mut place = Object {
            id: ObjectId::new("area:test-001"),
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
        };
        for (dir, target) in exits {
            place.add_exit(dir, ObjectId::new(*target));
        }
        for (alias, exit) in aliases {
            place.set_exit_alias(alias, exit);
        }
        place
    }

    #[test]
    fn resolve_matches_name_and_alias() {
        let place = place_with_exits(
            &[("around", "area:front-001"), ("west", "area:void-001")],
            &[("path", "around")],
        );
        let index = ExitIndex::from_place(&place);
        assert_eq!(
            index.resolve("Around").map(|(n, id)| (n, id.as_str())),
            Some(("around", "area:front-001"))
        );
        assert_eq!(
            index.resolve("path").map(|(n, _)| n),
            Some("around")
        );
        assert!(index.resolve("north").is_none());
    }

    #[test]
    fn aliases_for_lists_only_alternate_names() {
        let place = place_with_exits(&[("north", "area:b-001")], &[("n", "north")]);
        let index = ExitIndex::from_place(&place);
        assert_eq!(index.aliases_for("north"), vec!["n"]);
    }
}