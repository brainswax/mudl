# Default Universe

Official baseline universe for MUDL: one default world with flat MUDL files and optional expansion packs.

## Layout

```
modules/default/
  universe.mudl
  worlds/
    default_world/
      world.mudl         # @world entrypoint, @import expansions, @include content
      map.mudl           # Areas and exits (type: area)
      creatures.mudl     # @creature, @effect, stats and skills
      behaviors.mudl     # @behavior-template AI personalities
      npcs.mudl          # @npc, @spawner, @loot-spawner
      players.mudl       # @player-template (creature=human)
      items.mudl         # Scene items and prototypes
      objects.mudl       # Shared prototypes
      expansions/        # Five drop-in adventure packs (each: <name>.mudl + README.md)
```

`world.mudl` already `@import`s all five expansion packs. To add one to a custom world without editing files, use that pack's **Quick Install** block from its README (paste in IRC or REPL).

## Play

```bash
cargo run --bin repl
# or
MUDL_MODULE=modules/default MUDL_WORLD=default_world cargo run --bin repl
```

You spawn in **The Void** as a naked human. Type `help` for commands; see [docs/REPL.md](../../docs/REPL.md).

## Customize

Copy this folder to `modules/my-universe/` and edit the `.mudl` files, or add another world under `worlds/`:

```bash
MUDL_MODULE=modules/my-universe cargo run --bin repl
MUDL_WORLD=my_world cargo run --bin repl
```

To add a new playable species, define `@creature cat` in `creatures.mudl` and set `creature=cat` in a `@player-template`. Reload with `module reload` in the REPL.

## Expansion packs

| Pack | Folder | Entry room |
|------|--------|------------|
| Haunted Forest | `expansions/haunted_forest/` | `haunted-entry` |
| Poisonous Swamp | `expansions/poisonous_swamp/` | `swamp-entry` |
| Giant Spider Den | `expansions/giant_spider_den/` | `spider-entry` |
| Sandy Shoals Resort | `expansions/sandy_shoals/` | `beach-trail` |
| Glimmerfen | `expansions/glimmerfen/` | `fey-threshold` |

Authoring rules and README template: [MODULES.md](../../MODULES.md). Pack index: [expansions/README.md](worlds/default_world/expansions/README.md).