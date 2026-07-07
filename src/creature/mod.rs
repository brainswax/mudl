//! Creature systems — vitality, effects, and basic NPC behaviors (Milestone 3).

pub mod ai;
pub mod behavior;
pub mod combat;
pub mod effects;
pub mod equipment;
pub mod spawner;
pub mod vitality;

pub use ai::{npc_behaviors, npcs_in_room, run_on_enter_behaviors, NpcAction, NpcBehavior};
pub use behavior::{
    add_behavior_template, add_script_behavior, behavior_templates_to_property,
    build_creature_behavior_entries, creature_behaviors_to_property, format_creature_behavior_list,
    read_creature_behaviors, resolve_behavior_templates, run_creature_behaviors,
    run_on_enter_creature_behaviors, BehaviorOutcome, CreatureBehaviorEntry,
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
pub use spawner::{
    apply_spawner_def, count_active_spawns, despawn_creatures_from_spawner,
    destroy_spawners_for_target, is_spawner, is_spawner_infrastructure, pick_weighted_entry,
    run_on_enter_spawners, spawn_creature, spawner_entries, spawner_room_id, spawners_for_target,
    spawners_in_room, spawn_templates_to_property, SpawnResult,
};
pub use combat::{
    damage_creature, heal_creature, parse_vital_amount_args, CreatureCombatError,
    DEFAULT_DAMAGE_AMOUNT, DEFAULT_HEAL_AMOUNT, VitalAmountRequest,
};
pub use vitality::{
    apply_damage, creature_base_max_health, creature_def_for, creature_health,
    creature_is_defeated, creature_max_health, creature_skill, creature_stat,
    format_creature_stats_summary, format_health_clause, format_npc_health_clause, heal,
    init_creature_vitality, DEFAULT_MAX_HEALTH,
};