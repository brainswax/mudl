//! Field classification for builder tools and `@examine`.

/// Runtime properties stored in `Object.properties` but shown under `state:`.
pub const STATE_PROPERTY_KEYS: &[&str] = &[
    "contents",
    "body_slots",
    "stack_count",
    "carried_slot",
    "health",
    "active_effects",
    "condition_ticks",
    "stat_mods",
];

/// Top-level object fields shown under `state:`.
/// Computed examine lines; never writable via `@set`.
pub const STATUS_KEYS: &[&str] = &["contents_weight", "carried_weight", "total_weight"];

/// How a builder key maps to storage and `@examine` sections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    ObjectField(&'static str),
    StateProperty(String),
    ConfigProperty(String),
    Verb(String),
    Status,
    Immutable(&'static str),
}

/// Normalize a builder key to lowercase ASCII.
pub fn normalize_key(key: &str) -> String {
    key.trim().to_ascii_lowercase()
}

/// Classify a key for set/unset/examine routing.
pub fn classify_key(key: &str) -> FieldKind {
    let key = normalize_key(key);

    if key == "id" || key == "type" {
        return FieldKind::Immutable(if key == "id" { "id" } else { "type" });
    }

    if STATUS_KEYS.contains(&key.as_str()) || key.starts_with("status.") {
        return FieldKind::Status;
    }

    if let Some(rest) = key.strip_prefix("verb.") {
        if !rest.is_empty() {
            return FieldKind::Verb(rest.to_string());
        }
    }
    if let Some(rest) = key.strip_prefix("verb:") {
        if !rest.is_empty() {
            return FieldKind::Verb(rest.to_string());
        }
    }

    match key.as_str() {
        "name" | "owner" | "location" | "prototype" | "alias" => {
            FieldKind::ObjectField(match key.as_str() {
                "name" => "name",
                "owner" => "owner",
                "location" => "location",
                "prototype" => "prototype",
                _ => "alias",
            })
        }
        k if STATE_PROPERTY_KEYS.contains(&k) => FieldKind::StateProperty(k.to_string()),
        k => FieldKind::ConfigProperty(k.to_string()),
    }
}

pub fn is_state_property(name: &str) -> bool {
    STATE_PROPERTY_KEYS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_verb_key() {
        assert_eq!(
            classify_key("verb.wave"),
            FieldKind::Verb("wave".to_string())
        );
    }

    #[test]
    fn classify_status_is_readonly() {
        assert_eq!(classify_key("contents_weight"), FieldKind::Status);
    }

    #[test]
    fn classify_config_property() {
        assert_eq!(
            classify_key("max_weight"),
            FieldKind::ConfigProperty("max_weight".to_string())
        );
    }
}
