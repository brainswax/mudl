//! Command-layer helpers shared by REPL and future frontends.

use crate::mudl::{load_module, LoadedUniverse};
use crate::object::{ObjectFactory, ObjectId};
use crate::persistence::Persistence;
use crate::world::{bootstrap_world, bundle_module, ModuleManifest};

/// Load the active MUDL universe from `MUDL_MODULE` / `MUDL_UNIVERSE` env or default.
pub fn load_active_universe() -> anyhow::Result<LoadedUniverse> {
    crate::mudl::load_module(crate::mudl::default_module_dir())
}

/// Bootstrap the active universe's world for a player.
pub async fn bootstrap_active_universe<P: Persistence>(
    factory: &ObjectFactory<P>,
    owner: ObjectId,
) -> anyhow::Result<(LoadedUniverse, crate::object::ObjectId)> {
    let universe = load_active_universe()?;
    let world = universe.active_world()?;
    let start = bootstrap_world(factory, owner, world).await?;
    Ok((universe, start))
}

/// Package a universe module directory for distribution.
pub fn package_module(module_dir: &str, output_dir: &str) -> anyhow::Result<ModuleManifest> {
    bundle_module(module_dir, output_dir)
}

/// Reload a universe module from disk (for hot-reload during development).
pub fn reload_universe(path: &str) -> anyhow::Result<LoadedUniverse> {
    load_module(path)
}
