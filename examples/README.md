# Example Modules

Place alternative universe packs here (e.g. `examples/fantasy/`) using the same layout as `modules/default/`:

```
examples/my-world/
  universe.mudl
  config.mudl
  anatomy/
  rooms/
  players/
  items/
  objects/
```

Point the engine at your module with:

```bash
MUDL_MODULE=examples/my-world cargo run --bin repl
```

Or set `MUDL_UNIVERSE` to a specific `universe.mudl` path.