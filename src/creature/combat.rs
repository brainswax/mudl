//! Combat, damage, healing, death, and corpses for players and NPCs.

use std::collections::HashMap;
use std::fmt;

use crate::creature::behavior::{creature_attack_damage, read_creature_behaviors};
use crate::creature::equipment::{
    creature_effective_max_health, creature_effective_skill, creature_effective_stat,
};
use crate::creature::progression::award_skill_xp;
use crate::creature::tactics::{
    is_creature_aware, is_creature_hidden_from_player, is_player_aware, resolve_strike_order,
    set_creature_aware, set_creature_discovered, set_player_aware, StrikeOrder,
    SURPRISE_DAMAGE_BONUS,
};
use crate::creature::vitality::{
    apply_damage, creature_health, creature_is_defeated, creature_max_health, heal,
};
use crate::display::{resolve_object, ResolveScope, TargetResolution};

use crate::world::execute_kill_events;
use crate::mudl::{AnatomyRegistry, CreatureReact};
use crate::object::{
    generate_object_id, id_base_from_display_name, ContainerSpec, Object, ObjectId,
    PermissionFlags, Property, Value,
};

/// Default damage when a wizard omits the amount.
pub const DEFAULT_DAMAGE_AMOUNT: i64 = 10;

/// Default healing when a wizard omits the amount.
pub const DEFAULT_HEAL_AMOUNT: i64 = 10;

/// Extra damage on a clean strike from combat skill (non-surprise).
pub const CRITICAL_DAMAGE_BONUS: i64 = 2;

/// Resolved damage for one melee strike — includes awareness and skill crits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombatHit {
    pub damage: i64,
    pub surprise: bool,
    pub critical: bool,
}

/// Errors from damage/heal commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreatureCombatError {
    NotFound(String),
    NotCreature(String),
    Defeated(String),
    InvalidAmount(String),
    SelfTarget,
    ActorDefeated,
    NoRoom,
}

impl fmt::Display for CreatureCombatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(name) => write!(f, "You don't see any {name} here."),
            Self::NotCreature(name) => write!(f, "The {name} isn't a living creature."),
            Self::Defeated(name) => write!(f, "The {name} is already down."),
            Self::InvalidAmount(msg) => write!(f, "{msg}"),
            Self::SelfTarget => write!(f, "You can't attack yourself."),
            Self::ActorDefeated => write!(f, "You are in no shape to fight."),
            Self::NoRoom => write!(f, "You are nowhere to fight from."),
        }
    }
}

/// Outcome of an `attack` exchange — narrative lines and persistence hints.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttackOutcome {
    pub lines: Vec<String>,
    pub dirty: Vec<ObjectId>,
    /// When the player dies and respawns at home, the session should update location.
    pub respawn_location: Option<ObjectId>,
}

impl AttackOutcome {
    fn push_line(&mut self, line: String) {
        if !line.is_empty() {
            self.lines.push(line);
        }
    }

    fn mark_dirty(&mut self, id: &ObjectId) {
        if !self.dirty.iter().any(|d| d == id) {
            self.dirty.push(id.clone());
        }
    }
}

impl std::error::Error for CreatureCombatError {}

/// Parsed `@damage` / `@heal` trailing arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VitalAmountRequest {
    pub target_name: String,
    pub amount: i64,
}

/// Parse `damage <target...> [amount]` / `heal <target...> [amount]`.
pub fn parse_vital_amount_args(
    rest: &str,
    default_amount: i64,
) -> Result<VitalAmountRequest, CreatureCombatError> {
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    if tokens.is_empty() {
        return Err(CreatureCombatError::InvalidAmount(
            "Usage: damage <creature> [amount]".to_string(),
        ));
    }

    if let Ok(amount) = tokens.last().unwrap().parse::<i64>() {
        if tokens.len() < 2 {
            return Err(CreatureCombatError::InvalidAmount(
                "Usage: damage <creature> [amount]".to_string(),
            ));
        }
        let target_name = tokens[..tokens.len() - 1].join(" ");
        if amount < 0 {
            return Err(CreatureCombatError::InvalidAmount(
                "Amount must be zero or greater.".to_string(),
            ));
        }
        return Ok(VitalAmountRequest {
            target_name,
            amount,
        });
    }

    Ok(VitalAmountRequest {
        target_name: tokens.join(" "),
        amount: default_amount,
    })
}

fn resolve_creature_target(
    name: &str,
    actor_id: &ObjectId,
    room_id: Option<&ObjectId>,
    objects: &HashMap<ObjectId, Object>,
) -> Result<ObjectId, CreatureCombatError> {
    let resolution = resolve_object(
        name,
        actor_id,
        room_id,
        objects,
        ResolveScope::PossessionOrRoom,
    );
    match resolution {
        TargetResolution::Found(id) => Ok(id),
        TargetResolution::NotFound => Err(CreatureCombatError::NotFound(name.to_string())),
        TargetResolution::Ambiguous(hint) => Err(CreatureCombatError::NotFound(hint)),
    }
}

fn mark_dirty(dirty: &mut Option<&mut crate::world::DirtyTracker>, id: &ObjectId) {
    if let Some(tracker) = dirty.as_deref_mut() {
        tracker.mark(id);
    }
}

fn resolve_room_creature_target(
    name: &str,
    actor_id: &ObjectId,
    room_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
) -> Result<ObjectId, CreatureCombatError> {
    let resolution = resolve_object(
        name,
        actor_id,
        Some(room_id),
        objects,
        ResolveScope::RoomOnly,
    );
    match resolution {
        TargetResolution::Found(id) => Ok(id),
        TargetResolution::NotFound => Err(CreatureCombatError::NotFound(name.to_string())),
        TargetResolution::Ambiguous(hint) => Err(CreatureCombatError::NotFound(hint)),
    }
}

/// Compute melee damage from attacker stats, equipment, health, and defender mitigation.
pub fn compute_combat_damage(
    attacker: &Object,
    defender: &Object,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
    surprise: bool,
) -> i64 {
    let strength = creature_effective_stat(attacker, "strength", objects, anatomy);
    let combat = creature_effective_skill(attacker, "combat", objects, anatomy);
    let constitution = creature_effective_stat(defender, "constitution", objects, anatomy);
    let dexterity = creature_effective_stat(defender, "dexterity", objects, anatomy);

    let attack_power = strength
        .saturating_add(2)
        .saturating_add(combat / 2)
        .max(3);
    let defense = (constitution / 3) + (dexterity / 4);
    let mut damage = (attack_power - defense).max(1);
    if surprise {
        damage = damage.saturating_add(SURPRISE_DAMAGE_BONUS);
    }
    damage
}

fn combat_exchange_seed(
    attacker_id: &ObjectId,
    defender_id: &ObjectId,
    room_id: &ObjectId,
) -> u64 {
    let mut hash = 0u64;
    for part in [attacker_id.as_str(), defender_id.as_str(), room_id.as_str()] {
        for byte in part.as_bytes() {
            hash = hash.wrapping_mul(31).wrapping_add(u64::from(*byte));
        }
    }
    hash
}

/// Resolve one strike with light variance, surprise bonus, and critical hits.
#[allow(clippy::too_many_arguments)]
pub fn resolve_combat_hit(
    attacker: &Object,
    defender: &Object,
    attacker_id: &ObjectId,
    defender_id: &ObjectId,
    room_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
    surprise: bool,
) -> CombatHit {
    let base = compute_combat_damage(attacker, defender, objects, anatomy, false);
    let seed = combat_exchange_seed(attacker_id, defender_id, room_id);
    let variance = i64::from((seed % 3) as u32).saturating_sub(1);
    let combat = creature_effective_skill(attacker, "combat", objects, anatomy);
    let skill_crit = !surprise && combat >= 4 && seed % 15 == 7;
    let critical = surprise || skill_crit;
    let mut damage = base.saturating_add(variance);
    if surprise {
        damage = damage.saturating_add(SURPRISE_DAMAGE_BONUS);
    } else if skill_crit {
        damage = damage.saturating_add(CRITICAL_DAMAGE_BONUS);
    }
    CombatHit {
        damage: damage.max(1),
        surprise,
        critical,
    }
}

fn resolve_template_damage_hit(base: i64, surprise: bool, seed: u64) -> CombatHit {
    let variance = i64::from((seed % 3) as u32).saturating_sub(1);
    let critical = surprise;
    let mut damage = base.saturating_add(variance);
    if surprise {
        damage = damage.saturating_add(SURPRISE_DAMAGE_BONUS);
    }
    CombatHit {
        damage: damage.max(1),
        surprise,
        critical,
    }
}

fn wielded_weapon_label(attacker: &Object, objects: &HashMap<ObjectId, Object>) -> Option<String> {
    for (slot, item_id) in attacker.body_slots() {
        if !slot.contains("hand") {
            continue;
        }
        let Some(item) = objects.get(&item_id) else {
            continue;
        };
        if !item.is_active() {
            continue;
        }
        if !item.equipment_stat_mods().is_empty()
            || !item.equipment_skill_mods().is_empty()
            || item.equipment_max_health_bonus() != 0
        {
            return Some(item.name.to_lowercase());
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn npc_retaliation_hit(
    attacker: &Object,
    defender: &Object,
    attacker_id: &ObjectId,
    defender_id: &ObjectId,
    room_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
    surprise: bool,
) -> CombatHit {
    let seed = combat_exchange_seed(attacker_id, defender_id, room_id);
    let behaviors = read_creature_behaviors(attacker);
    if behaviors
        .iter()
        .any(|entry| entry.react == Some(CreatureReact::Attack))
    {
        return resolve_template_damage_hit(creature_attack_damage(attacker), surprise, seed);
    }
    resolve_combat_hit(
        attacker,
        defender,
        attacker_id,
        defender_id,
        room_id,
        objects,
        anatomy,
        surprise,
    )
}

fn next_corpse_index(objects: &HashMap<ObjectId, Object>) -> u32 {
    let max = objects
        .values()
        .filter(|obj| obj.get_bool_property("is_corpse").unwrap_or(false))
        .count();
    (max as u32).saturating_add(1)
}

fn move_item_into_container(
    item_id: &ObjectId,
    container_id: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
) {
    let Some(mut item) = objects.get(item_id).cloned() else {
        return;
    };
    item.location = Some(container_id.clone());
    item.set_carried_slot(None);
    objects.insert(item_id.clone(), item);

    let Some(mut container) = objects.get(container_id).cloned() else {
        return;
    };
    if !container.container_contents().contains(item_id) {
        container.add_to_list_property("contents", item_id.clone());
        objects.insert(container_id.clone(), container);
    }
}

fn create_creature_corpse(
    victim: &Object,
    room_id: &ObjectId,
    owner: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
    outcome: &mut AttackOutcome,
) -> ObjectId {
    let slug = id_base_from_display_name(&victim.name);
    let index = next_corpse_index(objects);
    let corpse_id = generate_object_id("item", &format!("{slug}-corpse"), index);
    let display = victim.name.to_lowercase();

    let mut corpse = Object {
        id: corpse_id.clone(),
        name: format!("corpse of {}", victim.name),
        aliases: vec!["corpse".to_string(), display.clone()],
        location: Some(room_id.clone()),
        prototype: None,
        owner: owner.clone(),
        permissions: PermissionFlags::EVERYONE,
        properties: HashMap::new(),
        verbs: HashMap::new(),
        event_handlers: HashMap::new(),
        is_deleted: false,
        deleted_at: None,
    };
    corpse.apply_container_role(&ContainerSpec {
        capacity: 24,
        open: true,
        ..ContainerSpec::default()
    });
    corpse.set_property_bool("is_corpse", true);
    corpse.set_property_object_ref("corpse_of", victim.id.clone());
    corpse.add_property(Property {
        name: "description".to_string(),
        value: Value::String(format!(
            "The lifeless body of {} lies here, stripped of warmth.",
            victim.name
        )),
        permissions: PermissionFlags::EVERYONE,
        behavior: None,
    });

    let gear: Vec<ObjectId> = victim.carried_body_items();
    objects.insert(corpse_id.clone(), corpse);
    outcome.mark_dirty(&corpse_id);

    for item_id in gear {
        move_item_into_container(&item_id, &corpse_id, objects);
        outcome.mark_dirty(&item_id);
    }

    corpse_id
}

fn strip_creature_gear(
    creature_id: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
    outcome: &mut AttackOutcome,
) {
    let Some(mut creature) = objects.get(creature_id).cloned() else {
        return;
    };
    creature.set_property_map("body_slots", HashMap::new());
    objects.insert(creature_id.clone(), creature);
    outcome.mark_dirty(creature_id);
}

fn handle_npc_death(
    victim_id: &ObjectId,
    killer_id: &ObjectId,
    room_id: &ObjectId,
    owner: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
    outcome: &mut AttackOutcome,
) {
    let victim = objects.get(victim_id).cloned().unwrap();
    let display = victim.name.to_lowercase();
    let had_gear = !victim.carried_body_items().is_empty();

    let kill_outcome = execute_kill_events(victim_id, killer_id, room_id, objects, Some(anatomy));
    for line in kill_outcome.lines {
        outcome.push_line(line);
    }
    for id in kill_outcome.dirty {
        outcome.mark_dirty(&id);
    }

    create_creature_corpse(&victim, room_id, owner, objects, outcome);
    strip_creature_gear(victim_id, objects, outcome);

    if let Some(npc) = objects.get_mut(victim_id) {
        npc.soft_delete();
        outcome.mark_dirty(victim_id);
    }

    outcome.push_line(format!("The {display} crumples, leaving a corpse."));
    if had_gear {
        outcome.push_line("What they were carrying is left on the corpse.".to_string());
    }
    if killer_id != victim_id {
        outcome.push_line("You catch your breath.".to_string());
    }
}

fn handle_player_death(
    player_id: &ObjectId,
    killer_name: Option<&str>,
    room_id: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
    outcome: &mut AttackOutcome,
) {
    let player = objects.get(player_id).cloned().unwrap();
    let owner = player.owner.clone();
    let home_id = player
        .get_object_ref_property("home_location")
        .or_else(|| player.location.clone());

    create_creature_corpse(&player, room_id, &owner, objects, outcome);
    strip_creature_gear(player_id, objects, outcome);

    if let Some(home_id) = home_id.clone() {
        let max = objects
            .get(player_id)
            .map(|player| creature_effective_max_health(player, objects, anatomy))
            .unwrap_or(1);
        if let Some(player) = objects.get_mut(player_id) {
            player.location = Some(home_id.clone());
            player.set_property_int("health", max);
            outcome.mark_dirty(player_id);
        }
        outcome.respawn_location = Some(home_id);
    }

    if let Some(killer) = killer_name {
        outcome.push_line(format!("You collapse as {killer}'s blow lands."));
    } else {
        outcome.push_line("You take the hit and collapse.".to_string());
    }
    outcome.push_line(
        "You wake somewhere familiar — naked, alive, and lighter by everything you were carrying."
            .to_string(),
    );
    if let Some(ref home_id) = home_id {
        if let Some(home) = objects.get(home_id) {
            outcome.push_line(format!("You are in {}.", home.name));
        }
    }
}

fn player_strike_opener(hit: CombatHit) -> &'static str {
    if hit.surprise {
        "You catch them off guard with a punishing hit"
    } else if hit.critical {
        "You land a clean critical blow"
    } else {
        "You strike"
    }
}

fn npc_strike_opener(hit: CombatHit, initiative: bool) -> &'static str {
    if hit.surprise {
        "catches you off guard and strikes"
    } else if hit.critical {
        "lands a heavy blow"
    } else if initiative {
        "is quicker and strikes"
    } else {
        "strikes back"
    }
}

#[allow(clippy::too_many_arguments)]
fn format_player_strike_line(
    target_name: &str,
    weapon: Option<&str>,
    hit: CombatHit,
    after: i64,
    max: i64,
) -> String {
    let target = target_name.to_lowercase();
    let opener = player_strike_opener(hit);
    if let Some(weapon) = weapon {
        if hit.critical && !hit.surprise {
            return format!(
                "{opener}, driving your {weapon} into {target} for {damage} damage ({after}/{max} health).",
                damage = hit.damage
            );
        }
        return format!(
            "{opener} {target} with your {weapon} for {damage} damage ({after}/{max} health).",
            damage = hit.damage
        );
    }
    format!(
        "{opener} {target} for {damage} damage ({after}/{max} health).",
        damage = hit.damage
    )
}

fn format_npc_strike_line(
    attacker_name: &str,
    hit: CombatHit,
    initiative: bool,
    after: i64,
    max: i64,
) -> String {
    let attacker = attacker_name.to_lowercase();
    let verb = npc_strike_opener(hit, initiative);
    format!(
        "The {attacker} {verb} for {damage} damage ({after}/{max} health remaining).",
        damage = hit.damage
    )
}

fn format_defender_wound_state(name: &str, after: i64, max: i64) -> Option<String> {
    if after == 0 || max <= 0 {
        return None;
    }
    let pct = after * 100 / max;
    let display = name.to_lowercase();
    if pct < 25 {
        Some(format!("The {display} can barely stay on their feet."))
    } else if pct < 50 {
        Some(format!("The {display} is bleeding badly."))
    } else {
        None
    }
}

fn format_player_stagger(after: i64, max: i64) -> Option<String> {
    if after == 0 || max <= 0 {
        return None;
    }
    if after * 100 / max < 25 {
        Some("You stagger from the blow.".to_string())
    } else {
        None
    }
}

/// Player `attack <creature>` — turn-based exchange with NPC counter-attacks.
pub fn attack_creature(
    actor_id: &ObjectId,
    room_id: Option<&ObjectId>,
    objects: &mut HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
    mut dirty: Option<&mut crate::world::DirtyTracker>,
    target_name: &str,
) -> Result<AttackOutcome, CreatureCombatError> {
    let room_id = room_id.ok_or(CreatureCombatError::NoRoom)?;

    let actor = objects
        .get(actor_id)
        .ok_or(CreatureCombatError::ActorDefeated)?
        .clone();
    if !actor.has_creature_role() {
        return Err(CreatureCombatError::NotCreature("you".to_string()));
    }
    if creature_is_defeated(&actor) {
        return Err(CreatureCombatError::ActorDefeated);
    }

    let target_id = resolve_room_creature_target(target_name, actor_id, room_id, objects)?;
    if target_id == *actor_id {
        return Err(CreatureCombatError::SelfTarget);
    }

    let target = objects
        .get(&target_id)
        .ok_or_else(|| CreatureCombatError::NotFound(target_name.to_string()))?
        .clone();
    let target_display = target.name.to_lowercase();
    if !target.has_creature_role() {
        return Err(CreatureCombatError::NotCreature(target_display));
    }
    if creature_is_defeated(&target) {
        return Err(CreatureCombatError::Defeated(target_display));
    }
    if target.location.as_ref() != Some(room_id) {
        return Err(CreatureCombatError::NotFound(target_name.to_string()));
    }
    if is_creature_hidden_from_player(&target) {
        return Err(CreatureCombatError::NotFound(target_display));
    }

    let mut outcome = AttackOutcome::default();
    let owner = actor.owner.clone();

    let player_surprise = !is_creature_aware(&target);
    let npc_surprise = !is_player_aware(&actor);
    let order = if player_surprise {
        StrikeOrder::ActorFirst
    } else if npc_surprise {
        StrikeOrder::TargetFirst
    } else {
        resolve_strike_order(&actor, &target, objects, anatomy)
    };

    let weapon = wielded_weapon_label(&actor, objects);
    let npc_initiative = order == StrikeOrder::TargetFirst
        && !npc_surprise
        && is_creature_aware(&target);

    if order == StrikeOrder::TargetFirst
        && target.object_type() == "npc"
        && (is_creature_aware(&target) || npc_surprise)
    {
        let npc = objects.get(&target_id).cloned().unwrap();
        let hit = npc_retaliation_hit(
            &npc,
            &actor,
            &target_id,
            actor_id,
            room_id,
            objects,
            anatomy,
            npc_surprise,
        );
        let player_max = creature_effective_max_health(&actor, objects, anatomy);
        let player = objects.get_mut(actor_id).unwrap();
        let after = apply_damage(player, hit.damage);
        set_player_aware(player, true);
        outcome.mark_dirty(actor_id);
        mark_dirty(&mut dirty, actor_id);
        outcome.push_line(format_npc_strike_line(
            &npc.name,
            hit,
            npc_initiative,
            after,
            player_max,
        ));
        if let Some(line) = format_player_stagger(after, player_max) {
            outcome.push_line(line);
        }
        if after == 0 {
            handle_player_death(
                actor_id,
                Some(&npc.name),
                room_id,
                objects,
                anatomy,
                &mut outcome,
            );
            for id in &outcome.dirty {
                mark_dirty(&mut dirty, id);
            }
            return Ok(outcome);
        }
    }

    let player_hit = resolve_combat_hit(
        &actor,
        &target,
        actor_id,
        &target_id,
        room_id,
        objects,
        anatomy,
        player_surprise,
    );
    let target_max = creature_effective_max_health(&target, objects, anatomy);
    {
        let target_mut = objects.get_mut(&target_id).unwrap();
        let after = apply_damage(target_mut, player_hit.damage);
        set_creature_aware(target_mut, true);
        set_creature_discovered(target_mut, true);
        outcome.mark_dirty(&target_id);
        mark_dirty(&mut dirty, &target_id);
        outcome.push_line(format_player_strike_line(
            &target.name,
            weapon.as_deref(),
            player_hit,
            after,
            target_max,
        ));
        if let Some(line) = format_defender_wound_state(&target.name, after, target_max) {
            outcome.push_line(line);
        }
        if let Some(actor_mut) = objects.get_mut(actor_id) {
            let xp = if after == 0 { 5 } else { 1 };
            if let Some(msg) = award_skill_xp(actor_mut, "combat", xp) {
                outcome.push_line(msg);
            }
            outcome.mark_dirty(actor_id);
            mark_dirty(&mut dirty, actor_id);
        }
        if after == 0 {
            if target.object_type() == "npc" {
                handle_npc_death(
                    &target_id,
                    actor_id,
                    room_id,
                    &owner,
                    objects,
                    anatomy,
                    &mut outcome,
                );
            } else {
                handle_player_death(
                    &target_id,
                    Some(&actor.name),
                    room_id,
                    objects,
                    anatomy,
                    &mut outcome,
                );
            }
            for id in &outcome.dirty {
                mark_dirty(&mut dirty, id);
            }
            return Ok(outcome);
        }
    }

    if order == StrikeOrder::ActorFirst
        && target.object_type() == "npc"
    {
        let npc = objects.get(&target_id).cloned().unwrap();
        if !creature_is_defeated(&npc) && is_creature_aware(&npc) && !npc_surprise {
            let hit = npc_retaliation_hit(
                &npc,
                &actor,
                &target_id,
                actor_id,
                room_id,
                objects,
                anatomy,
                false,
            );
            let player_max = creature_effective_max_health(&actor, objects, anatomy);
            let player = objects.get_mut(actor_id).unwrap();
            let after = apply_damage(player, hit.damage);
            outcome.mark_dirty(actor_id);
            mark_dirty(&mut dirty, actor_id);
            outcome.push_line(format_npc_strike_line(
                &npc.name, hit, false, after, player_max,
            ));
            if let Some(line) = format_player_stagger(after, player_max) {
                outcome.push_line(line);
            }
            if after == 0 {
                handle_player_death(
                    actor_id,
                    Some(&npc.name),
                    room_id,
                    objects,
                    anatomy,
                    &mut outcome,
                );
            }
        }
    }

    for id in &outcome.dirty {
        mark_dirty(&mut dirty, id);
    }
    Ok(outcome)
}

/// Apply damage to a creature in the room or in possession.
pub fn damage_creature(
    actor_id: &ObjectId,
    room_id: Option<&ObjectId>,
    objects: &mut HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
    mut dirty: Option<&mut crate::world::DirtyTracker>,
    target_name: &str,
    amount: i64,
) -> Result<String, CreatureCombatError> {
    let target_id = resolve_creature_target(target_name, actor_id, room_id, objects)?;
    let target = objects
        .get(&target_id)
        .ok_or_else(|| CreatureCombatError::NotFound(target_name.to_string()))?
        .clone();

    let display = target.name.to_lowercase();
    if !target.has_creature_role() {
        return Err(CreatureCombatError::NotCreature(display));
    }
    if creature_is_defeated(&target) {
        return Err(CreatureCombatError::Defeated(display));
    }

    let before = creature_health(&target);
    let target = objects.get_mut(&target_id).unwrap();
    let after = apply_damage(target, amount);
    mark_dirty(&mut dirty, &target_id);

    Ok(format_damage_message(
        &target.name,
        target_id == *actor_id,
        before,
        after,
        creature_max_health(target, Some(anatomy)),
    ))
}

/// Heal a creature up to its effective maximum health.
pub fn heal_creature(
    actor_id: &ObjectId,
    room_id: Option<&ObjectId>,
    objects: &mut HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
    mut dirty: Option<&mut crate::world::DirtyTracker>,
    target_name: &str,
    amount: i64,
) -> Result<String, CreatureCombatError> {
    let target_id = resolve_creature_target(target_name, actor_id, room_id, objects)?;
    let target = objects
        .get(&target_id)
        .ok_or_else(|| CreatureCombatError::NotFound(target_name.to_string()))?
        .clone();

    let display = target.name.to_lowercase();
    if !target.has_creature_role() {
        return Err(CreatureCombatError::NotCreature(display));
    }

    let before = creature_health(&target);
    let max = creature_max_health(&target, Some(anatomy));
    if before >= max {
        return Ok(if target_id == *actor_id {
            "You are already at full health.".to_string()
        } else {
            format!("The {display} is already at full health.")
        });
    }

    let target = objects.get_mut(&target_id).unwrap();
    let after = heal(target, amount, Some(anatomy));
    mark_dirty(&mut dirty, &target_id);

    Ok(format_heal_message(
        &target.name,
        target_id == *actor_id,
        before,
        after,
        max,
    ))
}

/// Player-facing damage narration.
pub fn format_damage_message(
    name: &str,
    addressing_self: bool,
    before: i64,
    after: i64,
    max: i64,
) -> String {
    let display = name.to_lowercase();
    if after == 0 {
        if addressing_self {
            return "You take the hit and collapse.".to_string();
        }
        return format!("The {display} crumples.");
    }
    if addressing_self {
        if before == max {
            return format!("You take damage ({after}/{max} health).");
        }
        return format!("You take damage ({after}/{max} health).");
    }
    if after * 100 / max < 25 {
        return format!("The {display} staggers ({after}/{max} health).");
    }
    format!("The {display} takes damage ({after}/{max} health).")
}

/// Player-facing heal narration.
pub fn format_heal_message(
    name: &str,
    addressing_self: bool,
    _before: i64,
    after: i64,
    max: i64,
) -> String {
    let display = name.to_lowercase();
    if addressing_self {
        if after == max {
            return "You feel fully restored.".to_string();
        }
        return format!("You feel better ({after}/{max} health).");
    }
    if after == max {
        return format!("The {display} looks fully restored.");
    }
    format!("The {display} looks healthier ({after}/{max} health).")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::behavior::{creature_behaviors_to_property, CreatureBehaviorEntry};
    use crate::creature::vitality::init_creature_vitality;
    use crate::mudl::{BodySlotDef, CreatureDef, CreatureReact, PlayerTemplate, SlotType};
    use crate::object::PermissionFlags;

    fn human_def() -> CreatureDef {
        CreatureDef {
            name: "human".to_string(),
            slots: vec![BodySlotDef {
                name: "right_hand".to_string(),
                capacity: 1,
                slot_type: SlotType::Grasp,
                hands: 1,
                effect: None,
            }],
            max_health: 100,
            base_max_weight: Some(90),
            stats: HashMap::from([("strength".to_string(), 10)]),
            skills: HashMap::new(),
        }
    }

    fn creature(id: &str, name: &str) -> Object {
        let mut obj = Object {
            id: ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: Some(ObjectId::new("area:room-001")),
            prototype: None,
            owner: ObjectId::new("player:admin-001"),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        obj.init_creature_role(&PlayerTemplate {
            name: "hero".to_string(),
            creature: "human".to_string(),
            gender: "neutral".to_string(),
        });
        init_creature_vitality(&mut obj, &human_def());
        obj
    }

    #[test]
    fn parse_vital_amount_args_reads_trailing_number() {
        let req = parse_vital_amount_args("path watcher 25", DEFAULT_DAMAGE_AMOUNT).unwrap();
        assert_eq!(req.target_name, "path watcher");
        assert_eq!(req.amount, 25);
        let default_req = parse_vital_amount_args("self", DEFAULT_HEAL_AMOUNT).unwrap();
        assert_eq!(default_req.amount, DEFAULT_HEAL_AMOUNT);
    }

    #[test]
    fn compute_combat_damage_uses_stats_and_equipment() {
        let anatomy = AnatomyRegistry::default();
        let _room = ObjectId::new("area:room-001");
        let mut attacker = creature("player:hero-001", "Hero");
        let defender = creature("npc:watcher-001", "Path Watcher");
        let mut blade = Object {
            id: ObjectId::new("item:blade-001"),
            name: "Chipped Blade".to_string(),
            aliases: Vec::new(),
            location: Some(attacker.id.clone()),
            prototype: None,
            owner: ObjectId::new("player:hero-001"),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        blade.set_int_map("mod_stats", HashMap::from([("strength".to_string(), 2)]));
        attacker.set_body_slot("right_hand", Some(blade.id.clone()));
        let objects = HashMap::from([
            (attacker.id.clone(), attacker.clone()),
            (defender.id.clone(), defender.clone()),
            (blade.id.clone(), blade),
        ]);

        let base = compute_combat_damage(&attacker, &defender, &objects, &anatomy, false);
        let mut bare = attacker.clone();
        bare.set_property_map("body_slots", HashMap::new());
        let objects_bare = HashMap::from([
            (bare.id.clone(), bare.clone()),
            (defender.id.clone(), defender.clone()),
        ]);
        let unarmed = compute_combat_damage(&bare, &defender, &objects_bare, &anatomy, false);
        assert!(base > unarmed);
        assert!(base >= 6);
    }

    #[test]
    fn surprise_damage_bonus_on_unaware_target() {
        let anatomy = AnatomyRegistry::default();
        let room = ObjectId::new("area:room-001");
        let attacker = creature("player:hero-001", "Hero");
        let mut defender = creature("npc:lurker-001", "Pale Lurker");
        defender.set_property_bool("creature_aware", false);
        let objects = HashMap::from([
            (attacker.id.clone(), attacker.clone()),
            (defender.id.clone(), defender.clone()),
        ]);
        let normal = resolve_combat_hit(
            &attacker,
            &defender,
            &attacker.id,
            &defender.id,
            &room,
            &objects,
            &anatomy,
            false,
        );
        let surprise = resolve_combat_hit(
            &attacker,
            &defender,
            &attacker.id,
            &defender.id,
            &room,
            &objects,
            &anatomy,
            true,
        );
        assert!(surprise.critical);
        assert!(surprise.damage >= normal.damage + SURPRISE_DAMAGE_BONUS - 1);
    }

    #[test]
    fn resolve_combat_hit_variance_is_bounded() {
        let anatomy = AnatomyRegistry::default();
        let room = ObjectId::new("area:room-001");
        let attacker = creature("player:hero-001", "Hero");
        let defender = creature("npc:watcher-001", "Path Watcher");
        let objects = HashMap::from([
            (attacker.id.clone(), attacker.clone()),
            (defender.id.clone(), defender.clone()),
        ]);
        let hit = resolve_combat_hit(
            &attacker,
            &defender,
            &attacker.id,
            &defender.id,
            &room,
            &objects,
            &anatomy,
            false,
        );
        assert!(hit.damage >= 1);
    }

    #[test]
    fn combat_skill_increases_damage() {
        let anatomy = AnatomyRegistry::default();
        let novice = creature("player:hero-001", "Hero");
        let mut veteran = novice.clone();
        veteran.set_int_map("skills", HashMap::from([("combat".to_string(), 4)]));
        let defender = creature("npc:watcher-001", "Path Watcher");
        let novice_map = HashMap::from([
            (novice.id.clone(), novice.clone()),
            (defender.id.clone(), defender.clone()),
        ]);
        let veteran_map = HashMap::from([
            (veteran.id.clone(), veteran.clone()),
            (defender.id.clone(), defender.clone()),
        ]);
        let novice_damage =
            compute_combat_damage(&novice, &defender, &novice_map, &anatomy, false);
        let veteran_damage =
            compute_combat_damage(&veteran, &defender, &veteran_map, &anatomy, false);
        assert!(veteran_damage >= novice_damage);
    }

    #[test]
    fn attack_awards_combat_skill_xp() {
        let actor = ObjectId::new("player:admin-001");
        let room = ObjectId::new("area:room-001");
        let mut player = creature("player:admin-001", "Admin");
        player.location = Some(room.clone());
        player.set_int_map("skills", HashMap::from([("combat".to_string(), 0)]));
        let mut watcher = creature("npc:watcher-001", "Path Watcher");
        watcher.location = Some(room.clone());
        let watcher_id = watcher.id.clone();
        let mut objects = HashMap::from([
            (player.id.clone(), player),
            (watcher_id.clone(), watcher),
        ]);
        let anatomy = AnatomyRegistry::default();

        for _ in 0..5 {
            let _ = attack_creature(
                &actor,
                Some(&room),
                &mut objects,
                &anatomy,
                None,
                "path watcher",
            );
            if creature_health(objects.get(&watcher_id).unwrap()) == 0 {
                break;
            }
        }

        let hero = objects.get(&actor).unwrap();
        assert!(
            hero.get_int_map("skills").get("combat").copied().unwrap_or(0) >= 1
                || hero.get_int_map("skill_xp").get("combat").copied().unwrap_or(0) > 0
        );
    }

    #[test]
    fn attack_creature_damages_npc_and_triggers_counterattack() {
        let actor = ObjectId::new("player:admin-001");
        let room = ObjectId::new("area:room-001");
        let mut player = creature("player:admin-001", "Admin");
        player.location = Some(room.clone());
        let mut watcher = creature("npc:watcher-001", "Path Watcher");
        watcher.add_property(creature_behaviors_to_property(&[CreatureBehaviorEntry {
            entry_type: "template".to_string(),
            template_name: Some("aggressive".to_string()),
            react: Some(CreatureReact::Attack),
            event: Some("on_enter".to_string()),
            action: None,
            text: None,
            wander_interval: None,
            attack_damage: Some(12),
            awareness_check: None,
            perception: None,
        }]));
        let mut objects = HashMap::from([
            (player.id.clone(), player),
            (watcher.id.clone(), watcher.clone()),
        ]);
        let anatomy = AnatomyRegistry::default();

        let outcome = attack_creature(
            &actor,
            Some(&room),
            &mut objects,
            &anatomy,
            None,
            "path watcher",
        )
        .unwrap();
        assert!(outcome
            .lines
            .iter()
            .any(|l| l.contains("You strike") || l.contains("strike")));
        assert!(outcome
            .lines
            .iter()
            .any(|l| l.contains("strikes back") || l.contains("strikes")));
        assert!(creature_health(objects.get(&watcher.id).unwrap()) < 100);
        assert!(creature_health(objects.get(&actor).unwrap()) < 100);
    }

    #[test]
    fn killing_npc_leaves_corpse_with_gear() {
        let actor = ObjectId::new("player:admin-001");
        let room = ObjectId::new("area:room-001");
        let mut player = creature("player:admin-001", "Admin");
        player.location = Some(room.clone());
        let mut watcher = creature("npc:watcher-001", "Path Watcher");
        watcher.set_property_int("health", 5);
        let blade = Object {
            id: ObjectId::new("item:blade-001"),
            name: "Rusty Knife".to_string(),
            aliases: Vec::new(),
            location: Some(watcher.id.clone()),
            prototype: None,
            owner: ObjectId::new("player:admin-001"),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        watcher.set_body_slot("right_hand", Some(blade.id.clone()));
        let mut objects = HashMap::from([
            (player.id.clone(), player),
            (watcher.id.clone(), watcher.clone()),
            (blade.id.clone(), blade.clone()),
        ]);
        let anatomy = AnatomyRegistry::default();
        let blade_id = blade.id.clone();

        let outcome = attack_creature(
            &actor,
            Some(&room),
            &mut objects,
            &anatomy,
            None,
            "path watcher",
        )
        .unwrap();
        assert!(outcome.lines.iter().any(|l| l.contains("corpse")));
        assert!(objects.get(&watcher.id).unwrap().is_deleted);

        let corpse = objects
            .values()
            .find(|o| o.get_bool_property("is_corpse").unwrap_or(false))
            .expect("corpse");
        assert_eq!(corpse.location.as_ref(), Some(&room));
        assert_eq!(
            objects.get(&blade_id).unwrap().location.as_ref(),
            Some(&corpse.id)
        );
        assert!(objects
            .get(&watcher.id)
            .unwrap()
            .carried_body_items()
            .is_empty());
    }

    #[test]
    fn player_death_respawns_naked_at_home() {
        let actor = ObjectId::new("player:admin-001");
        let room = ObjectId::new("area:forest-path-001");
        let home = ObjectId::new("area:the-void-001");
        let mut player = creature("player:admin-001", "Admin");
        player.location = Some(room.clone());
        player.set_property_object_ref("home_location", home.clone());
        player.set_property_int("health", 8);
        let vest = Object {
            id: ObjectId::new("item:vest-001"),
            name: "Leather Vest".to_string(),
            aliases: Vec::new(),
            location: Some(player.id.clone()),
            prototype: None,
            owner: ObjectId::new("player:admin-001"),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        player.set_body_slot("torso", Some(vest.id.clone()));
        let mut watcher = creature("npc:watcher-001", "Path Watcher");
        watcher.location = Some(room.clone());
        watcher.add_property(creature_behaviors_to_property(&[CreatureBehaviorEntry {
            entry_type: "template".to_string(),
            template_name: Some("aggressive".to_string()),
            react: Some(CreatureReact::Attack),
            event: Some("on_enter".to_string()),
            action: None,
            text: None,
            wander_interval: None,
            attack_damage: Some(20),
            awareness_check: None,
            perception: None,
        }]));
        let mut objects = HashMap::from([
            (player.id.clone(), player),
            (watcher.id.clone(), watcher),
            (vest.id.clone(), vest.clone()),
        ]);
        let anatomy = AnatomyRegistry::default();
        let vest_id = vest.id.clone();

        let outcome = attack_creature(
            &actor,
            Some(&room),
            &mut objects,
            &anatomy,
            None,
            "path watcher",
        )
        .unwrap();
        assert_eq!(outcome.respawn_location, Some(home.clone()));
        assert!(outcome.lines.iter().any(|l| l.contains("wake")));
        let player = objects.get(&actor).unwrap();
        assert_eq!(player.location.as_ref(), Some(&home));
        assert!(player.carried_body_items().is_empty());
        assert!(creature_health(player) > 0);
        let corpse = objects
            .values()
            .find(|o| o.get_bool_property("is_corpse").unwrap_or(false))
            .expect("player corpse");
        assert_eq!(
            objects.get(&vest_id).unwrap().location.as_ref(),
            Some(&corpse.id)
        );
    }

    #[test]
    fn attack_hidden_creature_fails_until_discovered() {
        let actor = ObjectId::new("player:admin-001");
        let room = ObjectId::new("area:room-001");
        let mut player = creature("player:admin-001", "Admin");
        player.location = Some(room.clone());
        let mut lurker = creature("npc:lurker-001", "Pale Lurker");
        lurker.location = Some(room.clone());
        lurker.set_property_bool("uses_awareness_check", true);
        lurker.set_property_bool("player_discovered", false);
        let mut objects = HashMap::from([
            (player.id.clone(), player),
            (lurker.id.clone(), lurker),
        ]);
        let anatomy = AnatomyRegistry::default();

        let err = attack_creature(
            &actor,
            Some(&room),
            &mut objects,
            &anatomy,
            None,
            "pale lurker",
        )
        .unwrap_err();
        assert!(err.to_string().contains("don't see"));
    }

    #[test]
    fn unaware_player_takes_npc_surprise_first_strike() {
        let actor = ObjectId::new("player:admin-001");
        let room = ObjectId::new("area:room-001");
        let mut player = creature("player:admin-001", "Admin");
        player.location = Some(room.clone());
        player.set_property_bool("player_aware", false);
        let mut watcher = creature("npc:watcher-001", "Path Watcher");
        watcher.location = Some(room.clone());
        watcher.set_property_bool("creature_aware", true);
        watcher.add_property(creature_behaviors_to_property(&[CreatureBehaviorEntry {
            entry_type: "template".to_string(),
            template_name: Some("aggressive".to_string()),
            react: Some(CreatureReact::Attack),
            event: Some("on_enter".to_string()),
            action: None,
            text: None,
            wander_interval: None,
            attack_damage: Some(8),
            awareness_check: None,
            perception: None,
        }]));
        let mut objects = HashMap::from([
            (player.id.clone(), player),
            (watcher.id.clone(), watcher),
        ]);
        let anatomy = AnatomyRegistry::default();

        let outcome = attack_creature(
            &actor,
            Some(&room),
            &mut objects,
            &anatomy,
            None,
            "path watcher",
        )
        .unwrap();
        assert!(outcome
            .lines
            .iter()
            .any(|l| l.contains("catches you off guard")));
        assert!(outcome.lines.iter().any(|l| l.contains("off guard and strike")));
        assert!(crate::creature::tactics::is_player_aware(
            objects.get(&actor).unwrap()
        ));
    }

    #[test]
    fn damage_and_heal_creature_update_health() {
        let actor = ObjectId::new("player:admin-001");
        let room = ObjectId::new("area:room-001");
        let watcher = creature("npc:watcher-001", "Path Watcher");
        let mut objects = HashMap::from([(watcher.id.clone(), watcher.clone())]);
        let anatomy = AnatomyRegistry::default();

        let msg = damage_creature(
            &actor,
            Some(&room),
            &mut objects,
            &anatomy,
            None,
            "path watcher",
            30,
        )
        .unwrap();
        assert!(msg.contains("stagger") || msg.contains("damage"));
        assert_eq!(creature_health(objects.get(&watcher.id).unwrap()), 70);

        let heal_msg = heal_creature(
            &actor,
            Some(&room),
            &mut objects,
            &anatomy,
            None,
            "path watcher",
            15,
        )
        .unwrap();
        assert!(heal_msg.contains("healthier"));
        assert_eq!(creature_health(objects.get(&watcher.id).unwrap()), 85);
    }
}
