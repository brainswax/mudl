use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::mudl::{default_module_dir, load_module, LoadedModule};

/// Manifest describing a packaged module (for distribution or inspection).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleManifest {
    pub name: String,
    pub root: String,
    pub universe: String,
    pub files: Vec<String>,
}

/// Bundle a module folder into an output directory with a `manifest.json`.
pub fn bundle_module(
    module_dir: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
) -> anyhow::Result<ModuleManifest> {
    let module = load_module(module_dir.as_ref())?;
    let output = output_dir.as_ref();
    fs::create_dir_all(output)?;

    let mut files = Vec::new();
    for source in &module.sources {
        let rel = source
            .strip_prefix(&module.root)
            .unwrap_or(source.as_path());
        let dest = output.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, &dest)?;
        files.push(rel.display().to_string());
    }

    let manifest = ModuleManifest {
        name: module.name.clone(),
        root: module.root.display().to_string(),
        universe: module
            .universe
            .strip_prefix(&module.root)
            .unwrap_or(module.universe.as_path())
            .display()
            .to_string(),
        files,
    };

    let manifest_path = output.join("manifest.json");
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;

    Ok(manifest)
}

/// List `.mudl` source files in a loaded module.
pub fn list_module_files(module: &LoadedModule) -> Vec<PathBuf> {
    module.sources.clone()
}

/// Resolve the active module directory from environment or default.
pub fn active_module_dir() -> PathBuf {
    default_module_dir()
}
