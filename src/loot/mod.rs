//! Loot spawner system — weighted item drops on triggers (open, enter, kill, timer).

pub mod spawner;

pub use spawner::{
    apply_loot_spawner_def, count_active_loot, is_loot_spawner, is_loot_spawner_infrastructure,
    loot_spawner_entries, loot_spawners_for_target, loot_spawners_in_room,
    loot_templates_to_property, run_on_break_loot_spawners, run_on_enter_loot_spawners,
    run_on_kill_loot_spawners, run_on_open_loot_spawners, run_timer_loot_spawners, LootSpawnResult,
};
