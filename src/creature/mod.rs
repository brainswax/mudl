//! Creature systems — vitality, effects, and basic NPC behaviors (Milestone 3).

pub mod ai;
pub mod effects;
pub mod vitality;

pub use ai::{npc_behaviors, npcs_in_room, run_on_enter_behaviors, NpcAction, NpcBehavior};
pub use effects::{
    active_effects, apply_effect, collect_active_effect_modifiers, effect_encumbrance_factor,
    effect_max_weight_bonus, refresh_effect_derived_properties, remove_effect, EffectModifiers,
};
pub use vitality::{
    apply_damage, creature_base_max_health, creature_def_for, creature_health,
    creature_max_health, creature_skill, creature_stat, format_health_clause, heal,
    init_creature_vitality, DEFAULT_MAX_HEALTH,
};