pub mod bootstrap;
pub mod container_fit;
pub mod dirty;
pub mod door;
pub mod exits;
pub mod place_builder;
pub mod portal;
pub mod navigation;
pub mod possession;
pub mod stack_transfer;
pub mod module;
pub mod move_manager;
pub mod session;

pub use bootstrap::bootstrap_world;
pub use dirty::{persist_dirty, DirtyTracker};
pub use module::{active_module_dir, bundle_module, list_universe_files, ModuleManifest};
pub use door::{door_for_direction, door_passage_block, door_permits_exit, DoorBlock};
pub use portal::{
    passable_portal_blocks_passage, passable_portal_for_direction, portal_for_direction,
    portal_kind_label, portal_passage_block, portal_permits_exit, portals_in_room, PortalBlock,
};
pub use exits::{
    apply_scatter_exit, can_traverse_exit, pick_scatter_destination, reverse_direction,
    validate_place_exits, validate_place_hierarchy, validate_reciprocal_exits,
    validate_world_places,
};
pub use place_builder::{
    apply_dig_result, dig_place, link_exit, link_places, unlink_exit, DigOptions, DigRequest,
    DigResult, PlaceBuildError,
};
pub use navigation::{
    exit_directions, is_direction_verb, movement_direction_from_line, normalize_direction,
    resolve_exit,
};
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
