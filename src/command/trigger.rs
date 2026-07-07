//! Parser and helpers for runtime `@trigger` builder commands.

use crate::mudl::{parse_trigger_line, validate_event_name, validate_script_code, TriggerDef};
use crate::object::{Behavior, Object, ObjectId, PermissionFlags};
use crate::world::{format_trigger_script, run_event_handlers_on};

/// Parsed `@trigger` subcommand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerCommand {
    Help,
    List { target: String },
    Add {
        target: String,
        event: String,
        code: String,
    },
    Remove {
        target: String,
        event: String,
        index: Option<usize>,
    },
    Clear {
        target: String,
        event: Option<String>,
    },
    Set {
        target: String,
        event: String,
        index: usize,
        code: String,
    },
    Test {
        target: String,
        event: String,
    },
}

/// Errors from `@trigger` parsing or application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerError {
    Usage(String),
    Validation(String),
    NotFound(String),
}

impl std::fmt::Display for TriggerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Usage(msg) | Self::Validation(msg) | Self::NotFound(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for TriggerError {}

impl From<String> for TriggerError {
    fn from(msg: String) -> Self {
        Self::Validation(msg)
    }
}

/// Builder help text for `@trigger`.
pub fn trigger_command_help() -> &'static str {
    "\
@trigger — attach scripted events to places, objects, and creatures

  @trigger help
  @trigger list [target]              list triggers (default: here)
  @trigger <target> <event> <script>   add a trigger (same as add)
  @trigger add <target> <event> <script>
  @trigger remove <target> <event> [n]  remove trigger #n (default: last)
  @trigger clear <target> [event]     remove all triggers (or one event)
  @trigger set <target> <event> <n> <script>  replace trigger #n
  @trigger test <target> <event>    preview narrative output (dry-run)

Targets: object/creature/place name, here/. (current room), me/self (player)

Events: on_enter, on_leave, on_take, on_drop, on_move, on_break, on_kill,
        on_discovered, on_harvest, on_use, on_unlock, on_open, on_weather, …

Scripts: narrate/say/emote/react, damage/heal, mod-stat/mod-skill, set-property,
         grant-effect, remove-effect, cure-tag, teleport, spawn,
         when/if conditionals (effect, condition, not), stop

Examples:
  @trigger here on_enter narrate Silver mist clings to the branches.
  @trigger chest on_open say The lid creaks.
  @trigger path-watcher on_kill grant-effect actor regeneration
  @trigger here on_enter when health below 60 then heal 10
  @trigger list chest
  @trigger remove here on_enter
  @trigger test here on_enter"
}

fn is_event_token(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    lower.starts_with("on_")
}

fn find_event_index(args: &[String]) -> Option<usize> {
    args.iter().position(|t| is_event_token(t))
}

fn join_until_event(args: &[String]) -> (String, usize) {
    match find_event_index(args) {
        Some(idx) => (args[..idx].join(" "), idx),
        None => (args.join(" "), args.len()),
    }
}

/// Parse `@trigger` arguments (everything after the verb).
pub fn parse_trigger_command(args: &[String]) -> Result<TriggerCommand, TriggerError> {
    if args.is_empty() {
        return Ok(TriggerCommand::Help);
    }

    let head = args[0].to_ascii_lowercase();
    match head.as_str() {
        "help" | "?" => Ok(TriggerCommand::Help),
        "list" | "ls" => {
            let target = if args.len() > 1 {
                args[1..].join(" ")
            } else {
                "here".to_string()
            };
            Ok(TriggerCommand::List { target })
        }
        "remove" | "rm" | "delete" | "del" => parse_remove_command(&args[1..]),
        "clear" => parse_clear_command(&args[1..]),
        "set" | "edit" => parse_set_command(&args[1..]),
        "test" | "preview" | "dry-run" | "dryrun" => parse_test_command(&args[1..]),
        "add" => parse_add_command(&args[1..]),
        _ => parse_add_command(args),
    }
}

fn parse_add_command(args: &[String]) -> Result<TriggerCommand, TriggerError> {
    let (target, event_idx) = join_until_event(args);
    let event = args
        .get(event_idx)
        .ok_or_else(|| TriggerError::Usage(add_usage()))?;
    let code = args
        .get(event_idx + 1..)
        .map(|slice| slice.join(" "))
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| TriggerError::Usage(add_usage()))?;

    let target = normalize_target_token(&target);
    validate_event_name(event)?;
    validate_script_code(&code)?;

    Ok(TriggerCommand::Add {
        target,
        event: event.to_ascii_lowercase(),
        code,
    })
}

fn parse_remove_command(args: &[String]) -> Result<TriggerCommand, TriggerError> {
    if args.is_empty() {
        return Err(TriggerError::Usage(
            "usage: @trigger remove <target> <event> [index]".to_string(),
        ));
    }
    let (target, event_idx) = join_until_event(args);
    let event = args
        .get(event_idx)
        .ok_or_else(|| TriggerError::Usage(remove_usage()))?;
    let index = args
        .get(event_idx + 1)
        .map(|s| {
            s.parse::<usize>()
                .map_err(|_| TriggerError::Usage(remove_usage()))
        })
        .transpose()?;

    Ok(TriggerCommand::Remove {
        target: normalize_target_token(&target),
        event: event.to_ascii_lowercase(),
        index,
    })
}

fn parse_clear_command(args: &[String]) -> Result<TriggerCommand, TriggerError> {
    if args.is_empty() {
        return Err(TriggerError::Usage(
            "usage: @trigger clear <target> [event]".to_string(),
        ));
    }
    let event_idx = find_event_index(args);
    let (target, event) = match event_idx {
        None => (args.join(" "), None),
        Some(0) => (String::new(), Some(args[0].to_ascii_lowercase())),
        Some(idx) => (args[..idx].join(" "), Some(args[idx].to_ascii_lowercase())),
    };
    let target = if target.is_empty() && event.is_some() && args.len() == 1 {
        "here".to_string()
    } else {
        normalize_target_token(&target)
    };
    Ok(TriggerCommand::Clear { target, event })
}

fn parse_set_command(args: &[String]) -> Result<TriggerCommand, TriggerError> {
    let (target, event_idx) = join_until_event(args);
    let event = args
        .get(event_idx)
        .ok_or_else(|| TriggerError::Usage(set_usage()))?;
    let index = args
        .get(event_idx + 1)
        .ok_or_else(|| TriggerError::Usage(set_usage()))?
        .parse::<usize>()
        .map_err(|_| TriggerError::Usage(set_usage()))?;
    let code = args
        .get(event_idx + 2..)
        .map(|slice| slice.join(" "))
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| TriggerError::Usage(set_usage()))?;

    validate_event_name(event)?;
    validate_script_code(&code)?;

    Ok(TriggerCommand::Set {
        target: normalize_target_token(&target),
        event: event.to_ascii_lowercase(),
        index,
        code,
    })
}

fn parse_test_command(args: &[String]) -> Result<TriggerCommand, TriggerError> {
    let (target, event_idx) = join_until_event(args);
    let event = args
        .get(event_idx)
        .ok_or_else(|| TriggerError::Usage(test_usage()))?;

    Ok(TriggerCommand::Test {
        target: normalize_target_token(&target),
        event: event.to_ascii_lowercase(),
    })
}

fn normalize_target_token(target: &str) -> String {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        "here".to_string()
    } else {
        trimmed.to_string()
    }
}

fn add_usage() -> String {
    "usage: @trigger <target> <event> <script>  (e.g. @trigger here on_enter narrate Mist rolls in.)"
        .to_string()
}

fn remove_usage() -> String {
    "usage: @trigger remove <target> <event> [index]".to_string()
}

fn set_usage() -> String {
    "usage: @trigger set <target> <event> <index> <script>".to_string()
}

fn test_usage() -> String {
    "usage: @trigger test <target> <event>".to_string()
}

/// Resolve builder target aliases (`here`, `me`) to a lookup name.
pub fn resolve_trigger_target_name(
    target: &str,
    current_location: Option<&ObjectId>,
    player_id: &ObjectId,
) -> String {
    match target.to_ascii_lowercase().as_str() {
        "here" | "." | "room" => current_location
            .map(|id| id.as_str().to_string())
            .unwrap_or_else(|| "here".to_string()),
        "me" | "self" | "player" => player_id.as_str().to_string(),
        _ => target.to_string(),
    }
}

/// Format attached triggers for builder listing.
pub fn format_trigger_list(obj: &Object) -> String {
    if obj.event_handlers.is_empty() {
        return format!("{} has no @trigger handlers.", obj.name);
    }

    let mut events: Vec<_> = obj.event_handlers.keys().collect();
    events.sort();

    let mut lines = vec![format!("{} triggers:", obj.name)];
    for event in events {
        if let Some(handlers) = obj.event_handlers.get(event) {
            for (idx, behavior) in handlers.iter().enumerate() {
                lines.push(format!(
                    "  {}. {event}: {}",
                    idx + 1,
                    behavior.code
                ));
            }
        }
    }
    lines.join("\n")
}

/// Add a trigger to a live object.
pub fn apply_trigger_add(obj: &mut Object, event: &str, code: &str) -> Result<(), TriggerError> {
    validate_event_name(event)?;
    validate_script_code(code)?;
    obj.add_event_handler(
        event.to_string(),
        Behavior {
            code: code.to_string(),
            permissions: PermissionFlags::EVERYONE,
        },
    );
    Ok(())
}

/// Remove one or all triggers for an event. Index is 1-based; `None` removes the last handler.
pub fn apply_trigger_remove(
    obj: &mut Object,
    event: &str,
    index: Option<usize>,
) -> Result<String, TriggerError> {
    let handlers = obj
        .event_handlers
        .get_mut(event)
        .ok_or_else(|| TriggerError::NotFound(format!("no triggers for {event} on {}", obj.name)))?;

    if handlers.is_empty() {
        obj.event_handlers.remove(event);
        return Err(TriggerError::NotFound(format!(
            "no triggers for {event} on {}",
            obj.name
        )));
    }

    let idx = match index {
        Some(one_based) => one_based
            .checked_sub(1)
            .filter(|&i| i < handlers.len())
            .ok_or_else(|| {
                TriggerError::NotFound(format!(
                    "trigger #{one_based} not found for {event} on {} ({} defined)",
                    obj.name,
                    handlers.len()
                ))
            })?,
        None => handlers.len() - 1,
    };

    let removed = handlers.remove(idx).code;
    if handlers.is_empty() {
        obj.event_handlers.remove(event);
    }
    Ok(removed)
}

/// Clear triggers on an object, optionally scoped to one event.
pub fn apply_trigger_clear(obj: &mut Object, event: Option<&str>) -> usize {
    match event {
        Some(name) => obj
            .event_handlers
            .remove(name)
            .map(|v| v.len())
            .unwrap_or(0),
        None => {
            let count: usize = obj.event_handlers.values().map(|v| v.len()).sum();
            obj.event_handlers.clear();
            count
        }
    }
}

/// Replace a trigger script at a 1-based index.
pub fn apply_trigger_set(
    obj: &mut Object,
    event: &str,
    index: usize,
    code: &str,
) -> Result<(), TriggerError> {
    validate_script_code(code)?;
    let handlers = obj
        .event_handlers
        .get_mut(event)
        .ok_or_else(|| TriggerError::NotFound(format!("no triggers for {event} on {}", obj.name)))?;
    let idx = index
        .checked_sub(1)
        .filter(|&i| i < handlers.len())
        .ok_or_else(|| {
            TriggerError::NotFound(format!(
                "trigger #{index} not found for {event} on {} ({} defined)",
                obj.name,
                handlers.len()
            ))
        })?;
    handlers[idx].code = code.to_string();
    Ok(())
}

/// Preview trigger output without side effects.
pub fn preview_trigger_test(obj: &Object, event: &str) -> Vec<String> {
    run_event_handlers_on(obj, event)
}

/// Narrative lines from a dry-run that also checks script formatting per handler.
pub fn preview_trigger_scripts(obj: &Object, event: &str) -> Vec<String> {
    obj.event_handlers
        .get(event)
        .map(|handlers| {
            handlers
                .iter()
                .filter_map(|b| format_trigger_script(obj, &b.code))
                .collect()
        })
        .unwrap_or_default()
}

/// Convert live handlers back to MUDL-style definitions (for export hints).
pub fn triggers_from_object(obj: &Object) -> Vec<TriggerDef> {
    let mut out = Vec::new();
    let mut events: Vec<_> = obj.event_handlers.keys().collect();
    events.sort();
    for event in events {
        if let Some(handlers) = obj.event_handlers.get(event) {
            for behavior in handlers {
                if let Some(def) = parse_trigger_line(&format!("{event} {}", behavior.code)) {
                    out.push(def);
                }
            }
        }
    }
    out
}

/// Builder feedback after adding a trigger.
pub fn narrate_trigger_added(obj: &Object, event: &str, code: &str) -> String {
    format!(
        "Attached @trigger {event} on {}: {code}",
        obj.name
    )
}

/// Builder feedback after removing a trigger.
pub fn narrate_trigger_removed(obj: &Object, event: &str, code: &str) -> String {
    format!(
        "Removed @trigger {event} from {}: {code}",
        obj.name
    )
}

/// Builder feedback after clearing triggers.
pub fn narrate_trigger_cleared(obj: &Object, count: usize, event: Option<&str>) -> String {
    match event {
        Some(ev) => format!("Cleared {count} @trigger handler(s) for {ev} on {}.", obj.name),
        None => format!("Cleared {count} @trigger handler(s) from {}.", obj.name),
    }
}

/// Builder feedback after replacing a trigger.
pub fn narrate_trigger_set(obj: &Object, event: &str, index: usize, code: &str) -> String {
    format!(
        "Set @trigger {event} #{index} on {}: {code}",
        obj.name
    )
}

/// Builder feedback when a test produces no narrative lines.
pub fn narrate_trigger_test_empty(obj: &Object, event: &str) -> String {
    format!(
        "No narrative preview for {event} on {} (handlers may have side effects only).",
        obj.name
    )
}

/// Validate that a target resolved to an attachable object.
pub fn validate_trigger_host(obj: &Object) -> Result<(), TriggerError> {
    if obj.is_deleted {
        return Err(TriggerError::Validation(format!(
            "{} is deleted — restore it before attaching triggers.",
            obj.name
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn sample_object() -> Object {
        Object {
            id: ObjectId::new("area:test-001"),
            name: "Test Room".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new("player:hero-001"),
            permissions: PermissionFlags::EVERYONE,
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
    fn parse_add_shorthand() {
        let cmd = parse_trigger_command(&[
            "here".to_string(),
            "on_enter".to_string(),
            "narrate".to_string(),
            "Mist".to_string(),
            "rolls".to_string(),
            "in.".to_string(),
        ])
        .unwrap();
        assert_eq!(
            cmd,
            TriggerCommand::Add {
                target: "here".to_string(),
                event: "on_enter".to_string(),
                code: "narrate Mist rolls in.".to_string(),
            }
        );
    }

    #[test]
    fn parse_list_defaults_to_here() {
        let cmd = parse_trigger_command(&["list".to_string()]).unwrap();
        assert_eq!(cmd, TriggerCommand::List { target: "here".to_string() });
    }

    #[test]
    fn parse_remove_with_index() {
        let cmd = parse_trigger_command(&[
            "remove".to_string(),
            "chest".to_string(),
            "on_open".to_string(),
            "2".to_string(),
        ])
        .unwrap();
        assert_eq!(
            cmd,
            TriggerCommand::Remove {
                target: "chest".to_string(),
                event: "on_open".to_string(),
                index: Some(2),
            }
        );
    }

    #[test]
    fn parse_when_conditional_add() {
        let cmd = parse_trigger_command(&[
            "here".to_string(),
            "on_enter".to_string(),
            "when".to_string(),
            "health".to_string(),
            "below".to_string(),
            "30".to_string(),
            "then".to_string(),
            "heal".to_string(),
            "15".to_string(),
        ])
        .unwrap();
        assert!(matches!(cmd, TriggerCommand::Add { .. }));
        if let TriggerCommand::Add { code, .. } = cmd {
            assert!(code.starts_with("when health below 30 then heal 15"));
        }
    }

    #[test]
    fn reject_unknown_script_verb() {
        let err = parse_trigger_command(&[
            "here".to_string(),
            "on_enter".to_string(),
            "frobnicate".to_string(),
            "everything".to_string(),
        ])
        .unwrap_err();
        assert!(matches!(err, TriggerError::Validation(_)));
    }

    #[test]
    fn apply_add_list_remove_roundtrip() {
        let mut obj = sample_object();
        apply_trigger_add(&mut obj, "on_enter", "narrate Hello.").unwrap();
        assert!(format_trigger_list(&obj).contains("on_enter"));
        let removed = apply_trigger_remove(&mut obj, "on_enter", None).unwrap();
        assert_eq!(removed, "narrate Hello.");
        assert!(obj.event_handlers.is_empty());
    }

    #[test]
    fn resolve_here_and_me() {
        let loc = ObjectId::new("area:forest-001");
        let player = ObjectId::new("player:hero-001");
        assert_eq!(
            resolve_trigger_target_name("here", Some(&loc), &player),
            "area:forest-001"
        );
        assert_eq!(
            resolve_trigger_target_name("me", Some(&loc), &player),
            "player:hero-001"
        );
    }
}