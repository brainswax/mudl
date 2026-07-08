//! Player login names — unique `player:<login>` ids without counter suffix.

use std::collections::HashMap;

use super::{constrain_id_base, slugify_display_name, Object, ObjectId};

/// Object property storing the canonical login name (lowercase slug).
pub const LOGIN_NAME_PROPERTY: &str = "login_name";

/// Build a player object id from a normalized login name (`brains` → `player:brains`).
pub fn player_id_for_login_name(login_name: &str) -> ObjectId {
    ObjectId::new(format!("player:{login_name}"))
}

/// Normalize user input into a unique login-name slug (lowercase, no counter suffix).
pub fn normalize_player_login_name(raw: &str) -> Result<String, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Login name cannot be empty.");
    }
    let slug = constrain_id_base(&slugify_display_name(trimmed));
    if slug.is_empty() || slug == "object" {
        return Err("Login name contains no valid characters.");
    }
    Ok(slug)
}

/// Default in-character display name derived from a login slug (`alice-wonder` → `Alice Wonder`).
pub fn display_name_from_login_name(login_name: &str) -> String {
    login_name
        .split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Derive a display name from a legacy `player:` id (`player:admin-001` → `Admin`).
pub fn display_name_from_player_id(id: &ObjectId) -> String {
    let slug = player_id_login_slug(id).unwrap_or(id.as_str());
    if slug.contains('-') && slug.chars().last().is_some_and(|c| c.is_ascii_digit()) {
        let base = slug.split('-').next().unwrap_or(slug);
        let mut chars = base.chars();
        match chars.next() {
            None => "Player".to_string(),
            Some(first) => first.to_uppercase().chain(chars).collect(),
        }
    } else {
        display_name_from_login_name(slug)
    }
}

/// Login slug from a player object id (`player:brains` → `Some("brains")`).
pub fn player_id_login_slug(id: &ObjectId) -> Option<&str> {
    id.as_str().strip_prefix("player:")
}

/// Canonical login name for a player object (property, else id suffix).
pub fn player_login_name(obj: &Object) -> Option<String> {
    if !obj.id.as_str().starts_with("player:") {
        return None;
    }
    if let Some(stored) = obj.get_string_property(LOGIN_NAME_PROPERTY) {
        if !stored.is_empty() {
            return Some(stored);
        }
    }
    player_id_login_slug(&obj.id).map(str::to_string)
}

/// Whether `identity` matches a player's login name (case-insensitive).
pub fn player_login_name_matches(obj: &Object, identity: &str) -> bool {
    player_login_name(obj)
        .is_some_and(|name| name.eq_ignore_ascii_case(identity))
}

/// Find an active player by login name (case-insensitive).
pub fn find_player_by_login_name<'a>(
    objects: &'a HashMap<ObjectId, Object>,
    login_name: &str,
) -> Option<&'a Object> {
    objects.values().find(|obj| {
        obj.is_active() && player_login_name_matches(obj, login_name)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::PermissionFlags;
    use std::collections::HashMap;

    fn player(id: &str, name: &str, login: Option<&str>) -> Object {
        let (revision, updated_at) = crate::object::object_persistence_defaults();
        let mut obj = Object {
            id: ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new(id),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
            revision,
            updated_at,
        };
        if let Some(login) = login {
            obj.set_property_string(LOGIN_NAME_PROPERTY, login);
        }
        obj
    }

    #[test]
    fn normalize_login_name_slugifies_verbatim_input() {
        assert_eq!(normalize_player_login_name("Brains").unwrap(), "brains");
        assert_eq!(
            normalize_player_login_name("Alice Wonder").unwrap(),
            "alice-wonder"
        );
    }

    #[test]
    fn player_id_has_no_counter_suffix() {
        assert_eq!(
            player_id_for_login_name("brains").as_str(),
            "player:brains"
        );
    }

    #[test]
    fn find_player_by_login_name_uses_property_or_id() {
        let mut objects = HashMap::new();
        objects.insert(
            ObjectId::new("player:brains"),
            player("player:brains", "Brains", Some("brains")),
        );
        assert!(find_player_by_login_name(&objects, "brains").is_some());
        assert!(find_player_by_login_name(&objects, "Brains").is_some());
    }
}