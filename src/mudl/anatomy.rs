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
    /// Optional ambient effect tag for this slot (injuries, curses, etc.).
    pub effect: Option<String>,
}

/// Anatomy slots and baseline vitals for a creature (e.g. human, cat).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatureDef {
    pub name: String,
    pub slots: Vec<BodySlotDef>,
    pub max_health: i64,
    pub base_max_weight: Option<i64>,
    pub stats: HashMap<String, i64>,
    pub skills: HashMap<String, i64>,
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

/// Temporary or persistent condition modifying creature capabilities.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectDef {
    pub name: String,
    pub mod_max_health: i64,
    pub mod_max_weight: i64,
    pub mod_encumbrance: f64,
    pub stat_mods: HashMap<String, i64>,
    pub skill_mods: HashMap<String, i64>,
    /// Health restored when the creature enters a new room (equipment-granted effects).
    pub regen_on_enter: i64,
    /// Condition category for cures and `when condition` checks (e.g. poison, bleed).
    pub condition_type: Option<String>,
    /// Tags matched by `cure-tag` scripts (defaults to `condition_type` when set).
    pub cure_tags: Vec<String>,
    /// Damage dealt each tick while active (`tick_on` event).
    pub damage_on_tick: i64,
    /// Health restored each tick while active (`tick_on` event).
    pub heal_on_tick: i64,
    /// Event that advances ticks and applies DOT/HoT (default `on_enter`).
    pub tick_on: String,
    /// Ticks until the condition expires (0 = until cured).
    pub duration_ticks: i64,
}

impl EffectDef {
    /// Whether this definition behaves as a timed/DOT condition (not just a passive buff).
    pub fn is_condition(&self) -> bool {
        self.condition_type.is_some()
            || self.damage_on_tick > 0
            || self.heal_on_tick > 0
            || self.duration_ticks > 0
    }

    /// Tags used by `cure-tag` — explicit `cure_tags` plus `condition_type` when present.
    pub fn all_cure_tags(&self) -> Vec<String> {
        let mut tags = self.cure_tags.clone();
        if let Some(kind) = &self.condition_type {
            if !tags.iter().any(|t| t == kind) {
                tags.push(kind.clone());
            }
        }
        tags
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
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnatomyRegistry {
    pub creatures: HashMap<String, CreatureDef>,
    pub player_templates: HashMap<String, PlayerTemplate>,
    pub effects: HashMap<String, EffectDef>,
}

impl AnatomyRegistry {
    pub fn creature(&self, name: &str) -> Option<&CreatureDef> {
        self.creatures.get(name)
    }

    /// Alias for [`creature`](Self::creature) — legacy name for anatomy lookups.
    pub fn body_plan(&self, name: &str) -> Option<&CreatureDef> {
        self.creature(name)
    }

    pub fn effect(&self, name: &str) -> Option<&EffectDef> {
        self.effects.get(name)
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
        self.effects.extend(other.effects);
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
    let name = line.trim().trim_end_matches('{').trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn parse_stat_line(rest: &str) -> Option<(String, i64)> {
    let rest = rest.trim();
    if let Some((name, value)) = rest.split_once('=') {
        return Some((name.trim().to_string(), value.trim().parse().ok()?));
    }
    let mut parts = rest.split_whitespace();
    let name = parts.next()?.to_string();
    let value = parts.next()?.parse().ok()?;
    Some((name, value))
}

fn default_creature(name: String) -> CreatureDef {
    CreatureDef {
        name,
        slots: Vec::new(),
        max_health: 100,
        base_max_weight: None,
        stats: HashMap::new(),
        skills: HashMap::new(),
    }
}

fn default_effect(name: String) -> EffectDef {
    EffectDef {
        name,
        mod_max_health: 0,
        mod_max_weight: 0,
        mod_encumbrance: 1.0,
        stat_mods: HashMap::new(),
        skill_mods: HashMap::new(),
        regen_on_enter: 0,
        condition_type: None,
        cure_tags: Vec::new(),
        damage_on_tick: 0,
        heal_on_tick: 0,
        tick_on: "on_enter".to_string(),
        duration_ticks: 0,
    }
}

/// Parse creature and player definitions from MUDL source text.
pub fn parse_anatomy_file(content: &str) -> anyhow::Result<AnatomyRegistry> {
    let mut registry = AnatomyRegistry::default();
    let mut current_creature: Option<CreatureDef> = None;
    let mut current_template: Option<PlayerTemplate> = None;
    let mut current_effect: Option<EffectDef> = None;

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
            if let Some(effect) = current_effect.take() {
                registry.effects.insert(effect.name.clone(), effect);
            }
            continue;
        }

        if let Some(name) = line
            .strip_prefix("@creature ")
            .or_else(|| line.strip_prefix("@body-plan "))
        {
            current_creature = Some(default_creature(
                parse_creature_name(name)
                    .ok_or_else(|| anyhow::anyhow!("@creature missing name: {line}"))?,
            ));
            current_template = None;
            current_effect = None;
            continue;
        }

        if let Some(name) = line
            .strip_prefix("@effect ")
            .or_else(|| line.strip_prefix("@condition "))
        {
            current_effect = Some(default_effect(
                parse_creature_name(name)
                    .ok_or_else(|| anyhow::anyhow!("@effect missing name: {line}"))?,
            ));
            current_creature = None;
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
            let effect = attrs.get("effect").cloned();

            if let Some(creature) = &mut current_creature {
                creature.slots.push(BodySlotDef {
                    name: slot_name,
                    capacity,
                    slot_type,
                    hands,
                    effect,
                });
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("@stat ") {
            if let Some((name, value)) = parse_stat_line(rest) {
                if let Some(creature) = &mut current_creature {
                    creature.stats.insert(name, value);
                }
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("@skill ") {
            if let Some((name, value)) = parse_stat_line(rest) {
                if let Some(creature) = &mut current_creature {
                    creature.skills.insert(name, value);
                }
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
            current_effect = None;
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_lowercase();
            let value = value.trim();
            if let Some(creature) = &mut current_creature {
                match key.as_str() {
                    "max_health" => creature.max_health = value.parse().unwrap_or(100),
                    "base_max_weight" => creature.base_max_weight = value.parse().ok(),
                    _ => {}
                }
            }
            if let Some(template) = &mut current_template {
                match key.as_str() {
                    "creature" | "body_plan" => template.creature = value.to_string(),
                    "gender" => template.gender = value.to_string(),
                    _ => {}
                }
            }
            if let Some(effect) = &mut current_effect {
                match key.as_str() {
                    "mod_max_health" | "mod_health" => {
                        effect.mod_max_health = value.parse().unwrap_or(0)
                    }
                    "mod_max_weight" => effect.mod_max_weight = value.parse().unwrap_or(0),
                    "mod_encumbrance" => effect.mod_encumbrance = value.parse().unwrap_or(1.0),
                    "regen_on_enter" | "regen" => {
                        effect.regen_on_enter = value.parse().unwrap_or(0)
                    }
                    key if key.starts_with("mod_stat_") => {
                        let stat = key.trim_start_matches("mod_stat_");
                        if let Ok(v) = value.parse::<i64>() {
                            effect.stat_mods.insert(stat.to_string(), v);
                        }
                    }
                    key if key.starts_with("mod_skill_") => {
                        let skill = key.trim_start_matches("mod_skill_");
                        if let Ok(v) = value.parse::<i64>() {
                            effect.skill_mods.insert(skill.to_string(), v);
                        }
                    }
                    "condition" | "condition_type" | "kind" => {
                        effect.condition_type = Some(value.to_string());
                    }
                    "cure_tag" | "cure_tags" | "cures" => {
                        effect.cure_tags = value
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                    "damage_on_tick" | "dot" | "damage_per_tick" => {
                        effect.damage_on_tick = value.parse().unwrap_or(0).max(0);
                    }
                    "heal_on_tick" | "hot" | "heal_per_tick" | "regen_per_tick" => {
                        effect.heal_on_tick = value.parse().unwrap_or(0).max(0);
                    }
                    "tick_on" | "tick_event" => {
                        effect.tick_on = value.to_string();
                    }
                    "duration" | "duration_ticks" | "ticks" => {
                        effect.duration_ticks = value.parse().unwrap_or(0).max(0);
                    }
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
        assert_eq!(human.max_health, 100);
        assert_eq!(human.stats.get("strength").copied(), Some(10));

        let players = include_str!("../../modules/default/worlds/default_world/players.mudl");
        let registry = parse_anatomy_file(players).unwrap();
        let template = registry.player_template("default").unwrap();
        assert_eq!(template.creature, "human");
    }

    #[test]
    fn parse_effect_and_slot_effect() {
        let content = r#"
@creature cat
  max_health=80
  @slot tail capacity=1 type=limb effect=sprained
  @stat agility 14
  @skill stalking 2
@end
@effect sprained
  mod_encumbrance=1.15
  mod_stat_dexterity=-3
@end
"#;
        let registry = parse_anatomy_file(content).unwrap();
        let cat = registry.creature("cat").unwrap();
        assert_eq!(cat.max_health, 80);
        assert_eq!(cat.stats.get("agility").copied(), Some(14));
        assert_eq!(cat.skills.get("stalking").copied(), Some(2));
        assert_eq!(cat.slots[0].effect.as_deref(), Some("sprained"));
        let effect = registry.effect("sprained").unwrap();
        assert!((effect.mod_encumbrance - 1.15).abs() < f64::EPSILON);
        assert_eq!(effect.stat_mods.get("dexterity").copied(), Some(-3));
    }

    #[test]
    fn parse_condition_effect() {
        let content = r#"
@condition swamp_venom
  condition=poison
  cure_tag=poison
  damage_on_tick=4
  duration=8
@end
@condition shore_regeneration
  condition=regeneration
  heal_on_tick=6
  duration=5
@end
"#;
        let registry = parse_anatomy_file(content).unwrap();
        let venom = registry.effect("swamp_venom").unwrap();
        assert_eq!(venom.condition_type.as_deref(), Some("poison"));
        assert_eq!(venom.damage_on_tick, 4);
        assert_eq!(venom.duration_ticks, 8);
        assert!(venom.is_condition());
        let regen = registry.effect("shore_regeneration").unwrap();
        assert_eq!(regen.heal_on_tick, 6);
        assert_eq!(regen.duration_ticks, 5);
    }
}
