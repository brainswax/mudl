# Expansion packs

Each adventure is a folder with `<name>.mudl` and `README.md`. Open the README beside the pack for a self-contained guide: theme teaser, quick install, details, and extension ideas.

## GitHub import URL

```
https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/<folder>/<folder>.mudl
```

Local path (same folder/file name):

```mudl
@import expansions/<folder>/<folder>.mudl
```

After editing `world.mudl` or `map.mudl`, restart the REPL or run `module reload`.

## Packs

| Pack | Folder |
|------|--------|
| Haunted Forest | `haunted_forest/` |
| Poisonous Swamp | `poisonous_swamp/` |
| Giant Spider Den | `giant_spider_den/` |
| Sandy Shoals Resort | `sandy_shoals/` |
| Glimmerfen | `glimmerfen/` |