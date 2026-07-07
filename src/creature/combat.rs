//! Simple damage and healing for players and NPCs.

use std::collections::HashMap;
use std::fmt;

use crate::creature::vitality::{
    apply_damage, creature_health, creature_is_defeated, creature_max_health, heal,
};
use crate::display::{resolve_object, ResolveScope, TargetResolution};
use crate::mudl::AnatomyRegistry;
use crate::object::{Object, ObjectId};

/// Default damage when a wizard omits the amount.
pub const DEFAULT_DAMAGE_AMOUNT: i64 = 10;

/// Default healing when a wizard omits the amount.
pub const DEFAULT_HEAL_AMOUNT: i64 = 10;

/// Errors from damage/heal commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreatureCombatError {
    NotFound(String),
    NotCreature(String),
    Defeated(String),
    InvalidAmount(String),
}

impl fmt::Display for CreatureCombatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(name) => write!(f, "You don't see any {name} here."),
            Self::NotCreature(name) => write!(f, "The {name} isn't a living creature."),
            Self::Defeated(name) => write!(f, "The {name} is already down."),
            Self::InvalidAmount(msg) => write!(f, "{msg}"),
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
pub fn parse_vital_amount_args(rest: &str, default_amount: i64) -> Result<VitalAmountRequest, CreatureCombatError> {
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

fn mark_dirty(dirty: Option<&mut crate::world::DirtyTracker>, id: &ObjectId) {
    if let Some(dirty) = dirty {
        dirty.mark(id);
    }
}

/// Apply damage to a creature in the room or in possession.
pub fn damage_creature(
    actor_id: &ObjectId,
    room_id: Option<&ObjectId>,
    objects: &mut HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
    dirty: Option<&mut crate::world::DirtyTracker>,
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
    mark_dirty(dirty, &target_id);

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
    dirty: Option<&mut crate::world::DirtyTracker>,
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
    mark_dirty(dirty, &target_id);

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
    before: i64,
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
    use crate::creature::vitality::init_creature_vitality;
    use crate::mudl::{BodySlotDef, CreatureDef, PlayerTemplate, SlotType};
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
    fn damage_and_heal_creature_update_health() {
        let actor = ObjectId::new("player:admin-001");
        let room = ObjectId::new("area:room-001");
        let mut watcher = creature("npc:watcher-001", "Path Watcher");
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