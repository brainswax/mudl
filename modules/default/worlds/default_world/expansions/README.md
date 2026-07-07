# Expansion packs

Each adventure lives in its own folder with a `.mudl` file and `README.md`:

```
expansions/
  haunted_forest/
    haunted_forest.mudl
    README.md
  sandy_shoals/
    sandy_shoals.mudl
    README.md
  …
```

Open a module's **README** for teaser, install, what to expect, and extensions (self-contained, no spoilers).

## GitHub URL pattern

```
https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/<folder>/<folder>.mudl
```

Example:

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/haunted_forest/haunted_forest.mudl
```

Local path (same folder/file name):

```mudl
@import expansions/haunted_forest/haunted_forest.mudl
```

Restart the REPL or run `module reload` after editing `world.mudl`.

## Modules

| Module | Folder | Documentation |
|--------|--------|----------------|
| Haunted Forest | `haunted_forest/` | [README](haunted_forest/README.md) |
| Poisonous Swamp | `poisonous_swamp/` | [README](poisonous_swamp/README.md) |
| Giant Spider Den | `giant_spider_den/` | [README](giant_spider_den/README.md) |
| Sandy Shoals Resort | `sandy_shoals/` | [README](sandy_shoals/README.md) |
| Glimmerfen | `glimmerfen/` | [README](glimmerfen/README.md) |

The stock `default_world` loads all packs from [world.mudl](../world.mudl).