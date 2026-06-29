use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::anatomy::{parse_anatomy_file, AnatomyRegistry};
use super::world_def::{parse_world_file, WorldDef};

/// A loaded MUDL module (anatomy + world content) resolved from `universe.mudl`.
#[derive(Debug, Clone)]
pub struct LoadedModule {
    pub name: String,
    pub root: PathBuf,
    pub universe: PathBuf,
    pub sources: Vec<PathBuf>,
    pub anatomy: AnatomyRegistry,
    pub world_defs: Vec<WorldDef>,
    pub starting_location: Option<String>,
}

/// Resolve the module directory (contains `universe.mudl`).
pub fn default_module_dir() -> PathBuf {
    std::env::var("MUDL_MODULE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("modules/default"))
}

/// Resolve path to `universe.mudl`.
pub fn default_universe_path() -> PathBuf {
    std::env::var("MUDL_UNIVERSE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_module_dir().join("universe.mudl"))
}

/// Load a module from its `universe.mudl` entrypoint (follows `@include` directives).
pub fn load_universe(universe_path: impl AsRef<Path>) -> anyhow::Result<LoadedModule> {
    let universe_path = universe_path.as_ref().canonicalize()?;
    let root = universe_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("universe path has no parent: {}", universe_path.display()))?
        .to_path_buf();

    let name = root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    let mut visited = HashSet::new();
    let sources = expand_includes(&universe_path, &root, &mut visited)?;

    let mut anatomy = AnatomyRegistry::default();
    let mut world_defs = Vec::new();
    let mut starting_location = None;

    for source in &sources {
        let content = fs::read_to_string(source)?;
        anatomy.merge(parse_anatomy_file(&content)?);
        let (defs, start) = parse_world_file(&content);
        world_defs.extend(defs);
        if start.is_some() {
            starting_location = start;
        }
    }

    if world_defs.is_empty() && starting_location.is_none() {
        anyhow::bail!(
            "No world content found in module {} (checked {} files)",
            name,
            sources.len()
        );
    }

    Ok(LoadedModule {
        name,
        root,
        universe: universe_path,
        sources,
        anatomy,
        world_defs,
        starting_location,
    })
}

/// Load module by directory (expects `universe.mudl` inside).
pub fn load_module(module_dir: impl AsRef<Path>) -> anyhow::Result<LoadedModule> {
    let module_dir = module_dir.as_ref();
    let universe = module_dir.join("universe.mudl");
    if !universe.exists() {
        anyhow::bail!(
            "No universe.mudl in module directory: {}",
            module_dir.display()
        );
    }
    load_universe(universe)
}

fn expand_includes(
    path: &Path,
    root: &Path,
    visited: &mut HashSet<PathBuf>,
) -> anyhow::Result<Vec<PathBuf>> {
    let canonical = path.canonicalize()?;
    if !visited.insert(canonical.clone()) {
        anyhow::bail!("Circular @include detected: {}", path.display());
    }

    let content = fs::read_to_string(path)?;
    let mut files = Vec::new();

    for line in content.lines() {
        let trimmed = line.split(';').next().unwrap_or(line).trim();
        if let Some(include_path) = trimmed.strip_prefix("@include ") {
            let target = root.join(include_path.trim());
            if !target.exists() {
                anyhow::bail!(
                    "Included file not found: {} (from {})",
                    target.display(),
                    path.display()
                );
            }
            files.extend(expand_includes(&target, root, visited)?);
        }
    }

    files.push(path.to_path_buf());
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_default_module_via_universe() {
        let module = load_module("modules/default").unwrap();
        assert_eq!(module.name, "default");
        assert!(module.anatomy.body_plan("human").is_some());
        assert!(module.anatomy.player_template("default").is_some());
        assert_eq!(module.starting_location.as_deref(), Some("the-void"));
        assert!(module.world_defs.iter().any(|d| d.base_name == "the-void"));
        assert!(module.sources.len() >= 4);
    }
}
