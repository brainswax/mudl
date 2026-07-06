pub mod bootstrap;
pub mod container_fit;
pub mod dirty;
pub mod possession;
pub mod stack_transfer;
pub mod module;
pub mod move_manager;
pub mod session;

pub use bootstrap::bootstrap_world;
pub use dirty::{persist_dirty, DirtyTracker};
pub use module::{active_module_dir, bundle_module, list_universe_files, ModuleManifest};
pub use move_manager::{
    move_object, move_to_container, move_to_grasp, move_to_inventory, move_to_room, resolve_location,
    MoveContext, MoveError, MoveEvent, MoveHooks, MoveResult,
};
pub use possession::{
    body_slot_item, body_slot_item_valid, body_slots, carried_body_items, clear_creature_slots_for_item,
    clear_item_from_body_slots, grasp_action_phrase, grasp_slot_available, grasp_slot_names,
    is_carried_by, is_in_player_possession, place_in_grasp_slots, prepare_grasp_placement,
    prune_creature_body_slots, prune_stale_body_slots, set_body_slot, PossessionError,
};
pub use stack_transfer::{
    compute_stack_transfer_plan, split_stack_id, stack_merge_key, StackTransferPlan,
};
pub use session::{
    hydrate_world, persist_all, persist_objects, resolve_bootstrap_location,
    resolve_player_location, restore_session, WorldSession,
};
