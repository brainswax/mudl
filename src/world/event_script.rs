//! Parse and execute MUDL `@trigger` script actions (narrate, react, teleport, spawn, …).

use std::collections::HashMap;

use crate::creature::behavior::{npc_attack_player, npc_flee_room, read_creature_behaviors};
use crate::creature::spawner::spawn_creature_from_template;
use crate::creature::vitality::{apply_damage, heal};
use crate::mudl::trigger_def::events;
use crate::mudl::{AnatomyRegistry, CreatureReact};
use crate::object::{Object, ObjectId};

use super::events::{EventContext, EventOutcome};

/// Parsed script verb + payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptAction {
    Narrate(String),
    Say(String),
    Emote(String),
    React(CreatureReact),
    Damage(i64),
    Heal(i64),
    ModStat(String, i64),
    ModSkill(String, i64),
    Teleport(String),
    Spawn(String),
    Raw(String),
}

/// Split `verb rest…` into a script action.
pub fn parse_script(code: &str) -> ScriptAction {
    let code = code.trim();
    if code.is_empty() {
        return ScriptAction::Raw(String::new());
    }
    let Some((verb, rest)) = code.split_once(char::is_whitespace) else {
        return match code.to_ascii_lowercase().as_str() {
            "attack" => ScriptAction::React(CreatureReact::Attack),
            "flee" => ScriptAction::React(CreatureReact::Flee),
            "greet" => ScriptAction::React(CreatureReact::Greet),
            "warn" => ScriptAction::React(CreatureReact::Warn),
            _ => ScriptAction::Raw(code.to_string()),
        };
    };
    let verb = verb.to_ascii_lowercase();
    let rest = rest.trim();
    match verb.as_str() {
        "narrate" | "message" => ScriptAction::Narrate(rest.to_string()),
        "say" => ScriptAction::Say(rest.to_string()),
        "emote" => ScriptAction::Emote(rest.to_string()),
        "react" if !rest.is_empty() => ScriptAction::React(CreatureReact::parse(rest)),
        "attack" | "flee" | "greet" | "warn" if rest.is_empty() => {
            ScriptAction::React(CreatureReact::parse(&verb))
        }
        "damage" => ScriptAction::Damage(rest.parse().unwrap_or(8)),
        "heal" => ScriptAction::Heal(rest.parse().unwrap_or(5)),
        "mod-stat" | "mod_stat" => {
            let mut parts = rest.split_whitespace();
            let stat = parts.next().unwrap_or("strength").to_string();
            let value = parts.next().and_then(|v| v.parse().ok()).unwrap_or(1);
            ScriptAction::ModStat(stat, value)
        }
        "mod-skill" | "mod_skill" => {
            let mut parts = rest.split_whitespace();
            let skill = parts.next().unwrap_or("survival").to_string();
            let value = parts.next().and_then(|v| v.parse().ok()).unwrap_or(1);
            ScriptAction::ModSkill(skill, value)
        }
        "teleport" | "send" => ScriptAction::Teleport(rest.to_string()),
        "spawn" => ScriptAction::Spawn(rest.to_string()),
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
        ScriptAction::Raw(text) if !text.is_empty() => Some(text.clone()),
        _ => None,
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
    let actor_id = ctx.actor_id.clone();
    let room_id = ctx
        .room_id
        .clone()
        .or_else(|| host.location.clone());

    match action {
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
                &actor_id,
                room_id.as_ref(),
                objects,
                anatomy,
                &mut outcome,
            );
        }
        ScriptAction::Damage(amount) => {
            if let Some(actor) = objects.get_mut(&actor_id) {
                if actor.has_creature_role() {
                    let after = apply_damage(actor, *amount);
                    outcome.push_line(format!(
                        "You take {amount} damage ({after} health remaining)."
                    ));
                    outcome.mark_dirty(&actor_id);
                }
            }
        }
        ScriptAction::Heal(amount) => {
            if let Some(actor) = objects.get_mut(&actor_id) {
                if actor.has_creature_role() {
                    let after = heal(actor, *amount, anatomy);
                    outcome.push_line(format!("You recover {amount} health ({after} remaining)."));
                    outcome.mark_dirty(&actor_id);
                }
            }
        }
        ScriptAction::ModStat(stat, delta) => {
            if let Some(actor) = objects.get_mut(&actor_id) {
                let mut stats = actor.get_int_map("stats");
                let current = stats.get(stat).copied().unwrap_or(0);
                stats.insert(stat.clone(), current + delta);
                actor.set_int_map("stats", stats);
                outcome.mark_dirty(&actor_id);
            }
        }
        ScriptAction::ModSkill(skill, delta) => {
            if let Some(actor) = objects.get_mut(&actor_id) {
                let mut skills = actor.get_int_map("skills");
                let current = skills.get(skill).copied().unwrap_or(0);
                skills.insert(skill.clone(), current + delta);
                actor.set_int_map("skills", skills);
                outcome.mark_dirty(&actor_id);
            }
        }
        ScriptAction::Teleport(place_base) => {
            if let Some(dest_id) = resolve_place_id(place_base, objects) {
                if let Some(actor) = objects.get_mut(&actor_id) {
                    actor.location = Some(dest_id.clone());
                    let dest_name = objects
                        .get(&dest_id)
                        .map(|p| p.name.to_lowercase())
                        .unwrap_or_else(|| place_base.clone());
                    outcome.push_line(format!("The world lurches — you find yourself at {dest_name}."));
                    outcome.mark_dirty(&actor_id);
                }
            }
        }
        ScriptAction::Spawn(template_name) => {
            let Some(room_id) = room_id.clone() else {
                return outcome;
            };
            let Some(owner) = objects.get(&actor_id).map(|o| o.owner.clone()) else {
                return outcome;
            };
            let anatomy = match anatomy {
                Some(a) => a,
                None => return outcome,
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
            }
        }
        ScriptAction::Raw(text) if !text.is_empty() => {
            outcome.push_line(text.clone());
        }
        ScriptAction::Raw(_) => {}
    }

    let _ = ctx.target_id.as_ref();
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
                .map(read_creature_behaviors)
                .map(|entries| {
                    entries
                        .iter()
                        .filter_map(|entry| entry.attack_damage)
                        .max()
                        .unwrap_or(10)
                        .max(1)
                })
                .unwrap_or(10);
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
        let action = parse_script(&code);
        let script_outcome = execute_script(&host, &action, ctx, objects, anatomy);
        outcome.append(script_outcome);
    }
    outcome
}

/// Convenience: run `on_kill` on victim and killer when a creature is slain.
pub fn execute_kill_events(
    victim_id: &ObjectId,
    killer_id: &ObjectId,
    room_id: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
    anatomy: Option<&AnatomyRegistry>,
) -> EventOutcome {
    let mut outcome = EventOutcome::default();

    let victim_ctx = EventContext {
        actor_id: killer_id.clone(),
        host_id: victim_id.clone(),
        room_id: Some(room_id.clone()),
        target_id: Some(killer_id.clone()),
    };
    outcome.append(execute_host_event(
        events::ON_DEATH,
        &victim_ctx,
        objects,
        anatomy,
    ));
    outcome.append(execute_host_event(
        events::ON_KILL,
        &victim_ctx,
        objects,
        anatomy,
    ));

    if killer_id != victim_id {
        let killer_ctx = EventContext {
            actor_id: victim_id.clone(),
            host_id: killer_id.clone(),
            room_id: Some(room_id.clone()),
            target_id: Some(victim_id.clone()),
        };
        outcome.append(execute_host_event(
            events::ON_KILL,
            &killer_ctx,
            objects,
            anatomy,
        ));
    }

    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::PermissionFlags;

    #[test]
    fn parse_react_and_mod_actions() {
        assert_eq!(
            parse_script("react flee"),
            ScriptAction::React(CreatureReact::Flee)
        );
        assert_eq!(parse_script("damage 12"), ScriptAction::Damage(12));
        assert_eq!(
            parse_script("mod-stat strength 2"),
            ScriptAction::ModStat("strength".to_string(), 2)
        );
        assert_eq!(
            parse_script("teleport haunted-entry"),
            ScriptAction::Teleport("haunted-entry".to_string())
        );
    }

    #[test]
    fn teleport_moves_actor_to_place() {
        let player_id = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("area:forest-path-001");
        let dest_id = ObjectId::new("area:haunted-entry-001");
        let mut player = Object {
            id: player_id.clone(),
            name: "Hero".to_string(),
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
        player.init_creature_role(&crate::mudl::PlayerTemplate {
            name: "hero".to_string(),
            creature: "human".to_string(),
            gender: "neutral".to_string(),
        });
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
            (player_id.clone(), player),
            (dest_id.clone(), dest),
        ]);
        let player_host = objects.get(&player_id).unwrap().clone();

        let outcome = execute_script(
            &player_host,
            &ScriptAction::Teleport("haunted-entry".to_string()),
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
}