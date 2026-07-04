//! Parser and helpers for `@set` / `@unset` builder commands.

use std::collections::HashMap;

use crate::object::{set_field, unset_field, EditError, Object, ObjectId};

/// Parsed `@set` command body (without the `@set` verb).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSetCommand {
    pub target: String,
    pub key: String,
    pub value: String,
}

/// Parsed `@unset` command body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedUnsetCommand {
    pub target: String,
    pub key: String,
}

/// Parse `@set <target> <key> <value...>`.
pub fn parse_set_command(args: &[String]) -> Result<ParsedSetCommand, EditError> {
    if args.len() < 3 {
        return Err(EditError::InvalidValue(
            "usage: @set <target> <key> <value>".to_string(),
        ));
    }
    Ok(ParsedSetCommand {
        target: args[0].clone(),
        key: args[1].clone(),
        value: args[2..].join(" "),
    })
}

/// Parse `@unset <target> <key>`.
pub fn parse_unset_command(args: &[String]) -> Result<ParsedUnsetCommand, EditError> {
    if args.len() < 2 {
        return Err(EditError::InvalidValue(
            "usage: @unset <target> <key>".to_string(),
        ));
    }
    Ok(ParsedUnsetCommand {
        target: args[0].clone(),
        key: args[1].clone(),
    })
}

/// Apply a parsed `@set` to an object.
pub fn apply_set(
    obj: &mut Object,
    key: &str,
    value: &str,
    observer: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> Result<(), EditError> {
    set_field(obj, key, value, observer, objects)
}

/// Apply a parsed `@unset` to an object.
pub fn apply_unset(obj: &mut Object, key: &str) -> Result<(), EditError> {
    unset_field(obj, key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_set_command_splits_value() {
        let args = vec![
            "backpack".to_string(),
            "verb.wave".to_string(),
            "say('hi')".to_string(),
            "extra".to_string(),
        ];
        let parsed = parse_set_command(&args).unwrap();
        assert_eq!(parsed.target, "backpack");
        assert_eq!(parsed.key, "verb.wave");
        assert_eq!(parsed.value, "say('hi') extra");
    }

    #[test]
    fn parse_unset_args() {
        let args = vec!["purse".to_string(), "weight".to_string()];
        let parsed = super::parse_unset_command(&args).unwrap();
        assert_eq!(parsed.target, "purse");
        assert_eq!(parsed.key, "weight");
    }
}