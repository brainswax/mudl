//! Loot spawner definitions — weighted item templates attached to locations or objects.
//!
//! TODO: Resource Spawner — extend this model for crafting materials (ore nodes, herb patches,
//! renewable harvest with cooldown) once crafting milestones land.

use super::spawner_def::SpawnerEntryDef;

/// When a loot spawner attempts to create items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LootSpawnerTrigger {
    OnEnter,
    OnOpen,
    OnKill,
    OnBreak,
    Timer,
}

impl LootSpawnerTrigger {
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "on_open" | "open" => Self::OnOpen,
            "on_kill" | "kill" => Self::OnKill,
            "on_break" | "break" => Self::OnBreak,
            "timer" | "periodic" => Self::Timer,
            _ => Self::OnEnter,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OnEnter => "on_enter",
            Self::OnOpen => "on_open",
            Self::OnKill => "on_kill",
            Self::OnBreak => "on_break",
            Self::Timer => "timer",
        }
    }
}

/// Reusable loot template (references an item prototype, not placed until spawned).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LootTemplateDef {
    pub base_name: String,
    /// Item prototype base name (`trail-rations`, `gold-coins`, …).
    pub prototype: String,
    /// Stack count when the prototype is stackable (default 1).
    pub count: u32,
}

/// Loot spawner attached to a location, object, or container.
#[derive(Debug, Clone, PartialEq)]
pub struct LootSpawnerDef {
    pub base_name: String,
    /// Target base name — area (`haunted-shrine`) or item instance (`scene-chest`).
    pub target: String,
    pub trigger: LootSpawnerTrigger,
    /// For `timer`: attempt every N trigger ticks (default 5).
    pub periodic_interval: u32,
    /// Probability (0.0–1.0) of attempting loot when the trigger fires.
    pub chance: f64,
    /// Maximum concurrently active loot items from this spawner at the target.
    pub max_active: u32,
    /// When true, the spawner fires at most once per game (good for chest surprises).
    pub once: bool,
    pub entries: Vec<SpawnerEntryDef>,
}

fn strip_comment(line: &str) -> &str {
    line.split(';').next().unwrap_or(line).trim()
}

/// Parse `@loot-template`, `@loot-spawner`, and related blocks from MUDL source.
pub fn parse_loot_spawner_file(content: &str) -> (Vec<LootTemplateDef>, Vec<LootSpawnerDef>) {
    let mut templates = Vec::new();
    let mut spawners = Vec::new();
    let mut current_template: Option<LootTemplateDef> = None;
    let mut current_spawner: Option<LootSpawnerDef> = None;

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

        if let Some(name) = line.strip_prefix("@loot-template ") {
            if let Some(spawner) = current_spawner.take() {
                spawners.push(spawner);
            }
            if let Some(template) = current_template.take() {
                templates.push(template);
            }
            current_template = Some(LootTemplateDef {
                base_name: name.trim().to_string(),
                prototype: String::new(),
                count: 1,
            });
            continue;
        }

        if let Some(name) = line.strip_prefix("@loot-spawner ") {
            if let Some(template) = current_template.take() {
                templates.push(template);
            }
            if let Some(spawner) = current_spawner.take() {
                spawners.push(spawner);
            }
            current_spawner = Some(LootSpawnerDef {
                base_name: name.trim().to_string(),
                target: String::new(),
                trigger: LootSpawnerTrigger::OnOpen,
                periodic_interval: 5,
                chance: 1.0,
                max_active: 4,
                once: false,
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

        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_lowercase();
            let value = value.trim();
            if let Some(template) = &mut current_template {
                match key.as_str() {
                    "prototype" => template.prototype = value.to_string(),
                    "count" => template.count = value.parse().unwrap_or(1).max(1),
                    _ => {}
                }
            }
            if let Some(spawner) = &mut current_spawner {
                match key.as_str() {
                    "target" | "location" | "attach" => spawner.target = value.to_string(),
                    "trigger" => spawner.trigger = LootSpawnerTrigger::parse(value),
                    "periodic_interval" | "interval" => {
                        spawner.periodic_interval = value.parse().unwrap_or(5).max(1)
                    }
                    "chance" => {
                        spawner.chance = value.parse::<f64>().unwrap_or(1.0).clamp(0.0, 1.0)
                    }
                    "max_active" => spawner.max_active = value.parse().unwrap_or(4),
                    "once" => spawner.once = value == "true",
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
    fn parse_loot_templates_and_spawner() {
        let content = r#"
@loot-template bonus-rations
  prototype=trail-rations
  count=2
@end
@loot-spawner chest-bonus
  target=scene-chest
  trigger=on_open
  once=true
  chance=1.0
  max_active=1
  @entry bonus-rations weight=3
@end
"#;
        let (templates, spawners) = parse_loot_spawner_file(content);
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].prototype, "trail-rations");
        assert_eq!(templates[0].count, 2);
        assert_eq!(spawners.len(), 1);
        assert_eq!(spawners[0].target, "scene-chest");
        assert_eq!(spawners[0].trigger, LootSpawnerTrigger::OnOpen);
        assert!(spawners[0].once);
        assert_eq!(spawners[0].entries[0].weight, 3);
    }
}
