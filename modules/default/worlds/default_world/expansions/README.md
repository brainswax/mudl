# Expansion packs

Each adventure is a single `.mudl` file with a **self-contained README** (teaser, install, what to expect, extensions). Pick a module below — you do not need any other pack to install or play it.

## GitHub URL pattern

```
https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/<file>.mudl
```

Import in `world.mudl`, restart the REPL (or `module reload`), then follow that module's README for map hooks, `@link`, `@create` portal, and `go` commands.

## Modules

| Module | File | Documentation |
|--------|------|----------------|
| Haunted Forest | [haunted_forest.mudl](haunted_forest.mudl) | [haunted_forest/README.md](haunted_forest/README.md) |
| Poisonous Swamp | [poisonous_swamp.mudl](poisonous_swamp.mudl) | [poisonous_swamp/README.md](poisonous_swamp/README.md) |
| Giant Spider Den | [giant_spider_den.mudl](giant_spider_den.mudl) | [giant_spider_den/README.md](giant_spider_den/README.md) |
| Sandy Shoals Resort | [beach_resort.mudl](beach_resort.mudl) | [beach_resort/README.md](beach_resort/README.md) |
| Glimmerfen | [fey_glade.mudl](fey_glade.mudl) | [fey_glade/README.md](fey_glade/README.md) |

The stock `default_world` loads all of the above from local paths in [world.mudl](../world.mudl).