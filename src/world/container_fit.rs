//! Backward-compatible re-exports — implementation lives in `stack_transfer`.

pub use crate::world::stack_transfer::{
    cap_inventory_fit_to_weight, compute_container_fit, compute_inventory_fit,
    find_mergeable_stack, find_mergeable_stack_in_grasp, fit_failure_reason, split_stack_id,
    stack_merge_key, ContainerFit, InventoryFit,
};