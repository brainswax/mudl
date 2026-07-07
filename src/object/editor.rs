//! Safe builder mutations: `@set` and `@unset`.

use std::collections::HashMap;

use crate::display::{name_matches, short_id};

use super::fields::{classify_key, normalize_key, FieldKind};
use super::{Object, ObjectId, PermissionFlags, Property, Value, Verb};

/// Errors from builder set/unset operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    ReadOnly(String),
    InvalidValue(String),
    NotFound(String),
    Validation(String),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadOnly(msg)
            | Self::InvalidValue(msg)
            | Self::NotFound(msg)
            | Self::Validation(msg) => {
                write!(f, "{msg}")
            }
        }
    }
}

impl std::error::Error for EditError {}

/// Parse a literal value for `@set`.
pub fn parse_value_literal(input: &str) -> Result<Value, EditError> {
    let trimmed = input.trim();
    if trimmed.eq_ignore_ascii_case("true") {
        return Ok(Value::Bool(true));
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return Ok(Value::Bool(false));
    }
    if let Ok(n) = trimmed.parse::<i64>() {
        return Ok(Value::Int(n));
    }
    if let Ok(f) = trimmed.parse::<f64>() {
        if f.is_finite() {
            return Ok(Value::Float(f));
        }
    }
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        return Ok(Value::String(trimmed[1..trimmed.len() - 1].to_string()));
    }
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        return parse_list_literal(&trimmed[1..trimmed.len() - 1]);
    }
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return parse_map_literal(&trimmed[1..trimmed.len() - 1]);
    }
    Ok(Value::String(trimmed.to_string()))
}

fn parse_list_literal(inner: &str) -> Result<Value, EditError> {
    if inner.trim().is_empty() {
        return Ok(Value::List(vec![]));
    }
    let mut items = Vec::new();
    for part in split_top_level(inner, ',') {
        items.push(parse_value_literal(part.trim())?);
    }
    Ok(Value::List(items))
}

fn parse_map_literal(inner: &str) -> Result<Value, EditError> {
    if inner.trim().is_empty() {
        return Ok(Value::Map(HashMap::new()));
    }
    let mut map = HashMap::new();
    for part in split_top_level(inner, ',') {
        let (key, value) = part
            .split_once(':')
            .ok_or_else(|| EditError::InvalidValue(format!("invalid map entry: {part}")))?;
        let key = normalize_key(key);
        map.insert(key, parse_value_literal(value.trim())?);
    }
    Ok(Value::Map(map))
}

fn split_top_level(input: &str, sep: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut in_quote = None;

    for ch in input.chars() {
        match ch {
            '"' | '\'' if in_quote == Some(ch) => in_quote = None,
            '"' | '\'' if in_quote.is_none() => in_quote = Some(ch),
            '[' | '{' if in_quote.is_none() => depth += 1,
            ']' | '}' if in_quote.is_none() => depth -= 1,
            c if c == sep && depth == 0 && in_quote.is_none() => {
                parts.push(current.trim().to_string());
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

/// Resolve a token to an object ID (full id, short id, `you`, or display name).
pub fn resolve_object_ref(
    token: &str,
    observer: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> Result<ObjectId, EditError> {
    let token = token.trim();
    if token.is_empty() || token.eq_ignore_ascii_case("none") {
        return Err(EditError::NotFound("none".to_string()));
    }
    if token.eq_ignore_ascii_case("you") || token.eq_ignore_ascii_case("me") {
        return Ok(observer.clone());
    }
    if token.contains(':') {
        let id = ObjectId::new(token);
        if objects.contains_key(&id) {
            return Ok(id);
        }
        return Err(EditError::NotFound(token.to_string()));
    }

    let needle = token.to_ascii_lowercase();
    let mut matches: Vec<&Object> = objects
        .values()
        .filter(|obj| {
            obj.is_active() && (short_id(&obj.id) == needle || name_matches(&needle, obj))
        })
        .collect();
    matches.sort_by_key(|obj| obj.id.as_str());

    match matches.len() {
        0 => Err(EditError::NotFound(token.to_string())),
        1 => Ok(matches[0].id.clone()),
        _ => Err(EditError::Validation(format!(
            "ambiguous object reference: {token}"
        ))),
    }
}

fn resolve_value_refs(
    value: Value,
    observer: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> Result<Value, EditError> {
    match value {
        Value::String(s) => {
            if s.contains(':') || s.eq_ignore_ascii_case("you") || s.eq_ignore_ascii_case("me") {
                if let Ok(id) = resolve_object_ref(&s, observer, objects) {
                    return Ok(Value::ObjectRef(id));
                }
            }
            if let Ok(id) = resolve_object_ref(&s, observer, objects) {
                return Ok(Value::ObjectRef(id));
            }
            Ok(Value::String(s))
        }
        Value::List(items) => Ok(Value::List(
            items
                .into_iter()
                .map(|v| resolve_value_refs(v, observer, objects))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Value::Map(map) => {
            let mut resolved = HashMap::new();
            for (k, v) in map {
                resolved.insert(k, resolve_value_refs(v, observer, objects)?);
            }
            Ok(Value::Map(resolved))
        }
        other => Ok(other),
    }
}

fn validate_config_value(key: &str, value: &Value) -> Result<(), EditError> {
    match key {
        "weight" | "volume" => {
            if !matches!(value, Value::Int(_) | Value::Float(_)) {
                return Err(EditError::InvalidValue(format!("{key} requires a number")));
            }
        }
        "capacity" | "max_weight" | "max_volume" | "stack_count" | "max_stack" => {
            if !matches!(value, Value::Int(_)) {
                return Err(EditError::InvalidValue(format!(
                    "{key} requires an integer"
                )));
            }
        }
        "is_container" | "is_wearable" | "is_pocketable" | "stackable"
            if !matches!(value, Value::Bool(_)) =>
        {
            return Err(EditError::InvalidValue(format!(
                "{key} requires true or false"
            )));
        }
        _ => {}
    }
    Ok(())
}

/// Apply `@set <key> <value>` to an object.
pub fn set_field(
    obj: &mut Object,
    key: &str,
    raw_value: &str,
    observer: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> Result<(), EditError> {
    let kind = classify_key(key);
    match kind {
        FieldKind::Immutable(field) => {
            Err(EditError::ReadOnly(format!("{field} cannot be changed")))
        }
        FieldKind::Status => Err(EditError::ReadOnly(
            "status fields are calculated; use @examine to view".to_string(),
        )),
        FieldKind::Verb(name) => {
            let code = raw_value.trim().to_string();
            if code.is_empty() {
                return Err(EditError::InvalidValue(
                    "verb code cannot be empty".to_string(),
                ));
            }
            obj.add_verb(Verb {
                name: name.clone(),
                code,
                permissions: PermissionFlags::OWNER,
            });
            Ok(())
        }
        FieldKind::ObjectField(field) => match field {
            "name" => {
                let name = raw_value.trim();
                if name.is_empty() {
                    return Err(EditError::InvalidValue("name cannot be empty".to_string()));
                }
                obj.name = name.to_string();
                Ok(())
            }
            "owner" => {
                obj.owner = resolve_object_ref(raw_value, observer, objects)?;
                Ok(())
            }
            "location" => {
                obj.location = Some(resolve_object_ref(raw_value, observer, objects)?);
                Ok(())
            }
            "prototype" => {
                obj.prototype = Some(resolve_object_ref(raw_value, observer, objects)?);
                Ok(())
            }
            "alias" => {
                let alias = raw_value.trim().to_string();
                if alias.is_empty() {
                    return Err(EditError::InvalidValue("alias cannot be empty".to_string()));
                }
                if !obj.aliases.iter().any(|a| a == &alias) {
                    obj.aliases.push(alias);
                }
                Ok(())
            }
            other => Err(EditError::Validation(format!(
                "unknown object field: {other}"
            ))),
        },
        FieldKind::StateProperty(state_key) => {
            let value = parse_value_literal(raw_value)?;
            let value = resolve_value_refs(value, observer, objects)?;
            obj.add_property(Property {
                name: state_key,
                value,
                permissions: PermissionFlags::OWNER,
                behavior: None,
            });
            Ok(())
        }
        FieldKind::ConfigProperty(config_key) => {
            let value = parse_value_literal(raw_value)?;
            let value = resolve_value_refs(value, observer, objects)?;
            validate_config_value(&config_key, &value)?;
            obj.add_property(Property {
                name: config_key,
                value,
                permissions: PermissionFlags::OWNER,
                behavior: None,
            });
            Ok(())
        }
    }
}

/// Apply `@unset <key>` to an object.
pub fn unset_field(obj: &mut Object, key: &str) -> Result<(), EditError> {
    let kind = classify_key(key);
    match kind {
        FieldKind::Immutable(field) => Err(EditError::ReadOnly(format!("{field} cannot be unset"))),
        FieldKind::Status => Err(EditError::ReadOnly(
            "status fields are calculated and cannot be unset".to_string(),
        )),
        FieldKind::Verb(name) => {
            if obj.verbs.remove(&name).is_none() {
                return Err(EditError::NotFound(format!("verb.{name}")));
            }
            Ok(())
        }
        FieldKind::ObjectField(field) => match field {
            "name" => Err(EditError::ReadOnly("name cannot be unset".to_string())),
            "owner" => Err(EditError::ReadOnly("owner cannot be unset".to_string())),
            "location" => {
                obj.location = None;
                Ok(())
            }
            "prototype" => {
                obj.prototype = None;
                Ok(())
            }
            "alias" => {
                obj.aliases.clear();
                Ok(())
            }
            other => Err(EditError::ReadOnly(format!(
                "{other} cannot be unset; use @set to change it"
            ))),
        },
        FieldKind::StateProperty(key) | FieldKind::ConfigProperty(key) => {
            if obj.properties.remove(&key).is_none() {
                return Err(EditError::NotFound(key));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::ContainerSpec;

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
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        }
    }

    fn objects_with(objs: Vec<Object>) -> HashMap<ObjectId, Object> {
        objs.into_iter().map(|o| (o.id.clone(), o)).collect()
    }

    #[test]
    fn set_config_property_weight() {
        let mut backpack = bare("item:backpack-001", "backpack");
        let objects = objects_with(vec![backpack.clone()]);
        let observer = ObjectId::new("player:admin-001");

        set_field(&mut backpack, "weight", "10", &observer, &objects).unwrap();
        assert!((backpack.get_numeric_property("weight").unwrap() - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn set_state_location_by_name() {
        let mut backpack = bare("item:backpack-001", "backpack");
        let admin = bare("player:admin-001", "Admin");
        let objects = objects_with(vec![backpack.clone(), admin]);
        let observer = ObjectId::new("player:admin-001");

        set_field(&mut backpack, "location", "Admin", &observer, &objects).unwrap();
        assert_eq!(
            backpack.location.as_ref(),
            Some(&ObjectId::new("player:admin-001"))
        );
    }

    #[test]
    fn set_verb() {
        let mut sword = bare("item:sword-001", "sword");
        let objects = HashMap::new();
        let observer = ObjectId::new("player:admin-001");

        set_field(
            &mut sword,
            "verb.wave",
            "say('You wave.')",
            &observer,
            &objects,
        )
        .unwrap();
        assert!(sword.verbs.contains_key("wave"));
    }

    #[test]
    fn set_status_rejected() {
        let mut backpack = bare("item:backpack-001", "backpack");
        let err = set_field(
            &mut backpack,
            "contents_weight",
            "0",
            &ObjectId::new("player:admin-001"),
            &HashMap::new(),
        )
        .unwrap_err();
        assert!(matches!(err, EditError::ReadOnly(_)));
    }

    #[test]
    fn unset_property_and_location() {
        let mut backpack = bare("item:backpack-001", "backpack");
        backpack.set_property_int("weight", 10);
        backpack.location = Some(ObjectId::new("area:void-001"));

        unset_field(&mut backpack, "weight").unwrap();
        unset_field(&mut backpack, "location").unwrap();

        assert!(backpack.get_numeric_property("weight").is_none());
        assert!(backpack.location.is_none());
    }

    #[test]
    fn set_container_contents_list() {
        let mut purse = bare("item:purse-001", "purse");
        purse.apply_container_role(&ContainerSpec::default());
        let coins = bare("item:coins-001", "coins");
        let objects = objects_with(vec![purse.clone(), coins.clone()]);
        let observer = ObjectId::new("player:admin-001");

        set_field(&mut purse, "contents", "[coins]", &observer, &objects).unwrap();
        let contents = purse.get_object_list_property("contents");
        assert_eq!(contents, vec![coins.id]);
    }

    #[test]
    fn parse_value_literals() {
        assert!(matches!(parse_value_literal("10").unwrap(), Value::Int(10)));
        assert!(matches!(
            parse_value_literal("0.1").unwrap(),
            Value::Float(f) if (f - 0.1).abs() < f64::EPSILON
        ));
        assert!(matches!(
            parse_value_literal("true").unwrap(),
            Value::Bool(true)
        ));
        assert!(matches!(
            parse_value_literal(r#""hello""#).unwrap(),
            Value::String(s) if s == "hello"
        ));
        assert!(matches!(parse_value_literal("[]").unwrap(), Value::List(v) if v.is_empty()));
    }
}
