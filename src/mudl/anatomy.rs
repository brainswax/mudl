use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// What a body slot is used for (loaded from MUDL `type=` attribute).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SlotType {
    Grasp,
    Wear,
    Limb,
    Pocket,
    Container,
}

impl SlotType {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "grasp" => Some(Self::Grasp),
            "wear" => Some(Self::Wear),
            "limb" => Some(Self::Limb),
            "pocket" => Some(Self::Pocket),
            "container" => Some(Self::Container),
            _ => None,
        }
    }
}

/// Definition of a single anatomical slot from a creature definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BodySlotDef {
    pub name: String,
    pub capacity: u32,
    pub slot_type: SlotType,
    pub hands: u32,
}

/// Anatomy slots for a creature (e.g. human, cat).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatureDef {
    pub name: String,
    pub slots: Vec<BodySlotDef>,
}

impl CreatureDef {
    pub fn slot(&self, name: &str) -> Option<&BodySlotDef> {
        self.slots.iter().find(|s| s.name == name)
    }

    pub fn slots_of_type(&self, slot_type: SlotType) -> Vec<&BodySlotDef> {
        self.slots
            .iter()
            .filter(|s| s.slot_type == slot_type)
            .collect()
    }

    pub fn grasp_slots(&self) -> Vec<&BodySlotDef> {
        self.slots_of_type(SlotType::Grasp)
    }

    pub fn wear_slots(&self) -> Vec<&BodySlotDef> {
        self.slots_of_type(SlotType::Wear)
    }
}

/// Backward-compatible alias for creature anatomy definitions.
pub type BodyPlan = CreatureDef;

/// Player spawn template referencing a creature definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerTemplate {
    pub name: String,
    pub creature: String,
    pub gender: String,
}

/// Loaded creature and player definitions from MUDL files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnatomyRegistry {
    pub creatures: HashMap<String, CreatureDef>,
    pub player_templates: HashMap<String, PlayerTemplate>,
}

impl AnatomyRegistry {
    pub fn creature(&self, name: &str) -> Option<&CreatureDef> {
        self.creatures.get(name)
    }

    /// Alias for [`creature`](Self::creature) — legacy name for anatomy lookups.
    pub fn body_plan(&self, name: &str) -> Option<&CreatureDef> {
        self.creature(name)
    }

    pub fn player_template(&self, name: &str) -> Option<&PlayerTemplate> {
        self.player_templates.get(name)
    }

    pub fn default_template(&self) -> Option<&PlayerTemplate> {
        self.player_template("default")
    }

    pub fn merge(&mut self, other: AnatomyRegistry) {
        self.creatures.extend(other.creatures);
        self.player_templates.extend(other.player_templates);
    }
}

fn strip_comment(line: &str) -> &str {
    line.split(';').next().unwrap_or(line).trim()
}

fn parse_key_value_pairs(s: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for part in s.split_whitespace() {
        if let Some((key, value)) = part.split_once('=') {
            map.insert(key.to_lowercase(), value.to_string());
        }
    }
    map
}

fn parse_creature_name(line: &str) -> Option<String> {
    let name = line
        .trim()
        .trim_end_matches('{')
        .trim()
        .to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Parse creature and player definitions from MUDL source text.
pub fn parse_anatomy_file(content: &str) -> anyhow::Result<AnatomyRegistry> {
    let mut registry = AnatomyRegistry::default();
    let mut current_creature: Option<CreatureDef> = None;
    let mut current_template: Option<PlayerTemplate> = None;

    for raw_line in content.lines() {
        let line = strip_comment(raw_line);
        if line.is_empty() {
            continue;
        }

        if line == "}" || line == "@end" {
            if let Some(creature) = current_creature.take() {
                registry.creatures.insert(creature.name.clone(), creature);
            }
            if let Some(template) = current_template.take() {
                registry
                    .player_templates
                    .insert(template.name.clone(), template);
            }
            continue;
        }

        if let Some(name) = line
            .strip_prefix("@creature ")
            .or_else(|| line.strip_prefix("@body-plan "))
        {
            current_creature = Some(CreatureDef {
                name: parse_creature_name(name).ok_or_else(|| {
                    anyhow::anyhow!("@creature missing name: {line}")
                })?,
                slots: Vec::new(),
            });
            current_template = None;
            continue;
        }

        if let Some(rest) = line.strip_prefix("@slot ") {
            let mut parts = rest.split_whitespace();
            let slot_name = parts
                .next()
                .ok_or_else(|| anyhow::anyhow!("@slot missing name: {line}"))?
                .to_string();
            let attrs = parse_key_value_pairs(&parts.collect::<Vec<_>>().join(" "));
            let capacity = attrs
                .get("capacity")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1);
            let slot_type = attrs
                .get("type")
                .and_then(|t| SlotType::parse(t))
                .unwrap_or(SlotType::Grasp);
            let hands = attrs.get("hands").and_then(|v| v.parse().ok()).unwrap_or(1);

            if let Some(creature) = &mut current_creature {
                creature.slots.push(BodySlotDef {
                    name: slot_name,
                    capacity,
                    slot_type,
                    hands,
                });
            }
            continue;
        }

        if let Some(name) = line.strip_prefix("@player-template ") {
            current_template = Some(PlayerTemplate {
                name: name.trim().trim_end_matches('{').trim().to_string(),
                creature: "human".to_string(),
                gender: "neutral".to_string(),
            });
            current_creature = None;
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            if let Some(template) = &mut current_template {
                match key.trim().to_lowercase().as_str() {
                    "creature" | "body_plan" => {
                        template.creature = value.trim().to_string();
                    }
                    "gender" => template.gender = value.trim().to_string(),
                    _ => {}
                }
            }
        }
    }

    Ok(registry)
}

/// Human-readable slot label (e.g. `left_hand` → "left hand").
pub fn slot_display_name(slot: &str) -> String {
    slot.replace('_', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_creature_and_player_template() {
        let content = include_str!("../../modules/default/worlds/default_world/creatures.mudl");
        let registry = parse_anatomy_file(content).unwrap();
        let human = registry.creature("human").unwrap();
        assert_eq!(human.slots.len(), 10);
        assert!(human.grasp_slots().len() >= 2);

        let players = include_str!("../../modules/default/worlds/default_world/players.mudl");
        let registry = parse_anatomy_file(players).unwrap();
        let template = registry.player_template("default").unwrap();
        assert_eq!(template.creature, "human");
    }
}