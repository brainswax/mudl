# Example Universes

Place alternative universe packs here (e.g. `examples/fantasy/`) using the same layout as `modules/default/`:

```
examples/my-universe/
  universe.mudl
  worlds/
    my_world/
      world.mudl
      anatomy/
      locations/
      players/
      creatures/
      items/
      objects/
```

Point the engine at your universe with:

```bash
MUDL_MODULE=examples/my-universe cargo run --bin repl
```

Select a world within the universe:

```bash
MUDL_WORLD=my_world cargo run --bin repl
```

Or set `MUDL_UNIVERSE` to a specific `universe.mudl` path.