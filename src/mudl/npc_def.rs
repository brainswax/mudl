//! NPC spawn definitions and scripted behaviors from MUDL.

use std::collections::HashMap;

use super::behavior_line::parse_behavior_line;
use super::trigger_def::parse_trigger_line;
use super::CreatureReact;
use super::TriggerDef;

/// A single scripted behavior on an NPC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpcBehaviorDef {
    pub event: String,
    pub action: String,
    pub text: String,
    /// When set, this entry is a react directive (`@behavior on_discovered react flee`).
    pub react: Option<CreatureReact>,
}

/// An NPC instance defined in MUDL and spawned at bootstrap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpcDef {
    pub base_name: String,
    pub name: Option<String>,
    pub creature: String,
    pub location: String,
    pub behaviors: Vec<NpcBehaviorDef>,
    /// `@use-behavior` template names applied at bootstrap.
    pub use_behaviors: Vec<String>,
    pub triggers: Vec<TriggerDef>,
}

fn strip_comment(line: &str) -> &str {
    line.split(';').next().unwrap_or(line).trim()
}

/// Parse `@npc` blocks from MUDL source.
pub fn parse_npc_file(content: &str) -> Vec<NpcDef> {
    let mut npcs = Vec::new();
    let mut current: Option<NpcDef> = None;

    for raw_line in content.lines() {
        let line = strip_comment(raw_line);
        if line.is_empty() {
            continue;
        }
        if line == "@end" {
            if let Some(npc) = current.take() {
                npcs.push(npc);
            }
            continue;
        }
        if let Some(name) = line.strip_prefix("@npc ") {
            current = Some(NpcDef {
                base_name: name.trim().to_string(),
                name: None,
                creature: "human".to_string(),
                location: String::new(),
                behaviors: Vec::new(),
                use_behaviors: Vec::new(),
                triggers: Vec::new(),
            });
            continue;
        }
        if let Some(rest) = line.strip_prefix("@trigger ") {
            if let Some(trigger) = parse_trigger_line(rest) {
                if let Some(npc) = &mut current {
                    npc.triggers.push(trigger);
                }
            }
            continue;
        }
        if let Some(name) = line.strip_prefix("@use-behavior ") {
            if let Some(npc) = &mut current {
                let template = name.trim().to_string();
                if !template.is_empty() && !npc.use_behaviors.contains(&template) {
                    npc.use_behaviors.push(template);
                }
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("@behavior ") {
            if let Some(behavior) = parse_behavior_line(rest) {
                if let Some(npc) = &mut current {
                    npc.behaviors.push(behavior);
                }
            }
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            if let Some(npc) = &mut current {
                match key.trim().to_lowercase().as_str() {
                    "name" => npc.name = Some(value.trim().to_string()),
                    "creature" => npc.creature = value.trim().to_string(),
                    "location" => npc.location = value.trim().to_string(),
                    _ => {}
                }
            }
        }
    }

    npcs
}

/// Serialize behaviors for storage on the NPC object.
pub fn behaviors_to_values(behaviors: &[NpcBehaviorDef]) -> Vec<crate::object::Value> {
    behaviors
        .iter()
        .map(|behavior| {
            let mut map = HashMap::from([
                (
                    "event".to_string(),
                    crate::object::Value::String(behavior.event.clone()),
                ),
                (
                    "action".to_string(),
                    crate::object::Value::String(behavior.action.clone()),
                ),
                (
                    "text".to_string(),
                    crate::object::Value::String(behavior.text.clone()),
                ),
            ]);
            if let Some(react) = behavior.react {
                map.insert(
                    "react".to_string(),
                    crate::object::Value::String(react.as_str().to_string()),
                );
            }
            crate::object::Value::Map(map)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_npc_with_behaviors() {
        let content = include_str!("../../modules/default/worlds/default_world/npcs.mudl");
        let npcs = parse_npc_file(content);
        assert_eq!(npcs.len(), 1);
        assert_eq!(npcs[0].base_name, "path-watcher");
        assert_eq!(npcs[0].location, "forest-path");
        assert!(npcs[0].behaviors.is_empty());
        assert_eq!(npcs[0].triggers.len(), 4);
        assert_eq!(npcs[0].triggers[0].event, "on_enter");
        assert!(npcs[0].triggers[0].code.contains("trees seem to lean closer"));
    }
}
