# Example Universes

Place alternative universe packs here using the same flat layout as `modules/default/`:

```
examples/my-universe/
  universe.mudl
  worlds/
    my_world/
      world.mudl
      map.mudl
      creatures.mudl
      players.mudl
      items.mudl
      objects.mudl
```

```bash
MUDL_MODULE=examples/my-universe cargo run --bin repl
MUDL_WORLD=my_world cargo run --bin repl
```