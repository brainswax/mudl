//! Parse and execute MUDL `@trigger` script actions (narrate, react, teleport, spawn, …).

use std::collections::HashMap;

use crate::creature::behavior::{
    creature_attack_damage, npc_attack_player, npc_flee_room, DEFAULT_ATTACK_DAMAGE,
};
use crate::creature::conditions::{
    apply_condition, creature_has_condition_tag, creature_has_effect, cure_by_tag, remove_condition,
};
use crate::creature::spawner::spawn_creature_from_template;
use crate::creature::vitality::{apply_damage, creature_health, heal};
use crate::mudl::{AnatomyRegistry, CreatureReact};
use crate::object::{generate_object_id, Object, ObjectId, PermissionFlags, Property, Value};

use super::events::{EventContext, EventOutcome};

/// Who receives a targeted script action. Defaults to [`ScriptTarget::Actor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScriptTarget {
    #[default]
    Actor,
    Host,
    Target,
}

impl ScriptTarget {
    pub fn parse(word: &str) -> Option<Self> {
        match word.to_ascii_lowercase().as_str() {
            "actor" | "player" => Some(Self::Actor),
            "host" | "self" => Some(Self::Host),
            "target" | "victim" | "killer" => Some(Self::Target),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Below,
    Above,
    AtMost,
    AtLeast,
    Equals,
}

impl CompareOp {
    fn parse(words: &[&str]) -> Option<(Self, usize)> {
        if words.len() < 2 {
            return None;
        }
        let op = match (words[0], words[1]) {
            ("below", _) => Self::Below,
            ("above", _) => Self::Above,
            ("at", "most") if words.len() >= 3 => Self::AtMost,
            ("at", "least") if words.len() >= 3 => Self::AtLeast,
            ("equals", _) | ("is", _) => Self::Equals,
            _ => return None,
        };
        let consumed = match op {
            CompareOp::AtMost | CompareOp::AtLeast => 2,
            _ => 1,
        };
        Some((op, consumed))
    }

    fn compare(self, left: i64, right: i64) -> bool {
        match self {
            Self::Below => left < right,
            Self::Above => left > right,
            Self::AtMost => left <= right,
            Self::AtLeast => left >= right,
            Self::Equals => left == right,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    Bool(bool),
    Int(i64),
    String(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScriptCondition {
    Health {
        target: ScriptTarget,
        op: CompareOp,
        value: i64,
    },
    Stat {
        target: ScriptTarget,
        name: String,
        op: CompareOp,
        value: i64,
    },
    Skill {
        target: ScriptTarget,
        name: String,
        op: CompareOp,
        value: i64,
    },
    Property {
        target: ScriptTarget,
        name: String,
        value: Option<PropertyValue>,
    },
    Chance {
        percent: u32,
    },
    Effect {
        target: ScriptTarget,
        name: String,
    },
    ConditionTag {
        target: ScriptTarget,
        tag: String,
    },
    Not(Box<ScriptCondition>),
}

/// Parsed script verb + payload.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptAction {
    Narrate(String),
    Say(String),
    Emote(String),
    React(CreatureReact),
    Damage(i64, ScriptTarget),
    Heal(i64, ScriptTarget),
    ModStat(String, i64, ScriptTarget),
    ModSkill(String, i64, ScriptTarget),
    SetProperty(String, PropertyValue, ScriptTarget),
    GrantEffect(String, ScriptTarget),
    RemoveEffect(String, ScriptTarget),
    CureTag(String, ScriptTarget),
    Teleport(String, ScriptTarget),
    SpawnCreature(String),
    SpawnItem(String, u32),
    When(ScriptCondition, Box<ScriptAction>),
    Stop,
    Raw(String),
}

fn tokenize(rest: &str) -> Vec<String> {
    rest.split_whitespace().map(|s| s.to_string()).collect()
}

fn parse_target_prefix(tokens: &[String]) -> (ScriptTarget, &[String]) {
    if let Some(first) = tokens.first() {
        if let Some(target) = ScriptTarget::parse(first) {
            return (target, &tokens[1..]);
        }
    }
    (ScriptTarget::Actor, tokens)
}

fn parse_property_value(raw: &str) -> PropertyValue {
    match raw.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" => PropertyValue::Bool(true),
        "false" | "no" | "off" => PropertyValue::Bool(false),
        _ => raw.parse::<i64>().map(PropertyValue::Int).unwrap_or_else(|_| {
            PropertyValue::String(raw.trim_matches('"').to_string())
        }),
    }
}

fn parse_condition(rest: &str) -> Option<ScriptCondition> {
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }

    if tokens[0].eq_ignore_ascii_case("not") {
        return parse_condition(&tokens[1..].join(" "))
            .map(|inner| ScriptCondition::Not(Box::new(inner)));
    }

    let (target, start) = if ScriptTarget::parse(tokens[0]).is_some() {
        (ScriptTarget::parse(tokens[0]).unwrap(), 1)
    } else {
        (ScriptTarget::Actor, 0)
    };
    let words = &tokens[start..];

    if words.first() == Some(&"effect") {
        let name = words.get(1)?.to_string();
        return Some(ScriptCondition::Effect { target, name });
    }

    if words.first() == Some(&"condition") {
        let tag = words.get(1)?.to_string();
        return Some(ScriptCondition::ConditionTag { target, tag });
    }

    if words.first() == Some(&"chance") {
        let percent = words.get(1).and_then(|v| v.parse().ok()).unwrap_or(50);
        return Some(ScriptCondition::Chance {
            percent: percent.min(100),
        });
    }

    if words.first() == Some(&"property") {
        let name = words.get(1)?.to_string();
        if words.len() >= 4 && matches!(words[2], "is" | "equals") {
            let value = parse_property_value(words[3]);
            return Some(ScriptCondition::Property {
                target,
                name,
                value: Some(value),
            });
        }
        return Some(ScriptCondition::Property {
            target,
            name,
            value: None,
        });
    }

    if words.first() == Some(&"health") {
        let (op, skip) = CompareOp::parse(&words[1..])?;
        let value = words.get(1 + skip).and_then(|v| v.parse().ok())?;
        return Some(ScriptCondition::Health { target, op, value });
    }

    if words.first() == Some(&"stat") {
        let name = words.get(1)?.to_string();
        let (op, skip) = CompareOp::parse(&words[2..])?;
        let value = words.get(2 + skip).and_then(|v| v.parse().ok())?;
        return Some(ScriptCondition::Stat {
            target,
            name,
            op,
            value,
        });
    }

    if words.first() == Some(&"skill") {
        let name = words.get(1)?.to_string();
        let (op, skip) = CompareOp::parse(&words[2..])?;
        let value = words.get(2 + skip).and_then(|v| v.parse().ok())?;
        return Some(ScriptCondition::Skill {
            target,
            name,
            op,
            value,
        });
    }

    None
}

fn parse_amount_and_target(tokens: &[String], default: i64) -> (i64, ScriptTarget) {
    let (target, rest) = parse_target_prefix(tokens);
    if let Some(raw) = rest.first() {
        if let Ok(amount) = raw.parse::<i64>() {
            return (amount, target);
        }
    }
    (default, ScriptTarget::Actor)
}

/// Split `verb rest…` into a script action.
pub fn parse_script(code: &str) -> ScriptAction {
    let code = code.trim();
    if code.is_empty() {
        return ScriptAction::Raw(String::new());
    }

    if let Some(rest) = code
        .strip_prefix("when ")
        .or_else(|| code.strip_prefix("if "))
    {
        if let Some(then_idx) = rest.find(" then ") {
            let cond = &rest[..then_idx];
            let action = &rest[then_idx + 6..];
            if let Some(condition) = parse_condition(cond) {
                return ScriptAction::When(condition, Box::new(parse_script(action)));
            }
        }
    }

    let Some((verb, rest)) = code.split_once(char::is_whitespace) else {
        return match code.to_ascii_lowercase().as_str() {
            "attack" => ScriptAction::React(CreatureReact::Attack),
            "flee" => ScriptAction::React(CreatureReact::Flee),
            "greet" => ScriptAction::React(CreatureReact::Greet),
            "warn" => ScriptAction::React(CreatureReact::Warn),
            "stop" | "cancel" | "halt" => ScriptAction::Stop,
            _ => ScriptAction::Raw(code.to_string()),
        };
    };
    let verb = verb.to_ascii_lowercase();
    let rest = rest.trim();
    let tokens = tokenize(rest);

    match verb.as_str() {
        "narrate" | "message" => ScriptAction::Narrate(rest.to_string()),
        "say" => ScriptAction::Say(rest.to_string()),
        "emote" => ScriptAction::Emote(rest.to_string()),
        "react" if !rest.is_empty() => ScriptAction::React(CreatureReact::parse(rest)),
        "attack" | "flee" | "greet" | "warn" if rest.is_empty() => {
            ScriptAction::React(CreatureReact::parse(&verb))
        }
        "damage" => {
            let (amount, target) = parse_amount_and_target(&tokens, 8);
            ScriptAction::Damage(amount, target)
        }
        "heal" => {
            let (amount, target) = parse_amount_and_target(&tokens, 5);
            ScriptAction::Heal(amount, target)
        }
        "mod-stat" | "mod_stat" => {
            let (target, rest_tokens) = parse_target_prefix(&tokens);
            let stat = rest_tokens.first().cloned().unwrap_or_else(|| "strength".to_string());
            let delta = rest_tokens
                .get(1)
                .and_then(|v| v.parse().ok())
                .unwrap_or(1);
            ScriptAction::ModStat(stat, delta, target)
        }
        "mod-skill" | "mod_skill" => {
            let (target, rest_tokens) = parse_target_prefix(&tokens);
            let skill = rest_tokens.first().cloned().unwrap_or_else(|| "survival".to_string());
            let delta = rest_tokens
                .get(1)
                .and_then(|v| v.parse().ok())
                .unwrap_or(1);
            ScriptAction::ModSkill(skill, delta, target)
        }
        "set-property" | "set_property" | "set" => {
            let (target, rest_tokens) = parse_target_prefix(&tokens);
            let name = rest_tokens.first().cloned().unwrap_or_default();
            let value = rest_tokens
                .get(1)
                .map(|v| parse_property_value(v))
                .unwrap_or(PropertyValue::Bool(true));
            ScriptAction::SetProperty(name, value, target)
        }
        "grant-effect" | "grant_effect" | "effect" => {
            let (target, rest_tokens) = parse_target_prefix(&tokens);
            let effect = rest_tokens.first().cloned().unwrap_or_default();
            ScriptAction::GrantEffect(effect, target)
        }
        "remove-effect" | "remove_effect" | "cure-effect" | "cure_effect" => {
            let (target, rest_tokens) = parse_target_prefix(&tokens);
            let effect = rest_tokens.first().cloned().unwrap_or_default();
            ScriptAction::RemoveEffect(effect, target)
        }
        "cure-tag" | "cure_tag" | "cure" => {
            let (target, rest_tokens) = parse_target_prefix(&tokens);
            let tag = rest_tokens.first().cloned().unwrap_or_default();
            ScriptAction::CureTag(tag, target)
        }
        "teleport" | "send" => {
            let (target, rest_tokens) = parse_target_prefix(&tokens);
            let place = rest_tokens.join(" ");
            ScriptAction::Teleport(place, target)
        }
        "spawn" => {
            let (_, rest_tokens) = parse_target_prefix(&tokens);
            if rest_tokens.first().is_some_and(|t| t.eq_ignore_ascii_case("item")) {
                let proto = rest_tokens.get(1).cloned().unwrap_or_default();
                let count = rest_tokens
                    .get(2)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1)
                    .max(1);
                ScriptAction::SpawnItem(proto, count)
            } else if rest_tokens.first().is_some_and(|t| t.eq_ignore_ascii_case("creature")) {
                ScriptAction::SpawnCreature(
                    rest_tokens.get(1).cloned().unwrap_or_default(),
                )
            } else {
                ScriptAction::SpawnCreature(rest.to_string())
            }
        }
        "stop" | "cancel" | "halt" => ScriptAction::Stop,
        _ if !rest.is_empty() => ScriptAction::Raw(code.to_string()),
        _ => ScriptAction::Raw(verb),
    }
}

/// Format a narrative-only script (no side effects) for read-only gate handlers.
pub fn format_script_line(host: &Object, action: &ScriptAction) -> Option<String> {
    let display = host.name.to_lowercase();
    match action {
        ScriptAction::Narrate(text) => Some(text.clone()),
        ScriptAction::Say(text) if host.has_creature_role() => {
            Some(format!("{} says, \"{text}\"", host.name))
        }
        ScriptAction::Say(text) => Some(text.clone()),
        ScriptAction::Emote(text) if host.is_location() => Some(text.clone()),
        ScriptAction::Emote(text) => Some(format!("The {display} {text}")),
        ScriptAction::When(_, inner) => format_script_line(host, inner),
        ScriptAction::Raw(text) if !text.is_empty() => Some(text.clone()),
        _ => None,
    }
}

fn resolve_target_id(
    target: ScriptTarget,
    ctx: &EventContext,
    host_id: &ObjectId,
) -> Option<ObjectId> {
    match target {
        ScriptTarget::Actor => Some(ctx.actor_id.clone()),
        ScriptTarget::Host => Some(host_id.clone()),
        ScriptTarget::Target => ctx.target_id.clone(),
    }
}

fn mix_seed(parts: &[&str]) -> u64 {
    let mut hash = 0u64;
    for part in parts {
        for byte in part.as_bytes() {
            hash = hash.wrapping_mul(31).wrapping_add(u64::from(*byte));
        }
        hash = hash.wrapping_mul(31).wrapping_add(255);
    }
    hash
}

fn evaluate_condition(
    condition: &ScriptCondition,
    ctx: &EventContext,
    host_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
    anatomy: Option<&AnatomyRegistry>,
) -> bool {
    match condition {
        ScriptCondition::Not(inner) => {
            !evaluate_condition(inner, ctx, host_id, objects, anatomy)
        }
        ScriptCondition::Effect { target, name } => {
            let Some(id) = resolve_target_id(*target, ctx, host_id) else {
                return false;
            };
            let Some(obj) = objects.get(&id) else {
                return false;
            };
            let Some(anatomy) = anatomy else {
                return false;
            };
            creature_has_effect(obj, name, objects, anatomy)
        }
        ScriptCondition::ConditionTag { target, tag } => {
            let Some(id) = resolve_target_id(*target, ctx, host_id) else {
                return false;
            };
            let Some(obj) = objects.get(&id) else {
                return false;
            };
            let Some(anatomy) = anatomy else {
                return false;
            };
            creature_has_condition_tag(obj, tag, anatomy)
        }
        ScriptCondition::Chance { percent } => {
            let seed = mix_seed(&[
                ctx.actor_id.as_str(),
                host_id.as_str(),
                ctx.room_id
                    .as_ref()
                    .map(|id| id.as_str())
                    .unwrap_or(""),
                &percent.to_string(),
                "chance",
            ]);
            seed % 100 < u64::from(*percent)
        }
        ScriptCondition::Health { target, op, value } => {
            let Some(id) = resolve_target_id(*target, ctx, host_id) else {
                return false;
            };
            let Some(obj) = objects.get(&id) else {
                return false;
            };
            op.compare(creature_health(obj), *value)
        }
        ScriptCondition::Stat { target, name, op, value } => {
            let Some(id) = resolve_target_id(*target, ctx, host_id) else {
                return false;
            };
            let Some(obj) = objects.get(&id) else {
                return false;
            };
            let current = obj.get_int_map("stats").get(name).copied().unwrap_or(0);
            op.compare(current, *value)
        }
        ScriptCondition::Skill { target, name, op, value } => {
            let Some(id) = resolve_target_id(*target, ctx, host_id) else {
                return false;
            };
            let Some(obj) = objects.get(&id) else {
                return false;
            };
            let current = obj.get_int_map("skills").get(name).copied().unwrap_or(0);
            op.compare(current, *value)
        }
        ScriptCondition::Property { target, name, value } => {
            let Some(id) = resolve_target_id(*target, ctx, host_id) else {
                return false;
            };
            let Some(obj) = objects.get(&id) else {
                return false;
            };
            match value {
                None => obj.get_bool_property(name).unwrap_or(false),
                Some(PropertyValue::Bool(b)) => {
                    obj.get_bool_property(name).unwrap_or(false) == *b
                }
                Some(PropertyValue::Int(n)) => obj.get_int_property(name).unwrap_or(0) == *n,
                Some(PropertyValue::String(s)) => obj
                    .get_property(name)
                    .and_then(|p| {
                        if let Value::String(v) = &p.value {
                            Some(v == s)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(false),
            }
        }
    }
}

fn apply_property_value(obj: &mut Object, name: &str, value: &PropertyValue) {
    match value {
        PropertyValue::Bool(b) => obj.set_property_bool(name, *b),
        PropertyValue::Int(n) => obj.set_property_int(name, *n),
        PropertyValue::String(s) => obj.set_property_string(name, s),
    }
}

fn find_item_prototype<'a>(
    prototype_base: &str,
    objects: &'a HashMap<ObjectId, Object>,
) -> Option<&'a Object> {
    let expected = ObjectId::new(format!("item:{prototype_base}-001"));
    if let Some(obj) = objects.get(&expected) {
        if obj.is_active() {
            return Some(obj);
        }
    }
    objects.values().find(|obj| {
        obj.is_active()
            && obj.object_type() == "item"
            && obj.prototype.is_none()
            && obj.id.as_str().starts_with("item:")
            && obj.id.as_str().contains(prototype_base)
    })
}

fn spawn_item_from_prototype(
    prototype_base: &str,
    count: u32,
    room_id: &ObjectId,
    owner: &ObjectId,
    host_id: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
) -> Option<ObjectId> {
    let proto = find_item_prototype(prototype_base, objects)?;
    let spawn_index = objects
        .values()
        .filter(|o| {
            o.is_active()
                && o.get_object_ref_property("script_spawned_by").as_ref() == Some(host_id)
        })
        .count() as u32
        + 1;
    let id = generate_object_id("item", &format!("script-{prototype_base}"), spawn_index.max(1));
    let mut item = Object {
        id: id.clone(),
        name: proto.name.clone(),
        aliases: proto.aliases.clone(),
        location: Some(room_id.clone()),
        prototype: Some(proto.id.clone()),
        owner: owner.clone(),
        permissions: proto.permissions,
        properties: proto.properties.clone(),
        verbs: proto.verbs.clone(),
        event_handlers: proto.event_handlers.clone(),
        is_deleted: false,
        deleted_at: None,
    };
    if count > 1 {
        item.set_property_int("stack_count", i64::from(count));
    }
    item.add_property(Property {
        name: "script_spawned_by".to_string(),
        value: Value::ObjectRef(host_id.clone()),
        permissions: PermissionFlags::EVERYONE,
        behavior: None,
    });
    objects.insert(id.clone(), item);
    Some(id)
}

fn damage_label(target: &Object, script_target: ScriptTarget) -> String {
    match script_target {
        ScriptTarget::Actor => "You".to_string(),
        ScriptTarget::Host => target.name.clone(),
        ScriptTarget::Target => target.name.clone(),
    }
}

/// Execute one script against the event context, mutating `objects` as needed.
pub fn execute_script(
    host: &Object,
    action: &ScriptAction,
    ctx: &EventContext,
    objects: &mut HashMap<ObjectId, Object>,
    anatomy: Option<&AnatomyRegistry>,
) -> EventOutcome {
    let mut outcome = EventOutcome::default();
    let host_id = host.id.clone();
    let room_id = ctx
        .room_id
        .clone()
        .or_else(|| host.location.clone());

    match action {
        ScriptAction::When(condition, inner) => {
            if evaluate_condition(condition, ctx, &host_id, objects, anatomy) {
                return execute_script(host, inner, ctx, objects, anatomy);
            }
        }
        ScriptAction::Narrate(text) => {
            if !text.is_empty() {
                outcome.push_line(text.clone());
            }
        }
        ScriptAction::Say(text) => {
            if !text.is_empty() {
                let line = if host.has_creature_role() {
                    format!("{} says, \"{text}\"", host.name)
                } else {
                    text.clone()
                };
                outcome.push_line(line);
            }
        }
        ScriptAction::Emote(_) => {
            if let Some(line) = format_script_line(host, action) {
                outcome.push_line(line);
            }
        }
        ScriptAction::React(react) => {
            execute_react(
                *react,
                &host_id,
                &ctx.actor_id,
                room_id.as_ref(),
                objects,
                anatomy,
                &mut outcome,
            );
        }
        ScriptAction::Damage(amount, target) => {
            let Some(id) = resolve_target_id(*target, ctx, &host_id) else {
                outcome.record_error("damage: no target in context".to_string());
                return outcome;
            };
            let label = objects
                .get(&id)
                .map(|o| damage_label(o, *target))
                .unwrap_or_else(|| "Something".to_string());
            if let Some(victim) = objects.get_mut(&id) {
                if victim.has_creature_role() {
                    let after = apply_damage(victim, *amount);
                    let line = if *target == ScriptTarget::Actor {
                        format!("You take {amount} damage ({after} health remaining).")
                    } else {
                        format!("{label} takes {amount} damage ({after} health remaining).")
                    };
                    outcome.push_line(line);
                    outcome.mark_dirty(&id);
                } else {
                    outcome.record_error(format!("damage: {label} is not a creature"));
                }
            }
        }
        ScriptAction::Heal(amount, target) => {
            let Some(id) = resolve_target_id(*target, ctx, &host_id) else {
                outcome.record_error("heal: no target in context".to_string());
                return outcome;
            };
            let label = objects
                .get(&id)
                .map(|o| damage_label(o, *target))
                .unwrap_or_else(|| "Something".to_string());
            if let Some(subject) = objects.get_mut(&id) {
                if subject.has_creature_role() {
                    let after = heal(subject, *amount, anatomy);
                    let line = if *target == ScriptTarget::Actor {
                        format!("You recover {amount} health ({after} remaining).")
                    } else {
                        format!("{label} recovers {amount} health ({after} remaining).")
                    };
                    outcome.push_line(line);
                    outcome.mark_dirty(&id);
                } else {
                    outcome.record_error(format!("heal: {label} is not a creature"));
                }
            }
        }
        ScriptAction::ModStat(stat, delta, target) => {
            let Some(id) = resolve_target_id(*target, ctx, &host_id) else {
                return outcome;
            };
            if let Some(subject) = objects.get_mut(&id) {
                let mut stats = subject.get_int_map("stats");
                let current = stats.get(stat).copied().unwrap_or(0);
                stats.insert(stat.clone(), current + delta);
                subject.set_int_map("stats", stats);
                outcome.mark_dirty(&id);
            }
        }
        ScriptAction::ModSkill(skill, delta, target) => {
            let Some(id) = resolve_target_id(*target, ctx, &host_id) else {
                return outcome;
            };
            if let Some(subject) = objects.get_mut(&id) {
                let mut skills = subject.get_int_map("skills");
                let current = skills.get(skill).copied().unwrap_or(0);
                skills.insert(skill.clone(), current + delta);
                subject.set_int_map("skills", skills);
                outcome.mark_dirty(&id);
            }
        }
        ScriptAction::SetProperty(name, value, target) => {
            if name.is_empty() {
                outcome.record_error("set-property: missing property name".to_string());
                return outcome;
            }
            let Some(id) = resolve_target_id(*target, ctx, &host_id) else {
                outcome.record_error("set-property: no target in context".to_string());
                return outcome;
            };
            if let Some(subject) = objects.get_mut(&id) {
                apply_property_value(subject, name, value);
                outcome.mark_dirty(&id);
            }
        }
        ScriptAction::GrantEffect(effect, target) => {
            if effect.is_empty() {
                outcome.record_error("grant-effect: missing effect name".to_string());
                return outcome;
            }
            let Some(id) = resolve_target_id(*target, ctx, &host_id) else {
                outcome.record_error("grant-effect: no target in context".to_string());
                return outcome;
            };
            let Some(anatomy) = anatomy else {
                outcome.record_error(format!(
                    "grant-effect '{effect}': anatomy registry required"
                ));
                return outcome;
            };
            if anatomy.effect(effect).is_none() {
                outcome.record_error(format!("grant-effect: unknown effect '{effect}'"));
                return outcome;
            }
            if let Some(subject) = objects.get_mut(&id) {
                if subject.has_creature_role() {
                    apply_condition(subject, effect, anatomy);
                    outcome.mark_dirty(&id);
                    if *target == ScriptTarget::Actor {
                        outcome.push_line(format!("You feel the {effect} effect take hold."));
                    }
                } else {
                    outcome.record_error("grant-effect: target is not a creature".to_string());
                }
            }
        }
        ScriptAction::RemoveEffect(effect, target) => {
            if effect.is_empty() {
                outcome.record_error("remove-effect: missing effect name".to_string());
                return outcome;
            }
            let Some(id) = resolve_target_id(*target, ctx, &host_id) else {
                return outcome;
            };
            let Some(anatomy) = anatomy else {
                return outcome;
            };
            if let Some(subject) = objects.get_mut(&id) {
                if subject.has_creature_role() {
                    remove_condition(subject, effect, anatomy);
                    outcome.mark_dirty(&id);
                    if *target == ScriptTarget::Actor {
                        outcome.push_line(format!("The {effect} effect lifts."));
                    }
                }
            }
        }
        ScriptAction::CureTag(tag, target) => {
            if tag.is_empty() {
                outcome.record_error("cure-tag: missing tag".to_string());
                return outcome;
            }
            let Some(id) = resolve_target_id(*target, ctx, &host_id) else {
                return outcome;
            };
            let Some(anatomy) = anatomy else {
                return outcome;
            };
            if let Some(subject) = objects.get_mut(&id) {
                if subject.has_creature_role() {
                    let removed = cure_by_tag(subject, tag, anatomy);
                    if !removed.is_empty() {
                        outcome.mark_dirty(&id);
                        if *target == ScriptTarget::Actor {
                            outcome.push_line(format!(
                                "You feel the {tag} condition ease."
                            ));
                        }
                    }
                }
            }
        }
        ScriptAction::Teleport(place_base, target) => {
            let Some(dest_id) = resolve_place_id(place_base, objects) else {
                outcome.record_error(format!("teleport: unknown place '{place_base}'"));
                return outcome;
            };
            let Some(id) = resolve_target_id(*target, ctx, &host_id) else {
                outcome.record_error("teleport: no target in context".to_string());
                return outcome;
            };
            let dest_name = objects
                .get(&dest_id)
                .map(|p| p.name.to_lowercase())
                .unwrap_or_else(|| place_base.clone());
            let subject_name = objects.get(&id).map(|o| o.name.clone());
            if let Some(subject) = objects.get_mut(&id) {
                subject.location = Some(dest_id.clone());
                let line = if *target == ScriptTarget::Actor {
                    format!("The world lurches — you find yourself at {dest_name}.")
                } else {
                    let name = subject_name.unwrap_or_else(|| "Something".to_string());
                    format!("{name} vanishes toward {dest_name}.")
                };
                outcome.push_line(line);
                outcome.mark_dirty(&id);
            }
        }
        ScriptAction::SpawnCreature(template_name) => {
            let Some(room_id) = room_id.clone() else {
                outcome.record_error(format!(
                    "spawn '{template_name}': no room context for host {}",
                    host.name
                ));
                return outcome;
            };
            let owner = objects
                .get(&ctx.actor_id)
                .map(|o| o.owner.clone())
                .unwrap_or_else(|| ctx.actor_id.clone());
            let Some(anatomy) = anatomy else {
                outcome.record_error(format!(
                    "spawn '{template_name}': anatomy registry required"
                ));
                return outcome;
            };
            if let Some((npc, message)) = spawn_creature_from_template(
                template_name,
                &room_id,
                &owner,
                anatomy,
                objects,
            ) {
                outcome.mark_dirty(&npc.id);
                if let Some(msg) = message {
                    outcome.push_line(msg);
                }
            } else {
                outcome.record_error(format!(
                    "spawn '{template_name}': template not found in world"
                ));
            }
        }
        ScriptAction::SpawnItem(prototype_base, count) => {
            let Some(room_id) = room_id.clone() else {
                outcome.record_error(format!(
                    "spawn item '{prototype_base}': no room context"
                ));
                return outcome;
            };
            let owner = objects
                .get(&ctx.actor_id)
                .map(|o| o.owner.clone())
                .unwrap_or_else(|| ctx.actor_id.clone());
            if let Some(item_id) = spawn_item_from_prototype(
                prototype_base,
                *count,
                &room_id,
                &owner,
                &host_id,
                objects,
            ) {
                let label = objects
                    .get(&item_id)
                    .map(|i| i.name.to_lowercase())
                    .unwrap_or_else(|| prototype_base.clone());
                outcome.push_line(format!("You notice {label} here."));
                outcome.mark_dirty(&item_id);
            } else {
                outcome.record_error(format!(
                    "spawn item '{prototype_base}': prototype not found"
                ));
            }
        }
        ScriptAction::Stop => {
            outcome.cancel();
        }
        ScriptAction::Raw(text) if !text.is_empty() => {
            outcome.record_error(format!("unrecognized script: {text}"));
        }
        ScriptAction::Raw(_) => {}
    }

    outcome
}

fn execute_react(
    react: CreatureReact,
    host_id: &ObjectId,
    actor_id: &ObjectId,
    room_id: Option<&ObjectId>,
    objects: &mut HashMap<ObjectId, Object>,
    anatomy: Option<&AnatomyRegistry>,
    outcome: &mut EventOutcome,
) {
    let host_name = objects
        .get(host_id)
        .map(|h| h.name.clone())
        .unwrap_or_else(|| "Something".to_string());
    let room_id = match room_id {
        Some(id) => id.clone(),
        None => return,
    };

    match react {
        CreatureReact::Flee => {
            outcome.push_line(format!("{host_name} bolts away as you spot it."));
            if npc_flee_room(host_id, &room_id, objects) {
                outcome.mark_dirty(host_id);
            }
        }
        CreatureReact::Attack => {
            let damage = objects
                .get(host_id)
                .map(creature_attack_damage)
                .unwrap_or(DEFAULT_ATTACK_DAMAGE);
            if let Some(lines) =
                npc_attack_player(host_id, actor_id, &room_id, objects, anatomy, damage, false)
            {
                for line in lines {
                    outcome.push_line(line);
                }
                outcome.mark_dirty(host_id);
                outcome.mark_dirty(actor_id);
            }
        }
        CreatureReact::Warn => {
            outcome.push_line(format!("{host_name} eyes you warily."));
        }
        CreatureReact::Greet => {
            outcome.push_line(format!("{host_name} greets you."));
        }
        CreatureReact::Wander | CreatureReact::Ignore => {}
    }
}

/// Resolve a place `base_name` to a live location object id.
pub fn resolve_place_id(base_name: &str, objects: &HashMap<ObjectId, Object>) -> Option<ObjectId> {
    let base = base_name.trim();
    if base.is_empty() {
        return None;
    }
    for prefix in ["area", "room", "location", "region", "zone"] {
        let id = ObjectId::new(format!("{prefix}:{base}-001"));
        if objects
            .get(&id)
            .is_some_and(|o| o.is_active() && o.is_location())
        {
            return Some(id);
        }
    }
    objects
        .values()
        .find(|o| {
            o.is_active()
                && o.is_location()
                && o.id.as_str().contains(&format!(":{base}-"))
        })
        .map(|o| o.id.clone())
}

/// Run all event-handler scripts on `host` for `event_name`.
pub fn execute_host_event(
    event_name: &str,
    ctx: &EventContext,
    objects: &mut HashMap<ObjectId, Object>,
    anatomy: Option<&AnatomyRegistry>,
) -> EventOutcome {
    let handlers: Vec<String> = objects
        .get(&ctx.host_id)
        .and_then(|host| host.event_handlers.get(event_name))
        .map(|handlers| handlers.iter().map(|b| b.code.clone()).collect())
        .unwrap_or_default();

    if handlers.is_empty() {
        return EventOutcome::default();
    }

    let host = objects.get(&ctx.host_id).cloned();
    let Some(host) = host else {
        return EventOutcome::default();
    };

    let mut outcome = EventOutcome::default();
    for code in handlers {
        if outcome.is_cancelled() {
            break;
        }
        let action = parse_script(&code);
        let script_outcome = execute_script(&host, &action, ctx, objects, anatomy);
        outcome.append(script_outcome);
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::PermissionFlags;

    fn player(id: &str, room: &ObjectId) -> Object {
        let player_id = ObjectId::new(id);
        let mut player = Object {
            id: player_id.clone(),
            name: "Hero".to_string(),
            aliases: Vec::new(),
            location: Some(room.clone()),
            prototype: None,
            owner: player_id.clone(),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        player.init_creature_role(&crate::mudl::PlayerTemplate {
            name: "hero".to_string(),
            creature: "human".to_string(),
            gender: "neutral".to_string(),
        });
        player
    }

    #[test]
    fn unrecognized_script_records_error() {
        let player_id = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("area:test-001");
        let host = player("player:hero-001", &room_id);
        let mut objects = HashMap::from([(player_id.clone(), host.clone())]);
        let outcome = execute_script(
            &host,
            &ScriptAction::Raw("frobnicate the moon".to_string()),
            &EventContext {
                actor_id: player_id,
                host_id: ObjectId::new("player:hero-001"),
                room_id: Some(room_id),
                target_id: None,
            },
            &mut objects,
            None,
        );
        assert_eq!(outcome.errors.len(), 1);
        assert!(outcome.errors[0].contains("unrecognized script"));
    }

    #[test]
    fn parse_react_and_mod_actions() {
        assert_eq!(
            parse_script("react flee"),
            ScriptAction::React(CreatureReact::Flee)
        );
        assert_eq!(
            parse_script("damage 12"),
            ScriptAction::Damage(12, ScriptTarget::Actor)
        );
        assert_eq!(
            parse_script("damage host 12"),
            ScriptAction::Damage(12, ScriptTarget::Host)
        );
        assert_eq!(
            parse_script("mod-stat strength 2"),
            ScriptAction::ModStat("strength".to_string(), 2, ScriptTarget::Actor)
        );
        assert_eq!(
            parse_script("teleport haunted-entry"),
            ScriptAction::Teleport("haunted-entry".to_string(), ScriptTarget::Actor)
        );
        assert_eq!(
            parse_script("spawn item trail-rations 2"),
            ScriptAction::SpawnItem("trail-rations".to_string(), 2)
        );
    }

    #[test]
    fn parse_when_condition() {
        let action = parse_script("when health below 30 then heal 15");
        assert!(matches!(
            action,
            ScriptAction::When(
                ScriptCondition::Health {
                    target: ScriptTarget::Actor,
                    op: CompareOp::Below,
                    value: 30,
                },
                _
            )
        ));
    }

    #[test]
    fn teleport_moves_actor_to_place() {
        let player_id = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("area:forest-path-001");
        let dest_id = ObjectId::new("area:haunted-entry-001");
        let ply = player("player:hero-001", &room_id);
        let dest = Object {
            id: dest_id.clone(),
            name: "Tangled Threshold".to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: player_id.clone(),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        let mut objects = HashMap::from([
            (player_id.clone(), ply.clone()),
            (dest_id.clone(), dest),
        ]);

        let outcome = execute_script(
            &ply,
            &ScriptAction::Teleport("haunted-entry".to_string(), ScriptTarget::Actor),
            &EventContext {
                actor_id: player_id.clone(),
                host_id: player_id.clone(),
                room_id: Some(room_id.clone()),
                target_id: None,
            },
            &mut objects,
            None,
        );
        assert!(outcome.lines[0].contains("tangled threshold"));
        assert_eq!(
            objects.get(&player_id).unwrap().location.as_ref(),
            Some(&dest_id)
        );
    }

    #[test]
    fn when_low_health_heals_actor() {
        let player_id = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("area:heart-001");
        let mut ply = player("player:hero-001", &room_id);
        ply.set_property_int("health", 20);
        ply.set_property_int("max_health", 100);
        let mut objects = HashMap::from([(player_id.clone(), ply.clone())]);

        let action = parse_script("when health below 30 then heal 15");
        let outcome = execute_script(
            &ply,
            &action,
            &EventContext {
                actor_id: player_id.clone(),
                host_id: room_id.clone(),
                room_id: Some(room_id.clone()),
                target_id: None,
            },
            &mut objects,
            None,
        );
        assert!(outcome.lines.iter().any(|l| l.contains("recover 15 health")));
        assert_eq!(creature_health(objects.get(&player_id).unwrap()), 35);
    }

    #[test]
    fn when_high_health_skips_heal() {
        let player_id = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("area:heart-001");
        let mut ply = player("player:hero-001", &room_id);
        ply.set_property_int("health", 80);
        let mut objects = HashMap::from([(player_id.clone(), ply.clone())]);

        let action = parse_script("when health below 30 then heal 15");
        let outcome = execute_script(
            &ply,
            &action,
            &EventContext {
                actor_id: player_id.clone(),
                host_id: room_id.clone(),
                room_id: Some(room_id),
                target_id: None,
            },
            &mut objects,
            None,
        );
        assert!(outcome.lines.is_empty());
        assert_eq!(creature_health(objects.get(&player_id).unwrap()), 80);
    }

    #[test]
    fn react_attack_uses_creature_attack_damage() {
        use crate::creature::behavior::{creature_behaviors_to_property, CreatureBehaviorEntry};
        use crate::mudl::CreatureReact;

        let player_id = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("area:haunted-moon-001");
        let npc_id = ObjectId::new("npc:lurker-001");

        let mut ply = player("player:hero-001", &room_id);
        ply.set_property_int("health", 100);
        ply.set_property_int("max_health", 100);

        let mut lurker = Object {
            id: npc_id.clone(),
            name: "Pale Lurker".to_string(),
            aliases: Vec::new(),
            location: Some(room_id.clone()),
            prototype: None,
            owner: player_id.clone(),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        lurker.init_creature_role(&crate::mudl::PlayerTemplate {
            name: "lurker".to_string(),
            creature: "human".to_string(),
            gender: "neutral".to_string(),
        });
        lurker.add_property(creature_behaviors_to_property(&[CreatureBehaviorEntry {
            entry_type: "template".to_string(),
            template_name: Some("aggressive".to_string()),
            react: Some(CreatureReact::Attack),
            event: Some("on_discovered".to_string()),
            action: None,
            text: None,
            wander_interval: None,
            attack_damage: Some(15),
            awareness_check: None,
            perception: None,
            grant_effect_on_hit: None,
        }]));

        let mut objects = HashMap::from([
            (player_id.clone(), ply),
            (npc_id.clone(), lurker.clone()),
        ]);

        let outcome = execute_script(
            &lurker,
            &ScriptAction::React(CreatureReact::Attack),
            &EventContext {
                actor_id: player_id.clone(),
                host_id: npc_id.clone(),
                room_id: Some(room_id.clone()),
                target_id: None,
            },
            &mut objects,
            None,
        );
        assert!(outcome.lines.iter().any(|l| l.contains("attacks you for 15 damage")));
        assert_eq!(creature_health(objects.get(&player_id).unwrap()), 85);
    }

    #[test]
    fn grant_effect_applies_to_actor() {
        let player_id = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("area:forest-path-001");
        let ply = player("player:hero-001", &room_id);
        let content = include_str!("../../modules/default/worlds/default_world/creatures.mudl");
        let anatomy = crate::mudl::parse_anatomy_file(content).unwrap();
        let mut objects = HashMap::from([(player_id.clone(), ply.clone())]);

        let outcome = execute_script(
            &ply,
            &ScriptAction::GrantEffect("regeneration".to_string(), ScriptTarget::Actor),
            &EventContext {
                actor_id: player_id.clone(),
                host_id: player_id.clone(),
                room_id: Some(room_id),
                target_id: None,
            },
            &mut objects,
            Some(&anatomy),
        );
        assert!(outcome.lines.iter().any(|l| l.contains("regeneration")));
        assert!(
            crate::creature::effects::active_effects(objects.get(&player_id).unwrap())
                .contains(&"regeneration".to_string())
        );
    }
}