# Default Module

Official baseline universe for MUDL: naked human anatomy, starter rooms, and default player template.

## Layout

```
modules/default/
  universe.mudl      # Entrypoint — @include other files
  config.mudl        # starting_location, module metadata
  anatomy/human.mudl # Body plan (grasp, wear, limb slots)
  players/default.mudl
  rooms/locations.mudl
  items/             # Standard items (future)
  objects/           # Shared object prototypes (future)
```

## Customization

Copy this folder to `modules/my-world/` (or under `examples/`) and edit:

- Change `players/default.mudl` to use a different `body_plan` (e.g. after adding `anatomy/cat.mudl`)
- Add rooms, items, and objects as separate `.mudl` files
- Reference them from your `universe.mudl`

Point the engine at your fork:

```bash
MUDL_MODULE=modules/my-world cargo run --bin repl
```

Prototype inheritance for objects and player templates is planned; modules are the packaging unit for now.