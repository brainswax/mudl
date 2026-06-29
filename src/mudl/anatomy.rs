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

/// Definition of a single anatomical slot from a body plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BodySlotDef {
    pub name: String,
    pub capacity: u32,
    pub slot_type: SlotType,
    pub hands: u32,
}

/// A complete body plan (e.g. human, quadruped) defined in MUDL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BodyPlan {
    pub name: String,
    pub slots: Vec<BodySlotDef>,
}

impl BodyPlan {
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

/// Player spawn template referencing a body plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerTemplate {
    pub name: String,
    pub body_plan: String,
    pub gender: String,
}

/// Loaded anatomy definitions from MUDL files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnatomyRegistry {
    pub body_plans: HashMap<String, BodyPlan>,
    pub player_templates: HashMap<String, PlayerTemplate>,
}

impl AnatomyRegistry {
    pub fn body_plan(&self, name: &str) -> Option<&BodyPlan> {
        self.body_plans.get(name)
    }

    pub fn player_template(&self, name: &str) -> Option<&PlayerTemplate> {
        self.player_templates.get(name)
    }

    pub fn default_template(&self) -> Option<&PlayerTemplate> {
        self.player_template("default")
    }

    pub fn merge(&mut self, other: AnatomyRegistry) {
        self.body_plans.extend(other.body_plans);
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

/// Parse anatomy definitions from MUDL source text.
pub fn parse_anatomy_file(content: &str) -> anyhow::Result<AnatomyRegistry> {
    let mut registry = AnatomyRegistry::default();
    let mut current_plan: Option<BodyPlan> = None;
    let mut current_template: Option<PlayerTemplate> = None;

    for raw_line in content.lines() {
        let line = strip_comment(raw_line);
        if line.is_empty() {
            continue;
        }

        if line == "@end" {
            if let Some(plan) = current_plan.take() {
                registry.body_plans.insert(plan.name.clone(), plan);
            }
            if let Some(template) = current_template.take() {
                registry
                    .player_templates
                    .insert(template.name.clone(), template);
            }
            continue;
        }

        if let Some(name) = line.strip_prefix("@body-plan ") {
            current_plan = Some(BodyPlan {
                name: name.trim().to_string(),
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

            if let Some(plan) = &mut current_plan {
                plan.slots.push(BodySlotDef {
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
                name: name.trim().to_string(),
                body_plan: "human".to_string(),
                gender: "neutral".to_string(),
            });
            current_plan = None;
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            if let Some(template) = &mut current_template {
                match key.trim().to_lowercase().as_str() {
                    "body_plan" => template.body_plan = value.trim().to_string(),
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
