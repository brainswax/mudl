# Default Universe

Official baseline universe for MUDL: one default world with flat MUDL files (no subfolders under the world for now).

## Layout

```
modules/default/
  universe.mudl
  worlds/
    default_world/
      world.mudl       # World entrypoint (@world, @include)
      map.mudl         # Areas and locations (type=area)
      creatures.mudl   # @creature definitions with anatomy slots
      players.mudl     # @player-template definitions (creature=human)
      items.mudl       # Item prototypes (future)
      objects.mudl     # Shared object prototypes (future)
```

## Customization

Copy this folder to `modules/my-universe/` and edit the flat `.mudl` files, or add another world under `worlds/`:

```bash
MUDL_MODULE=modules/my-universe cargo run --bin repl
MUDL_WORLD=default_world cargo run --bin repl
```

To add a new creature, define `@creature cat` in `creatures.mudl` and set `creature=cat` in a player template. Nested subfolders can be reintroduced later when content grows.

## Expansion packs

Self-contained adventures live in `worlds/default_world/expansions/<name>/` — each folder has `<name>.mudl` and `README.md`.

**Install from GitHub:**

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/haunted_forest/haunted_forest.mudl
```

- **Pack index:** [worlds/default_world/expansions/README.md](worlds/default_world/expansions/README.md)