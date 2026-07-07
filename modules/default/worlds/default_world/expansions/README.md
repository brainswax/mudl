# Expansion packs

Self-contained adventure modules for Project MUDL. Each pack is a single `.mudl` file you can copy locally or load directly from GitHub.

## Load from GitHub (recommended)

Official packs live on the `main` branch:

```
https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/<file>.mudl
```

Add one line to your `world.mudl`:

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/haunted_forest.mudl
```

Restart the REPL or run `module reload` after editing `world.mudl`. The loader fetches the file at bootstrap time (network required for URL imports).

## Full quick-start (custom world)

Minimal example: drop **Haunted Forest** into a world with a forest path and portal.

### 1. `world.mudl`

```mudl
@world my_world
  starting_location=my-clearing
@end
@include map.mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/haunted_forest.mudl
```

### 2. `map.mudl` — hub + link to module entry

```mudl
type: area
base_name: my-clearing
name: My Clearing
description: A quiet staging ground. A worn path leads north into the trees.
exits:
  north: forest-path
exit_returns:
  north: south

type: area
base_name: forest-path
name: Forest Path
description: Pine needles soften your steps. An ancient split oak hunches beside the trail.
exits:
  south: my-clearing
  in: haunted-entry
exit_aliases:
  n: north
  s: south
exit_returns:
  south: north
  in: out
```

The expansion defines `haunted-entry` and places the **hollow oak** portal on `forest-path` automatically. The map exit `in: haunted-entry` lets players use `go in` once the oak is open.

### 3. Optional host objects (full default-world integration)

Some packs place clues or keys in named host locations. Match these `base_name` values or edit the pack's `@item` blocks:

| Pack | Host locations (`integrates=`) |
|------|--------------------------------|
| Haunted Forest | `forest-path`, `the-void`, `scene-chest` |
| Poisonous Swamp | `forest-path`, `the-void` |
| Giant Spider Den | `swamp-dry`, `forest-path`, `the-void` |
| Sandy Shoals Resort | `the-void`, `forest-path` |
| Glimmerfen | `forest-path`, `beach-shore`, `the-void`, `beach-trail` |

### 4. Portal without editing the map (wizard)

Stand in the host room in the REPL, then link an exit to the module's entry `base_name`:

```text
> go forest-path
> @link in haunted-entry --return out
Linked in → haunted-entry (reciprocal out).
```

Or place a portal object (after `@import`, prototypes from the pack exist):

```mudl
@item my-forest-portal
  prototype=hollow-oak-portal
  location=forest-path
  door_direction=in
  door_destination=haunted-entry
@end
```

Add that block to a local `integration.mudl` and `@include integration.mudl` from `world.mudl`.

### 5. Run and play

```bash
MUDL_MODULE=modules/my-universe cargo run --bin repl
```

```text
> look
> go north
> look
> examine split oak
> go in
> look
```

**Useful commands:** `look` / `l`, `examine` / `x`, `read <object>`, `go <direction>`, `take`, `harvest <object>`, `inventory` / `i`, `attack <creature>` (combat modules only).

---

## Module index

| Module | File | Doc |
|--------|------|-----|
| Haunted Forest | [haunted_forest.mudl](haunted_forest.mudl) | [haunted_forest/README.md](haunted_forest/README.md) |
| Poisonous Swamp | [poisonous_swamp.mudl](poisonous_swamp.mudl) | [poisonous_swamp/README.md](poisonous_swamp/README.md) |
| Giant Spider Den | [giant_spider_den.mudl](giant_spider_den.mudl) | [giant_spider_den/README.md](giant_spider_den/README.md) |
| Sandy Shoals Resort | [beach_resort.mudl](beach_resort.mudl) | [beach_resort/README.md](beach_resort/README.md) |
| Glimmerfen | [fey_glade.mudl](fey_glade.mudl) | [fey_glade/README.md](fey_glade/README.md) |

The stock **default world** already `@import`s all of the above from local paths in [world.mudl](../world.mudl).

Spoiler-free overview and suggested play order: [MODULES.md](../../../../../MODULES.md) (repo root).