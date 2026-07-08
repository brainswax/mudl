//! Shared `@behavior` line parser for NPC and spawn-template definitions.

use super::behavior_def::CreatureReact;
use super::npc_def::NpcBehaviorDef;

/// Parse `@behavior <event> <action> <text…>` or react shorthand.
///
/// Supported forms:
/// - `on_enter emote waves slowly.` — scripted action + text
/// - `on_discovered react flee` — explicit react directive
/// - `on_enter attack` — react shorthand when action is a known react verb
pub fn parse_behavior_line(rest: &str) -> Option<NpcBehaviorDef> {
    let mut parts = rest.split_whitespace();
    let event = parts.next()?.to_string();
    let action = parts.next()?.to_string();
    let text = parts.collect::<Vec<_>>().join(" ").trim().to_string();
    if action == "react" && !text.is_empty() {
        return Some(NpcBehaviorDef {
            event,
            action: String::new(),
            text: String::new(),
            react: Some(CreatureReact::parse(&text)),
        });
    }
    if text.is_empty() && matches!(event.as_str(), "on_discovered" | "on_enter") {
        let react = CreatureReact::parse(&action);
        if react != CreatureReact::Ignore || action.eq_ignore_ascii_case("ignore") {
            return Some(NpcBehaviorDef {
                event,
                action: String::new(),
                text: String::new(),
                react: Some(react),
            });
        }
        return None;
    }
    if text.is_empty() {
        return None;
    }
    Some(NpcBehaviorDef {
        event,
        action,
        text,
        react: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mudl::CreatureReact;

    #[test]
    fn parse_scripted_behavior() {
        let behavior = parse_behavior_line("on_enter emote drifts through the air.")
            .expect("scripted behavior");
        assert_eq!(behavior.event, "on_enter");
        assert_eq!(behavior.action, "emote");
        assert_eq!(behavior.text, "drifts through the air.");
        assert!(behavior.react.is_none());
    }

    #[test]
    fn parse_explicit_react() {
        let behavior = parse_behavior_line("on_discovered react flee").expect("react behavior");
        assert_eq!(behavior.event, "on_discovered");
        assert_eq!(behavior.react, Some(CreatureReact::Flee));
    }

    #[test]
    fn parse_react_shorthand() {
        let behavior = parse_behavior_line("on_enter attack").expect("react shorthand");
        assert_eq!(behavior.event, "on_enter");
        assert_eq!(behavior.react, Some(CreatureReact::Attack));
    }

    #[test]
    fn reject_empty_text_for_non_react() {
        assert!(parse_behavior_line("on_enter emote").is_none());
    }
}