use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::anatomy::{parse_anatomy_file, AnatomyRegistry};
use super::world_def::{parse_world_file, WorldDef};

/// A loaded world: self-contained game setting (locations, anatomy, creatures, items).
#[derive(Debug, Clone)]
pub struct LoadedWorld {
    pub name: String,
    pub root: PathBuf,
    pub entrypoint: PathBuf,
    pub sources: Vec<PathBuf>,
    pub anatomy: AnatomyRegistry,
    pub world_defs: Vec<WorldDef>,
    pub starting_location: Option<String>,
}

/// A loaded universe: top container holding one or more worlds.
#[derive(Debug, Clone)]
pub struct LoadedUniverse {
    pub name: String,
    pub root: PathBuf,
    pub universe: PathBuf,
    pub default_world: String,
    pub worlds: HashMap<String, LoadedWorld>,
}

impl LoadedUniverse {
    /// Resolve the active world from `MUDL_WORLD` or the universe default.
    pub fn active_world(&self) -> anyhow::Result<&LoadedWorld> {
        let name = std::env::var("MUDL_WORLD").unwrap_or_else(|_| self.default_world.clone());
        self.worlds
            .get(&name)
            .ok_or_else(|| anyhow::anyhow!("World not found in universe '{}': {name}", self.name))
    }

    /// All source files across every loaded world (for bundling).
    pub fn all_sources(&self) -> Vec<PathBuf> {
        let mut sources = vec![self.universe.clone()];
        let mut world_names: Vec<_> = self.worlds.keys().cloned().collect();
        world_names.sort();
        for name in world_names {
            if let Some(world) = self.worlds.get(&name) {
                sources.extend(world.sources.iter().cloned());
            }
        }
        sources
    }
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

/// Load a universe from its `universe.mudl` entrypoint (follows `@include-world` directives).
pub fn load_universe(universe_path: impl AsRef<Path>) -> anyhow::Result<LoadedUniverse> {
    let universe_path = universe_path.as_ref().canonicalize()?;
    let root = universe_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("universe path has no parent: {}", universe_path.display()))?
        .to_path_buf();

    let content = fs::read_to_string(&universe_path)?;
    let (universe_name, default_world) = parse_universe_meta(&content);
    let name = universe_name.unwrap_or_else(|| {
        root.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string()
    });
    let default_world = default_world.unwrap_or_else(|| "default_world".to_string());

    let mut worlds = HashMap::new();
    for line in content.lines() {
        let trimmed = line.split(';').next().unwrap_or(line).trim();
        if let Some(world_name) = trimmed.strip_prefix("@include-world ") {
            let world_name = world_name.trim();
            if world_name.is_empty() {
                continue;
            }
            let world_dir = root.join("worlds").join(world_name);
            let world = load_world(&world_dir, world_name)?;
            worlds.insert(world_name.to_string(), world);
        }
    }

    if worlds.is_empty() {
        anyhow::bail!(
            "No worlds loaded from universe {} (expected @include-world directives)",
            name
        );
    }

    if !worlds.contains_key(&default_world) {
        anyhow::bail!(
            "Default world '{}' not found in universe {} (loaded: {:?})",
            default_world,
            name,
            worlds.keys().collect::<Vec<_>>()
        );
    }

    Ok(LoadedUniverse {
        name,
        root,
        universe: universe_path,
        default_world,
        worlds,
    })
}

/// Load universe by directory (expects `universe.mudl` inside).
pub fn load_module(module_dir: impl AsRef<Path>) -> anyhow::Result<LoadedUniverse> {
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

fn load_world(world_dir: impl AsRef<Path>, expected_name: &str) -> anyhow::Result<LoadedWorld> {
    let world_dir = world_dir.as_ref();
    let entrypoint = world_dir.join("world.mudl");
    if !entrypoint.exists() {
        anyhow::bail!(
            "World entrypoint not found: {} (expected world.mudl)",
            entrypoint.display()
        );
    }

    let root = world_dir.canonicalize()?;
    let entrypoint = entrypoint.canonicalize()?;
    let content = fs::read_to_string(&entrypoint)?;
    let (world_name, starting_location) = parse_world_meta(&content);
    let name = world_name.unwrap_or_else(|| expected_name.to_string());

    let mut visited = HashSet::new();
    let sources = expand_includes(&entrypoint, &root, &mut visited)?;

    let mut anatomy = AnatomyRegistry::default();
    let mut world_defs = Vec::new();
    let mut resolved_start = starting_location;

    for source in &sources {
        let file_content = fs::read_to_string(source)?;
        anatomy.merge(parse_anatomy_file(&file_content)?);
        let (defs, start) = parse_world_file(&file_content);
        world_defs.extend(defs);
        if start.is_some() {
            resolved_start = start;
        }
    }

    if world_defs.is_empty() && resolved_start.is_none() {
        anyhow::bail!(
            "No world content found in world {} (checked {} files)",
            name,
            sources.len()
        );
    }

    Ok(LoadedWorld {
        name,
        root,
        entrypoint,
        sources,
        anatomy,
        world_defs,
        starting_location: resolved_start,
    })
}

fn parse_universe_meta(content: &str) -> (Option<String>, Option<String>) {
    parse_block_meta(content, "@universe", &["default_world"])
}

fn parse_world_meta(content: &str) -> (Option<String>, Option<String>) {
    let (name, fields) = parse_block_meta_fields(content, "@world", &["starting_location"]);
    (name, fields.get("starting_location").cloned())
}

fn parse_block_meta(
    content: &str,
    block_tag: &str,
    field_keys: &[&str],
) -> (Option<String>, Option<String>) {
    let (name, fields) = parse_block_meta_fields(content, block_tag, field_keys);
    let field = field_keys
        .first()
        .and_then(|key| fields.get(*key).cloned());
    (name, field)
}

fn parse_block_meta_fields(
    content: &str,
    block_tag: &str,
    field_keys: &[&str],
) -> (Option<String>, HashMap<String, String>) {
    let mut name = None;
    let mut fields = HashMap::new();
    let mut in_block = false;

    for line in content.lines() {
        let trimmed = line.split(';').next().unwrap_or(line).trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed == "@end" {
            if in_block {
                break;
            }
            continue;
        }
        if let Some(block_name) = trimmed.strip_prefix(block_tag).map(str::trim) {
            in_block = true;
            if !block_name.is_empty() {
                name = Some(block_name.to_string());
            }
            continue;
        }
        if in_block && trimmed.contains('=') {
            let parts: Vec<&str> = trimmed.splitn(2, '=').collect();
            if parts.len() == 2 {
                let key = parts[0].trim().to_string();
                let value = parts[1].trim().to_string();
                if field_keys.contains(&key.as_str()) || key == "default_world" {
                    fields.insert(key, value);
                }
            }
        }
    }

    (name, fields)
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
    fn load_default_universe_with_default_world() {
        let universe = load_module("modules/default").unwrap();
        assert_eq!(universe.name, "default");
        assert_eq!(universe.default_world, "default_world");
        assert!(universe.worlds.contains_key("default_world"));

        let world = universe.active_world().unwrap();
        assert_eq!(world.name, "default_world");
        assert!(world.anatomy.body_plan("human").is_some());
        assert!(world.anatomy.player_template("default").is_some());
        assert_eq!(world.starting_location.as_deref(), Some("the-void"));
        assert!(world.world_defs.iter().any(|d| d.base_name == "the-void"));
        assert!(world.sources.len() >= 4);
    }
}