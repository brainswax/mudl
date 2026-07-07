# Example Universes

Alternative universe packs live here using the same layout as `modules/default/`. See [MODULES.md](../MODULES.md) and [LANGUAGE.md](../LANGUAGE.md) for MUDL syntax.

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