# Default Universe

Official baseline universe for MUDL: one default world with naked human anatomy, starter rooms, and a default player template.

## Layout

```
modules/default/
  universe.mudl                    # Universe entrypoint — @include-world directives
  worlds/
    default_world/
      world.mudl                   # World entrypoint (starting_location, @include)
      anatomy/human.mudl           # Body plan (grasp, wear, limb slots)
      players/default.mudl         # Player spawn template
      locations/
        world_locations.mudl       # Room definitions
        rooms/                     # Per-room files (future)
        areas/                     # Area groupings (future)
      creatures/                   # NPC/creature templates (future)
      items/                       # Standard items (future)
      objects/                     # Shared object prototypes (future)
```

## Customization

Copy this folder to `modules/my-universe/` (or under `examples/`) and edit:

- Add a new world under `worlds/my_world/` with its own `world.mudl`
- Reference it from `universe.mudl` with `@include-world my_world`
- Override anatomy, locations, or player templates per world
- Set `default_world=my_world` in the `@universe` block

Point the engine at your fork:

```bash
MUDL_MODULE=modules/my-universe cargo run --bin repl
```

Select a specific world within a universe:

```bash
MUDL_WORLD=my_world cargo run --bin repl
```

Custom worlds can inherit from the default by `@include`ing shared anatomy or location files from another world path, or by copying and overriding individual `.mudl` files.