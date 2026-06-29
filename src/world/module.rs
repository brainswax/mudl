use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::mudl::{default_module_dir, load_module, LoadedUniverse};

/// Manifest describing a packaged universe module (for distribution or inspection).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleManifest {
    pub name: String,
    pub root: String,
    pub universe: String,
    pub default_world: String,
    pub worlds: Vec<String>,
    pub files: Vec<String>,
}

/// Bundle a universe module folder into an output directory with a `manifest.json`.
pub fn bundle_module(
    module_dir: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
) -> anyhow::Result<ModuleManifest> {
    let universe = load_module(module_dir.as_ref())?;
    let output = output_dir.as_ref();
    fs::create_dir_all(output)?;

    let mut files = Vec::new();
    for source in universe.all_sources() {
        let rel = source
            .strip_prefix(&universe.root)
            .unwrap_or(source.as_path());
        let dest = output.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&source, &dest)?;
        files.push(rel.display().to_string());
    }

    let mut worlds: Vec<String> = universe.worlds.keys().cloned().collect();
    worlds.sort();

    let manifest = ModuleManifest {
        name: universe.name.clone(),
        root: universe.root.display().to_string(),
        universe: universe
            .universe
            .strip_prefix(&universe.root)
            .unwrap_or(universe.universe.as_path())
            .display()
            .to_string(),
        default_world: universe.default_world.clone(),
        worlds,
        files,
    };

    let manifest_path = output.join("manifest.json");
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;

    Ok(manifest)
}

/// List `.mudl` source files in a loaded universe (universe + all worlds).
pub fn list_universe_files(universe: &LoadedUniverse) -> Vec<PathBuf> {
    universe.all_sources()
}

/// Resolve the active module directory from environment or default.
pub fn active_module_dir() -> PathBuf {
    default_module_dir()
}
