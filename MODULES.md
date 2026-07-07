# Adventure Modules

Official expansion packs for Project MUDL. Each module is a folder with `<name>/<name>.mudl` and a self-contained README beside it.

## Packs

| Module | Folder |
|--------|--------|
| Haunted Forest | `haunted_forest/` |
| Poisonous Swamp | `poisonous_swamp/` |
| Giant Spider Den | `giant_spider_den/` |
| Sandy Shoals Resort | `sandy_shoals/` |
| Glimmerfen | `glimmerfen/` |

Docs live next to each `.mudl` file under `modules/default/worlds/default_world/expansions/<folder>/README.md`.

## Doc structure (every module README)

1. Theme teaser (no spoilers)
2. Quick install — one copy-paste block: `@import` URL, minimal host map, `cargo run`, `module reload`, `go`
3. Details — tone, what to expect, commands (no puzzle solutions)
4. Extension ideas (optional, builder-focused)

## Import URL pattern

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/<folder>/<folder>.mudl
```