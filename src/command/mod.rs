//! Command-layer helpers shared by REPL and future frontends.

use crate::mudl::{load_module, LoadedModule};
use crate::object::{ObjectFactory, ObjectId};
use crate::persistence::Persistence;
use crate::world::{bootstrap_module, bundle_module, ModuleManifest};

/// Load the active MUDL module from `MUDL_MODULE` / `MUDL_UNIVERSE` env or default.
pub fn load_active_module() -> anyhow::Result<LoadedModule> {
    crate::mudl::load_module(crate::mudl::default_module_dir())
}

/// Bootstrap the active module for a player.
pub async fn bootstrap_active_module<P: Persistence>(
    factory: &ObjectFactory<P>,
    owner: ObjectId,
) -> anyhow::Result<(LoadedModule, crate::object::ObjectId)> {
    let module = load_active_module()?;
    let start = bootstrap_module(factory, owner, &module).await?;
    Ok((module, start))
}

/// Package a module directory for distribution.
pub fn package_module(module_dir: &str, output_dir: &str) -> anyhow::Result<ModuleManifest> {
    bundle_module(module_dir, output_dir)
}

/// Reload a module from disk (for hot-reload during development).
pub fn reload_module(path: &str) -> anyhow::Result<LoadedModule> {
    load_module(path)
}
