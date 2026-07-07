//! Creature spawner definitions — weighted templates attached to locations.

use super::npc_def::NpcBehaviorDef;
use super::trigger_def::parse_trigger_line;
use super::TriggerDef;

/// When a spawner attempts to create creatures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnerTrigger {
    OnEnter,
    Periodic,
}

impl SpawnerTrigger {
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "periodic" => Self::Periodic,
            _ => Self::OnEnter,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OnEnter => "on_enter",
            Self::Periodic => "periodic",
        }
    }
}

/// Reusable creature template for spawners (not placed until spawned).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnTemplateDef {
    pub base_name: String,
    pub name: Option<String>,
    pub creature: String,
    pub behaviors: Vec<NpcBehaviorDef>,
    pub use_behaviors: Vec<String>,
    pub triggers: Vec<TriggerDef>,
}

/// Weighted reference to a spawn template inside a spawner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnerEntryDef {
    pub template: String,
    pub weight: u32,
}

/// Spawner attached to a location — controls weighted random creature creation.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnerDef {
    pub base_name: String,
    /// Area base name when the spawner is room-attached.
    pub location: String,
    /// Item instance base name when the spawner is object-attached (ethereal).
    pub target: String,
    pub trigger: SpawnerTrigger,
    /// For `periodic`: attempt every N room entries (default 5).
    pub periodic_interval: u32,
    /// Probability (0.0–1.0) of attempting a spawn when the trigger fires.
    pub chance: f64,
    /// Maximum concurrently active creatures from this spawner in the location.
    pub max_active: u32,
    pub entries: Vec<SpawnerEntryDef>,
}

fn strip_comment(line: &str) -> &str {
    line.split(';').next().unwrap_or(line).trim()
}

fn parse_behavior_line(rest: &str) -> Option<NpcBehaviorDef> {
    let mut parts = rest.split_whitespace();
    let event = parts.next()?.to_string();
    let action = parts.next()?.to_string();
    let text = parts.collect::<Vec<_>>().join(" ").trim().to_string();
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

/// Parse `@spawn-template`, `@spawner`, and related blocks from MUDL source.
pub fn parse_spawner_file(content: &str) -> (Vec<SpawnTemplateDef>, Vec<SpawnerDef>) {
    let mut templates = Vec::new();
    let mut spawners = Vec::new();
    let mut current_template: Option<SpawnTemplateDef> = None;
    let mut current_spawner: Option<SpawnerDef> = None;

    for raw_line in content.lines() {
        let line = strip_comment(raw_line);
        if line.is_empty() {
            continue;
        }
        if line == "@end" {
            if let Some(template) = current_template.take() {
                templates.push(template);
            }
            if let Some(spawner) = current_spawner.take() {
                spawners.push(spawner);
            }
            continue;
        }

        if let Some(name) = line.strip_prefix("@spawn-template ") {
            if let Some(spawner) = current_spawner.take() {
                spawners.push(spawner);
            }
            if let Some(template) = current_template.take() {
                templates.push(template);
            }
            current_template = Some(SpawnTemplateDef {
                base_name: name.trim().to_string(),
                name: None,
                creature: "human".to_string(),
                behaviors: Vec::new(),
                use_behaviors: Vec::new(),
                triggers: Vec::new(),
            });
            continue;
        }

        if let Some(rest) = line.strip_prefix("@trigger ") {
            if let Some(trigger) = parse_trigger_line(rest) {
                if let Some(template) = &mut current_template {
                    template.triggers.push(trigger);
                }
            }
            continue;
        }

        if let Some(name) = line.strip_prefix("@use-behavior ") {
            if let Some(template) = &mut current_template {
                let behavior = name.trim().to_string();
                if !behavior.is_empty() && !template.use_behaviors.contains(&behavior) {
                    template.use_behaviors.push(behavior);
                }
            }
            continue;
        }

        if let Some(name) = line.strip_prefix("@spawner ") {
            if let Some(template) = current_template.take() {
                templates.push(template);
            }
            if let Some(spawner) = current_spawner.take() {
                spawners.push(spawner);
            }
            current_spawner = Some(SpawnerDef {
                base_name: name.trim().to_string(),
                location: String::new(),
                target: String::new(),
                trigger: SpawnerTrigger::OnEnter,
                periodic_interval: 5,
                chance: 1.0,
                max_active: 1,
                entries: Vec::new(),
            });
            continue;
        }

        if let Some(rest) = line.strip_prefix("@entry ") {
            if let Some(spawner) = &mut current_spawner {
                let mut parts = rest.split_whitespace();
                let template = parts.next().unwrap_or("").to_string();
                let mut weight = 1u32;
                for part in parts {
                    if let Some((key, value)) = part.split_once('=') {
                        if key.eq_ignore_ascii_case("weight") {
                            weight = value.parse().unwrap_or(1);
                        }
                    }
                }
                if !template.is_empty() {
                    spawner.entries.push(SpawnerEntryDef { template, weight });
                }
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("@behavior ") {
            if let Some(behavior) = parse_behavior_line(rest) {
                if let Some(template) = &mut current_template {
                    template.behaviors.push(behavior);
                }
            }
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_lowercase();
            let value = value.trim();
            if let Some(template) = &mut current_template {
                match key.as_str() {
                    "name" => template.name = Some(value.to_string()),
                    "creature" => template.creature = value.to_string(),
                    _ => {}
                }
            }
            if let Some(spawner) = &mut current_spawner {
                match key.as_str() {
                    "location" => spawner.location = value.to_string(),
                    "target" | "attach" => spawner.target = value.to_string(),
                    "trigger" => spawner.trigger = SpawnerTrigger::parse(value),
                    "periodic_interval" => {
                        spawner.periodic_interval = value.parse().unwrap_or(5).max(1)
                    }
                    "chance" => {
                        spawner.chance = value.parse::<f64>().unwrap_or(1.0).clamp(0.0, 1.0)
                    }
                    "max_active" => spawner.max_active = value.parse().unwrap_or(1),
                    _ => {}
                }
            }
        }
    }

    if let Some(template) = current_template {
        templates.push(template);
    }
    if let Some(spawner) = current_spawner {
        spawners.push(spawner);
    }

    (templates, spawners)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_spawn_templates_and_spawner() {
        let content = r#"
@spawn-template mist-wisp
  name=Mist Wisp
  creature=human
  @trigger on_enter emote drifts through the air.
@end
@spawner moon-phantoms
  location=haunted-moon
  trigger=on_enter
  chance=0.5
  max_active=2
  @entry mist-wisp weight=3
  @entry lurker weight=1
@end
"#;
        let (templates, spawners) = parse_spawner_file(content);
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].base_name, "mist-wisp");
        assert_eq!(templates[0].triggers.len(), 1);
        assert_eq!(templates[0].triggers[0].event, "on_enter");
        assert_eq!(spawners.len(), 1);
        assert_eq!(spawners[0].location, "haunted-moon");
        assert_eq!(spawners[0].entries.len(), 2);
        assert_eq!(spawners[0].entries[0].weight, 3);
    }

    #[test]
    fn parse_pale_lurker_on_discovered_triggers() {
        let content = include_str!(
            "../../modules/default/worlds/default_world/expansions/haunted_forest.mudl"
        );
        let (templates, _) = parse_spawner_file(content);
        let lurker = templates
            .iter()
            .find(|t| t.base_name == "pale-lurker")
            .expect("pale-lurker template");
        assert_eq!(lurker.triggers.len(), 3);
        assert_eq!(lurker.triggers[0].event, "on_enter");
        assert_eq!(lurker.triggers[1].event, "on_discovered");
        assert_eq!(lurker.triggers[1].code, "react attack");
        assert_eq!(lurker.triggers[2].event, "on_discovered");
        assert!(lurker.triggers[2].code.contains("lunges from the shadows"));
    }
}
