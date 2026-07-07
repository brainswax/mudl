//! Resource spawner definitions — renewable harvest nodes for crafting materials.

use super::spawner_def::SpawnerEntryDef;

/// When a resource spawner attempts to create items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceSpawnerTrigger {
    OnEnter,
    OnHarvest,
    Timer,
}

impl ResourceSpawnerTrigger {
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "on_harvest" | "harvest" => Self::OnHarvest,
            "timer" | "periodic" => Self::Timer,
            _ => Self::OnEnter,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OnEnter => "on_enter",
            Self::OnHarvest => "on_harvest",
            Self::Timer => "timer",
        }
    }
}

/// Reusable resource template (references an item prototype).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceTemplateDef {
    pub base_name: String,
    pub prototype: String,
    pub count: u32,
}

/// Resource spawner attached to a harvestable object or location.
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceSpawnerDef {
    pub base_name: String,
    pub target: String,
    pub trigger: ResourceSpawnerTrigger,
    pub periodic_interval: u32,
    pub chance: f64,
    pub max_active: u32,
    pub once: bool,
    pub entries: Vec<SpawnerEntryDef>,
}

fn strip_comment(line: &str) -> &str {
    line.split(';').next().unwrap_or(line).trim()
}

/// Parse `@resource-template` and `@resource-spawner` blocks from MUDL source.
pub fn parse_resource_spawner_file(
    content: &str,
) -> (Vec<ResourceTemplateDef>, Vec<ResourceSpawnerDef>) {
    let mut templates = Vec::new();
    let mut spawners = Vec::new();
    let mut current_template: Option<ResourceTemplateDef> = None;
    let mut current_spawner: Option<ResourceSpawnerDef> = None;

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

        if let Some(name) = line.strip_prefix("@resource-template ") {
            if let Some(spawner) = current_spawner.take() {
                spawners.push(spawner);
            }
            if let Some(template) = current_template.take() {
                templates.push(template);
            }
            current_template = Some(ResourceTemplateDef {
                base_name: name.trim().to_string(),
                prototype: String::new(),
                count: 1,
            });
            continue;
        }

        if let Some(name) = line.strip_prefix("@resource-spawner ") {
            if let Some(template) = current_template.take() {
                templates.push(template);
            }
            if let Some(spawner) = current_spawner.take() {
                spawners.push(spawner);
            }
            current_spawner = Some(ResourceSpawnerDef {
                base_name: name.trim().to_string(),
                target: String::new(),
                trigger: ResourceSpawnerTrigger::OnHarvest,
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
                    "trigger" => spawner.trigger = ResourceSpawnerTrigger::parse(value),
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
    fn parse_resource_templates_and_spawner() {
        let content = r#"
@resource-template moon-moss
  prototype=trail-rations
  count=2
@end
@resource-spawner moss-harvest
  target=moss-patch
  trigger=on_harvest
  chance=1.0
  max_active=1
  @entry moon-moss weight=1
@end
"#;
        let (templates, spawners) = parse_resource_spawner_file(content);
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].prototype, "trail-rations");
        assert_eq!(spawners.len(), 1);
        assert_eq!(spawners[0].trigger, ResourceSpawnerTrigger::OnHarvest);
        assert_eq!(spawners[0].target, "moss-patch");
    }
}