use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use super::anatomy::{parse_anatomy_file, AnatomyRegistry};
use super::behavior_def::parse_behavior_file;
use super::item_def::{parse_item_file, ItemInstanceDef, ItemPrototypeDef};
use super::npc_def::parse_npc_file;
use super::loot_spawner_def::parse_loot_spawner_file;
use super::spawner_def::parse_spawner_file;
use super::world_def::{parse_world_file, WorldDef};
use crate::mudl::{LootSpawnerDef, LootTemplateDef, NpcDef, SpawnTemplateDef, SpawnerDef};

/// A loaded MUDL source — local file or remote URL fetched at load time.
#[derive(Debug, Clone)]
pub enum MudlSource {
    File(PathBuf),
    Remote { url: String, content: String },
}

impl MudlSource {
    pub fn label(&self) -> String {
        match self {
            MudlSource::File(path) => path.display().to_string(),
            MudlSource::Remote { url, .. } => url.clone(),
        }
    }

}

/// A loaded world: self-contained game setting (locations, anatomy, creatures, items).
#[derive(Debug, Clone)]
pub struct LoadedWorld {
    pub name: String,
    pub root: PathBuf,
    pub entrypoint: PathBuf,
    pub sources: Vec<MudlSource>,
    pub anatomy: AnatomyRegistry,
    pub world_defs: Vec<WorldDef>,
    pub item_prototypes: Vec<ItemPrototypeDef>,
    pub item_instances: Vec<ItemInstanceDef>,
    pub npc_defs: Vec<NpcDef>,
    pub spawn_template_defs: Vec<SpawnTemplateDef>,
    pub spawner_defs: Vec<SpawnerDef>,
    pub loot_template_defs: Vec<LootTemplateDef>,
    pub loot_spawner_defs: Vec<LootSpawnerDef>,
    pub behavior_template_defs: Vec<super::behavior_def::BehaviorTemplateDef>,
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
                for source in &world.sources {
                    if let MudlSource::File(path) = source {
                        sources.push(path.clone());
                    }
                }
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
            let world = load_world(&world_dir, world_name, &root)?;
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

fn load_world(
    world_dir: impl AsRef<Path>,
    expected_name: &str,
    universe_root: &Path,
) -> anyhow::Result<LoadedWorld> {
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

    let ctx = ImportContext {
        world_root: root.clone(),
        universe_root: universe_root.to_path_buf(),
    };
    let mut visited = HashSet::new();
    let sources = expand_sources(
        SourceRef::File(entrypoint.clone()),
        &ctx,
        &mut visited,
    )?;

    let mut anatomy = AnatomyRegistry::default();
    let mut world_defs = Vec::new();
    let mut item_prototypes = Vec::new();
    let mut item_instances = Vec::new();
    let mut npc_defs = Vec::new();
    let mut spawn_template_defs = Vec::new();
    let mut spawner_defs = Vec::new();
    let mut loot_template_defs = Vec::new();
    let mut loot_spawner_defs = Vec::new();
    let mut behavior_template_defs = Vec::new();
    let mut resolved_start = starting_location;

    for source in &sources {
        let file_content = read_source_content(source)?;
        anatomy.merge(parse_anatomy_file(&file_content)?);
        let (defs, start) = parse_world_file(&file_content);
        world_defs.extend(defs);
        let (protos, items) = parse_item_file(&file_content);
        item_prototypes.extend(protos);
        item_instances.extend(items);
        npc_defs.extend(parse_npc_file(&file_content));
        let (templates, spawners) = parse_spawner_file(&file_content);
        spawn_template_defs.extend(templates);
        spawner_defs.extend(spawners);
        let (loot_templates, loot_spawners) = parse_loot_spawner_file(&file_content);
        loot_template_defs.extend(loot_templates);
        loot_spawner_defs.extend(loot_spawners);
        behavior_template_defs.extend(parse_behavior_file(&file_content));
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
        item_prototypes,
        item_instances,
        npc_defs,
        spawn_template_defs,
        spawner_defs,
        loot_template_defs,
        loot_spawner_defs,
        behavior_template_defs,
        starting_location: resolved_start,
    })
}

fn read_source_content(source: &MudlSource) -> anyhow::Result<String> {
    match source {
        MudlSource::File(path) => Ok(fs::read_to_string(path)?),
        MudlSource::Remote { content, .. } => Ok(content.clone()),
    }
}

#[derive(Debug, Clone)]
struct ImportContext {
    world_root: PathBuf,
    universe_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum SourceKey {
    File(PathBuf),
    Url(String),
}

#[derive(Debug, Clone)]
enum SourceRef {
    File(PathBuf),
    Url(String),
}

#[derive(Debug, Clone)]
enum ImportBase {
    FileDir(PathBuf),
    Url(String),
}

fn expand_sources(
    source: SourceRef,
    ctx: &ImportContext,
    visited: &mut HashSet<SourceKey>,
) -> anyhow::Result<Vec<MudlSource>> {
    let (base, content, output) = match &source {
        SourceRef::File(path) => {
            let canonical = path.canonicalize()?;
            if !visited.insert(SourceKey::File(canonical.clone())) {
                anyhow::bail!("Circular @include/@import detected: {}", path.display());
            }
            let parent = canonical
                .parent()
                .ok_or_else(|| anyhow::anyhow!("source path has no parent: {}", path.display()))?
                .to_path_buf();
            let content = fs::read_to_string(&canonical)?;
            (
                ImportBase::FileDir(parent),
                content,
                MudlSource::File(canonical),
            )
        }
        SourceRef::Url(url) => {
            if !visited.insert(SourceKey::Url(url.clone())) {
                anyhow::bail!("Circular @import detected: {url}");
            }
            let content = fetch_url(url)?;
            (
                ImportBase::Url(url.clone()),
                content.clone(),
                MudlSource::Remote {
                    url: url.clone(),
                    content,
                },
            )
        }
    };

    let mut files = Vec::new();
    expand_from_content(&content, base, ctx, visited, &mut files)?;
    files.push(output);
    Ok(files)
}

fn expand_from_content(
    content: &str,
    base: ImportBase,
    ctx: &ImportContext,
    visited: &mut HashSet<SourceKey>,
    out: &mut Vec<MudlSource>,
) -> anyhow::Result<()> {
    for line in content.lines() {
        let trimmed = line.split(';').next().unwrap_or(line).trim();
        if let Some(include_path) = trimmed.strip_prefix("@include ") {
            let target = ctx.world_root.join(include_path.trim());
            if !target.exists() {
                anyhow::bail!(
                    "Included file not found: {} (expected relative to world root {})",
                    target.display(),
                    ctx.world_root.display()
                );
            }
            out.extend(expand_sources(
                SourceRef::File(target),
                ctx,
                visited,
            )?);
            continue;
        }
        if let Some(import_spec) = trimmed.strip_prefix("@import ") {
            let import_spec = import_spec.trim();
            if import_spec.is_empty() {
                continue;
            }
            let resolved = resolve_import(import_spec, &base, ctx)?;
            out.extend(expand_sources(resolved, ctx, visited)?);
        }
    }
    Ok(())
}

fn resolve_import(
    spec: &str,
    base: &ImportBase,
    ctx: &ImportContext,
) -> anyhow::Result<SourceRef> {
    if is_url(spec) {
        return Ok(SourceRef::Url(spec.to_string()));
    }

    if let Some(file_path) = spec.strip_prefix("file://") {
        let path = PathBuf::from(file_path);
        if !path.exists() {
            anyhow::bail!("Imported file not found: {}", path.display());
        }
        return Ok(SourceRef::File(path));
    }

    let path = Path::new(spec);
    if path.is_absolute() {
        if !path.exists() {
            anyhow::bail!("Imported file not found: {}", path.display());
        }
        return Ok(SourceRef::File(path.to_path_buf()));
    }

    if let ImportBase::Url(url) = base {
        if let Some(resolved) = resolve_url_relative(url, spec) {
            return Ok(SourceRef::Url(resolved));
        }
    }

    let candidates = match base {
        ImportBase::FileDir(dir) => {
            let mut list = vec![dir.join(spec)];
            list.push(ctx.world_root.join(spec));
            list.push(ctx.universe_root.join(spec));
            list
        }
        ImportBase::Url(_) => {
            vec![
                ctx.world_root.join(spec),
                ctx.universe_root.join(spec),
            ]
        }
    };

    for candidate in candidates {
        if candidate.exists() {
            return Ok(SourceRef::File(candidate));
        }
    }

    anyhow::bail!(
        "Imported file not found: {spec} (searched relative to import base, world root, and universe root)"
    )
}

fn is_url(spec: &str) -> bool {
    spec.starts_with("http://") || spec.starts_with("https://")
}

fn resolve_url_relative(base_url: &str, relative: &str) -> Option<String> {
    if is_url(relative) {
        return Some(relative.to_string());
    }

    let base = url::parse_base(base_url)?;
    let mut segments: Vec<&str> = base
        .path
        .trim_end_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    if !base.path.ends_with('/') && !segments.is_empty() {
        segments.pop();
    }

    for component in Path::new(relative).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                segments.pop();
            }
            Component::Normal(part) => segments.push(part.to_str()?),
            _ => return None,
        }
    }

    let path = if segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", segments.join("/"))
    };

    Some(format!("{}://{}{}", base.scheme, base.host, path))
}

#[derive(Debug)]
struct ParsedBaseUrl {
    scheme: String,
    host: String,
    path: String,
}

mod url {
    use super::ParsedBaseUrl;

    pub fn parse_base(url: &str) -> Option<ParsedBaseUrl> {
        let (scheme, rest) = url.split_once("://")?;
        let (host, path) = match rest.find('/') {
            Some(idx) => (&rest[..idx], &rest[idx..]),
            None => (rest, "/"),
        };
        if host.is_empty() {
            return None;
        }
        Some(ParsedBaseUrl {
            scheme: scheme.to_string(),
            host: host.to_string(),
            path: path.to_string(),
        })
    }
}

fn fetch_url(url: &str) -> anyhow::Result<String> {
    let response = ureq::get(url).call().map_err(|err| match err {
        ureq::Error::Status(code, resp) => {
            anyhow::anyhow!("HTTP {code} fetching {url}: {}", resp.status_text())
        }
        other => anyhow::anyhow!("Failed to fetch {url}: {other}"),
    })?;
    response
        .into_string()
        .map_err(|err| anyhow::anyhow!("Failed to read response from {url}: {err}"))
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
    let field = field_keys.first().and_then(|key| fields.get(*key).cloned());
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
        assert!(world.anatomy.creature("human").is_some());
        assert_eq!(
            world
                .anatomy
                .player_template("default")
                .map(|t| t.creature.as_str()),
            Some("human")
        );
        assert_eq!(world.starting_location.as_deref(), Some("the-void"));
        let void = world
            .world_defs
            .iter()
            .find(|d| d.base_name == "the-void")
            .unwrap();
        assert_eq!(void.obj_type, "area");
        assert!(
            world
                .world_defs
                .iter()
                .any(|d| d.base_name == "forest-path")
        );
        assert!(
            world
                .world_defs
                .iter()
                .any(|d| d.base_name == "cottage-interior")
        );
        assert!(world.sources.len() >= 5);
        assert!(!world.item_prototypes.is_empty());
        assert!(!world.item_instances.is_empty());
        assert!(
            world
                .item_instances
                .iter()
                .any(|i| i.base_name == "scene-mailbox")
        );
        assert!(
            world
                .world_defs
                .iter()
                .any(|d| d.base_name == "haunted-entry"),
            "haunted forest expansion should load via @import"
        );
        assert!(
            world
                .item_instances
                .iter()
                .any(|i| i.base_name == "forest-hollow-oak")
        );
    }

    #[test]
    fn import_resolves_relative_to_world_root() {
        let universe = load_module("modules/default").unwrap();
        let world = universe.active_world().unwrap();
        assert!(
            world
                .sources
                .iter()
                .any(|s| matches!(s, MudlSource::File(p) if p.ends_with("expansions/haunted_forest.mudl")))
        );
    }

    #[test]
    fn import_file_url_loads_expansion() {
        let expansion = PathBuf::from("modules/default/worlds/default_world/expansions/haunted_forest.mudl")
            .canonicalize()
            .unwrap();
        let url = format!("file://{}", expansion.display());
        let ctx = ImportContext {
            world_root: expansion.parent().unwrap().parent().unwrap().to_path_buf(),
            universe_root: PathBuf::from("modules/default"),
        };
        let resolved = resolve_import(&url, &ImportBase::FileDir(ctx.world_root.clone()), &ctx)
            .unwrap();
        match resolved {
            SourceRef::File(path) => {
                let content = fs::read_to_string(path).unwrap();
                let (defs, _) = parse_world_file(&content);
                assert!(defs.iter().any(|d| d.base_name == "haunted-heart"));
                let (_, items) = parse_item_file(&content);
                assert!(items.iter().any(|i| i.base_name == "forest-hollow-oak"));
            }
            other => panic!("expected file import, got {other:?}"),
        }
    }

    #[test]
    fn resolve_url_relative_paths() {
        let base = "https://example.com/mudl/worlds/default/expansions/pack.mudl";
        assert_eq!(
            resolve_url_relative(base, "sibling.mudl").as_deref(),
            Some("https://example.com/mudl/worlds/default/expansions/sibling.mudl")
        );
        assert_eq!(
            resolve_url_relative(base, "../shared/items.mudl").as_deref(),
            Some("https://example.com/mudl/worlds/default/shared/items.mudl")
        );
    }

    #[test]
    fn circular_import_is_detected() {
        let dir = std::env::temp_dir().join(format!("mudl-import-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.mudl"), "@import b.mudl\n").unwrap();
        fs::write(dir.join("b.mudl"), "@import a.mudl\n").unwrap();

        let ctx = ImportContext {
            world_root: dir.clone(),
            universe_root: dir.clone(),
        };
        let mut visited = HashSet::new();
        let err = expand_sources(SourceRef::File(dir.join("a.mudl")), &ctx, &mut visited).unwrap_err();
        assert!(err.to_string().contains("Circular"));
    }
}