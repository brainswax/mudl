//! Creature systems — vitality, effects, and basic NPC behaviors (Milestone 3).

pub mod ai;
pub mod behavior;
pub mod combat;
pub mod conditions;
pub mod effects;
pub mod equipment;
pub mod progression;
pub mod spawner;
pub mod stats;
pub mod tactics;
pub mod vitality;

pub use ai::{npc_behaviors, npcs_in_room, run_on_enter_behaviors, NpcAction, NpcBehavior};
pub use behavior::{
    add_behavior_template, add_script_behavior, behavior_templates_to_property,
    bootstrap_creature_behavior_system, build_creature_behavior_entries,
    collect_behavior_triggers, creature_attack_damage, creature_behaviors_to_property,
    format_creature_behavior_list, DEFAULT_ATTACK_DAMAGE,
    read_creature_behaviors, resolve_behavior_templates, run_creature_behaviors,
    run_on_enter_creature_behaviors, run_perception_discovery_on_look, BehaviorOutcome,
    CreatureBehaviorEntry,
};
pub use combat::{
    attack_creature, compute_combat_damage, damage_creature, heal_creature,
    parse_vital_amount_args, resolve_combat_hit, AttackOutcome, CombatHit, CreatureCombatError,
    VitalAmountRequest, CRITICAL_DAMAGE_BONUS, DEFAULT_DAMAGE_AMOUNT, DEFAULT_HEAL_AMOUNT,
};
pub use conditions::{
    apply_condition, creature_has_condition_tag, creature_has_effect, cure_by_tag,
    remove_condition, tick_conditions, ConditionTickOutcome,
};
pub use effects::{
    active_effects, apply_effect, collect_active_effect_modifiers, effect_encumbrance_factor,
    effect_max_weight_bonus, refresh_effect_derived_properties, remove_effect, EffectModifiers,
};
pub use equipment::{
    apply_equipment_regen_on_enter, collect_equipment_modifiers, creature_effective_max_health,
    creature_effective_skill, creature_effective_stat, equipment_granted_encumbrance_factor,
    item_has_equipment_modifiers, EquipmentModifiers,
};
pub use tactics::{
    apply_tactics_from_behaviors, creature_visible_to_player, initiative_score,
    is_creature_aware, is_creature_discovered, is_creature_hidden_from_player, is_player_aware,
    player_perception_score, player_stealth_score, reset_player_awareness_on_enter,
    resolve_encounter_awareness_on_enter, resolve_strike_order, roll_awareness_on_enter,
    set_creature_aware, set_creature_discovered, set_player_aware, EncounterAwareness, StrikeOrder,
    SURPRISE_DAMAGE_BONUS,
};
pub use spawner::{
    apply_spawner_def, count_active_spawns, despawn_creatures_from_spawner,
    destroy_spawners_for_target, dispatch_creature_spawners_for_event, is_spawner,
    is_spawner_infrastructure, pick_weighted_entry, run_on_enter_spawners, spawn_creature,
    spawn_templates_to_property, spawner_entries, spawner_room_id, spawners_for_target,
    spawners_in_room, SpawnResult,
};
pub use progression::{award_skill_xp, SKILL_XP_PER_RANK};
pub use stats::{
    format_creature_gauge, max_health_from_constitution, max_health_from_creature_def,
    CORE_SKILLS, CORE_STATS, CONSTITUTION_HEALTH_BASELINE, HEALTH_PER_CONSTITUTION_POINT,
};
pub use vitality::{
    apply_damage, creature_base_max_health, creature_def_for, creature_health,
    creature_is_defeated, creature_max_health, creature_skill, creature_stat,
    format_creature_stats_summary, format_creature_stats_summary_with_equipment,
    format_creature_vitals_summary, format_health_clause, format_npc_health_clause, heal,
    init_creature_vitality, DEFAULT_MAX_HEALTH,
};
