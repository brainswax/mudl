//! MUDL `@trigger` definitions — attach scripted events to places and objects.

use crate::world::event_script::{parse_script, ScriptAction};

/// A single trigger script bound to an event name (`on_enter`, `on_break`, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerDef {
    pub event: String,
    pub code: String,
}

/// Parse `@trigger <event> <action> [text...]` or `@trigger <event> <full script>`.
pub fn parse_trigger_line(rest: &str) -> Option<TriggerDef> {
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    let mut parts = rest.splitn(2, char::is_whitespace);
    let event = parts.next()?.trim().to_lowercase();
    let code = parts.next()?.trim().to_string();
    if event.is_empty() || code.is_empty() {
        return None;
    }
    Some(TriggerDef { event, code })
}

/// Well-known world events builders can attach triggers to.
pub mod events {
    pub const ON_ENTER: &str = "on_enter";
    pub const ON_LEAVE: &str = "on_leave";
    pub const ON_TAKE: &str = "on_take";
    pub const ON_DROP: &str = "on_drop";
    pub const ON_MOVE: &str = "on_move";
    pub const ON_BREAK: &str = "on_break";
    pub const ON_DEATH: &str = "on_death";
    pub const ON_KILL: &str = "on_kill";
    pub const ON_DISCOVERED: &str = "on_discovered";
    pub const ON_UNLOCK: &str = "on_unlock";
    pub const ON_OPEN: &str = "on_open";
    pub const ON_HARVEST: &str = "on_harvest";
    pub const ON_USE: &str = "on_use";
    /// Custom timed events — typically fired by `@schedule` jobs.
    pub const ON_WEATHER: &str = "on_weather";
    pub const ON_RESPAWN: &str = "on_respawn";
}

/// Built-in event names builders can attach triggers to.
pub fn known_events() -> &'static [&'static str] {
    &[
        events::ON_ENTER,
        events::ON_LEAVE,
        events::ON_TAKE,
        events::ON_DROP,
        events::ON_MOVE,
        events::ON_BREAK,
        events::ON_DEATH,
        events::ON_KILL,
        events::ON_DISCOVERED,
        events::ON_UNLOCK,
        events::ON_OPEN,
        events::ON_HARVEST,
        events::ON_USE,
        events::ON_WEATHER,
        events::ON_RESPAWN,
    ]
}

/// Validate an event name for runtime/MUDL triggers.
pub fn validate_event_name(event: &str) -> Result<(), String> {
    let event = event.trim().to_ascii_lowercase();
    if event.is_empty() {
        return Err("event name is required".to_string());
    }
    if !event.starts_with("on_") {
        return Err(format!(
            "event '{event}' should start with on_ (e.g. on_enter, on_kill)"
        ));
    }
    if event.contains(char::is_whitespace) {
        return Err("event name cannot contain spaces".to_string());
    }
    Ok(())
}

fn is_known_script_verb(verb: &str) -> bool {
    matches!(
        verb.to_ascii_lowercase().as_str(),
        "narrate"
            | "message"
            | "say"
            | "emote"
            | "react"
            | "damage"
            | "heal"
            | "mod-stat"
            | "mod_stat"
            | "mod-skill"
            | "mod_skill"
            | "set-property"
            | "set_property"
            | "set"
            | "grant-effect"
            | "grant_effect"
            | "effect"
            | "remove-effect"
            | "remove_effect"
            | "cure-effect"
            | "cure_effect"
            | "cure-tag"
            | "cure_tag"
            | "cure"
            | "teleport"
            | "send"
            | "spawn"
            | "when"
            | "if"
            | "stop"
            | "cancel"
            | "halt"
            | "attack"
            | "flee"
            | "greet"
            | "warn"
    )
}

/// Validate script code parses to a recognized action.
pub fn validate_script_code(code: &str) -> Result<(), String> {
    let code = code.trim();
    if code.is_empty() {
        return Err("script code is required".to_string());
    }

    let action = parse_script(code);
    if (code.starts_with("when ") || code.starts_with("if "))
        && matches!(action, ScriptAction::Raw(_))
    {
        return Err(
            "unrecognized conditional — use: when <condition> then <action>  \
             (e.g. when health below 30 then heal 15)"
                .to_string(),
        );
    }

    if let ScriptAction::Raw(raw) = &action {
        let verb = raw.split_whitespace().next().unwrap_or("");
        if !is_known_script_verb(verb) {
            return Err(format!(
                "unknown script verb '{verb}' — try: narrate, say, emote, react, damage, heal, \
                 mod-stat, grant-effect, teleport, spawn, when … then …"
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_event_and_script() {
        assert!(validate_event_name("on_enter").is_ok());
        assert!(validate_event_name("enter").is_err());
        assert!(validate_script_code("narrate Hello.").is_ok());
        assert!(validate_script_code("frobnicate x").is_err());
        assert!(validate_script_code("when health below 30 then heal 15").is_ok());
    }

    #[test]
    fn parse_trigger_narrate_and_emote() {
        let t = parse_trigger_line("on_enter narrate Silver mist clings to the branches.").unwrap();
        assert_eq!(t.event, "on_enter");
        assert_eq!(t.code, "narrate Silver mist clings to the branches.");

        let e = parse_trigger_line("on_break emote shatters into pale dust.").unwrap();
        assert_eq!(e.event, "on_break");
        assert_eq!(e.code, "emote shatters into pale dust.");
    }
}