//! Reusable creature behavior templates — composable AI personalities in MUDL.

/// How a creature reacts when a player enters its room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CreatureReact {
    #[default]
    Ignore,
    Warn,
    Attack,
    Flee,
    Wander,
}

impl CreatureReact {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "warn" | "guard" => Self::Warn,
            "attack" | "aggressive" => Self::Attack,
            "flee" | "coward" | "passive_flee" => Self::Flee,
            "wander" | "roam" => Self::Wander,
            "passive" | "ignore" | "calm" => Self::Ignore,
            _ => Self::Ignore,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ignore => "ignore",
            Self::Warn => "warn",
            Self::Attack => "attack",
            Self::Flee => "flee",
            Self::Wander => "wander",
        }
    }
}

/// A reusable behavior template defined in MUDL (`@behavior-template`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BehaviorTemplateDef {
    pub base_name: String,
    pub react: CreatureReact,
    /// Optional scripted line on player enter (`say`, `emote`, etc.).
    pub on_enter_action: Option<String>,
    pub on_enter_text: Option<String>,
    /// For `wander`: emote every N player entries in the room (default 3).
    pub wander_interval: u32,
    /// Default damage for `attack` react when no script overrides.
    pub attack_damage: i64,
}

impl Default for BehaviorTemplateDef {
    fn default() -> Self {
        Self {
            base_name: String::new(),
            react: CreatureReact::Ignore,
            on_enter_action: None,
            on_enter_text: None,
            wander_interval: 3,
            attack_damage: 8,
        }
    }
}

fn strip_comment(line: &str) -> &str {
    line.split(';').next().unwrap_or(line).trim()
}

fn parse_on_enter_script(value: &str) -> (Option<String>, Option<String>) {
    let value = value.trim();
    if value.is_empty() {
        return (None, None);
    }
    let mut parts = value.splitn(2, ' ');
    let action = parts.next().map(|s| s.to_string());
    let text = parts
        .next()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    (action, text)
}

/// Parse `@behavior-template` blocks from MUDL source.
pub fn parse_behavior_file(content: &str) -> Vec<BehaviorTemplateDef> {
    let mut templates = Vec::new();
    let mut current: Option<BehaviorTemplateDef> = None;

    for raw_line in content.lines() {
        let line = strip_comment(raw_line);
        if line.is_empty() {
            continue;
        }
        if line == "@end" {
            if let Some(template) = current.take() {
                templates.push(template);
            }
            continue;
        }
        if let Some(name) = line.strip_prefix("@behavior-template ") {
            if let Some(template) = current.take() {
                templates.push(template);
            }
            current = Some(BehaviorTemplateDef {
                base_name: name.trim().to_string(),
                ..BehaviorTemplateDef::default()
            });
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_lowercase();
            let value = value.trim();
            if let Some(template) = &mut current {
                match key.as_str() {
                    "react" => template.react = CreatureReact::parse(value),
                    "on_enter" => {
                        let (action, text) = parse_on_enter_script(value);
                        template.on_enter_action = action;
                        template.on_enter_text = text;
                    }
                    "wander_interval" | "interval" => {
                        template.wander_interval = value.parse().unwrap_or(3).max(1);
                    }
                    "attack_damage" | "damage" => {
                        template.attack_damage = value.parse().unwrap_or(8).max(0);
                    }
                    _ => {}
                }
            }
        }
    }

    if let Some(template) = current {
        templates.push(template);
    }

    templates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_behavior_templates() {
        let content = r#"
@behavior-template passive
  react=ignore
@end
@behavior-template aggressive
  react=attack
  on_enter=say You should not have come here.
  attack_damage=12
@end
@behavior-template guard
  react=warn
  on_enter=say Halt! Who goes there?
@end
@behavior-template skittish
  react=flee
  on_enter=emote scrambles away from you.
@end
"#;
        let templates = parse_behavior_file(content);
        assert_eq!(templates.len(), 4);
        let aggressive = templates
            .iter()
            .find(|t| t.base_name == "aggressive")
            .unwrap();
        assert_eq!(aggressive.react, CreatureReact::Attack);
        assert_eq!(aggressive.attack_damage, 12);
        assert_eq!(
            aggressive.on_enter_text.as_deref(),
            Some("You should not have come here.")
        );
    }
}
