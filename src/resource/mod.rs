//! Renewable resource spawner system — weighted harvest nodes on triggers.

pub mod spawner;

pub use spawner::{
    apply_resource_spawner_def, count_active_resources, dispatch_resource_spawners_for_event,
    is_resource_spawner, is_resource_spawner_infrastructure, resource_spawner_entries,
    resource_spawners_for_target, resource_templates_to_property, ResourceSpawnResult,
};