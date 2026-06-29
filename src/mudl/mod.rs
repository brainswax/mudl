pub mod anatomy;
pub mod loader;
pub mod world_def;

pub use anatomy::{
    parse_anatomy_file, slot_display_name, AnatomyRegistry, BodyPlan, BodySlotDef, CreatureDef,
    PlayerTemplate, SlotType,
};
pub use loader::{
    default_module_dir, default_universe_path, load_module, load_universe, LoadedUniverse,
    LoadedWorld,
};
pub use world_def::{parse_world_file, WorldDef};