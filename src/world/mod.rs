pub mod bootstrap;
pub mod container_fit;
pub mod dirty;
pub mod discovery;
pub mod door;
pub mod exit_index;
pub mod exits;
pub mod event_script;
pub mod event_subscribers;
pub mod events;
pub mod module;
pub mod move_manager;
pub mod navigation;
pub mod place_builder;
pub mod portal;
pub mod possession;
pub mod scheduler;
pub mod session;
pub mod stack_transfer;

pub use bootstrap::bootstrap_world;
pub use dirty::{persist_dirty, DirtyTracker};
pub use discovery::{
    entity_visible_to_player, is_object_hidden_from_player, object_visible_to_player,
    run_discovery_on_look, run_object_discovery_on_look,
};
pub use door::{door_for_direction, door_passage_block, door_permits_exit, DoorBlock};
pub use exit_index::{normalize_exit_input, ExitIndex};
pub use exits::{
    apply_loop_entry, apply_scatter_exit, can_traverse_exit, pick_scatter_destination,
    validate_place_exits, validate_place_hierarchy, validate_reciprocal_exits, validate_world_places,
};
pub use event_script::{
    execute_host_event, execute_script, parse_script, resolve_place_id, ScriptAction,
};
pub use events::{
    attach_triggers, emit_on_move_event, execute_event, execute_kill_events,
    format_trigger_script, run_event_handlers_on, EventContext, EventOutcome,
};
pub use module::{active_module_dir, bundle_module, list_universe_files, ModuleManifest};
pub use move_manager::{
    move_object, move_to_container, move_to_grasp, move_to_inventory, move_to_room,
    resolve_location, MoveContext, MoveError, MoveEvent, MoveHooks, MoveResult,
};
pub use navigation::{
    exit_directions, exit_index, movement_direction_from_line, movement_from_line, movement_input,
    resolve_exit, resolve_exit_map,
};
pub use place_builder::{
    apply_dig_result, dig_place, link_exit, link_places, unlink_exit, DigOptions, DigRequest,
    DigResult, PlaceBuildError,
};
pub use portal::{
    passable_portal_blocks_passage, passable_portal_for_direction, portal_for_direction,
    portal_kind_label, portal_passage_block, portal_permits_exit, portals_in_room, PortalBlock,
};
pub use scheduler::{
    advance_tick, current_tick, due_schedule_jobs, increment_counter, periodic_fires,
    read_counter, register_schedule_job, reset_counter,
};
pub use possession::{
    body_slot_item, body_slot_item_valid, body_slots, carried_body_items,
    clear_creature_slots_for_item, clear_item_from_body_slots, grasp_action_phrase,
    grasp_slot_available, grasp_slot_names, is_carried_by, is_in_player_possession,
    place_in_grasp_slots, prepare_grasp_placement, prune_creature_body_slots,
    prune_stale_body_slots, set_body_slot, PossessionError,
};
pub use session::{
    hydrate_world, persist_all, persist_objects, resolve_bootstrap_location,
    resolve_player_location, restore_session, WorldSession,
};
pub use stack_transfer::{
    compute_stack_transfer_plan, split_stack_id, stack_merge_key, StackTransferPlan,
};
