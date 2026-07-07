# Poisonous Swamp

A stinking sink where bitter fen, sweet reeds, and black water teach hard lessons. Green fog beads on your sleeves; leeches ripple without breaking the surface. The bog keeps what it steals — and something warden-sized waits where the peat runs deepest.

## Quick Install

Stand in any room and paste:

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/poisonous_swamp/poisonous_swamp.mudl
@create portal Swamp door_direction=down door_destination=swamp-entry
@link down swamp-entry --return up
```

Then `go down`.

## Detailed description

**Module:** `@expansion poisonous_swamp` · entry `swamp-entry` · portal `down` / return `up`

**Areas**

| base_name | Role |
|-----------|------|
| `swamp-entry` | Entry; north/east/south/west to route and wrong turns; `up` returns to host when integrated |
| `swamp-bitter` | Main route — bitter fen; east → `swamp-sweet` |
| `swamp-sweet` | Main route — sweet reeds; south → `swamp-dry` |
| `swamp-dry` | Dry rise; west → `swamp-heart`; `in` exit (`exit_returns`: `out`) |
| `swamp-heart` | Finale; `up` scatters (`scatter_to`: `forest-path`, `the-void`) |
| `swamp-gas`, `swamp-quicksand`, `swamp-wisps`, `swamp-snare`, `swamp-drown`, `swamp-shrine` | Wrong turns → `loop_to: swamp-entry` |

**Tone:** High danger. Environmental damage on harsh rooms, creature ambushes, antidote scarcity. Fixed warden NPC at the deep heart; kill grants `bog_resilience`.

**Features:** `@effect` reed_breathing, bog_resilience. Reed breather mask, reed-walker boots, antidote salves. Harvest bitter roots and sweet reed beds; breakable spore pods. Gas grate (readable) in gas pocket; heartwood coffer at deep heart. Gas wisps, bog leeches, mire crawlers via `@spawner`.

**Hidden:** Antidote cache at `swamp-sweet` (`hidden_until_discovered`).

**Puzzles:** Warning post and bog markers teach a taste vocabulary (BITTER, SWEET, DRY, DEEP). Safe routes and stake logic are in-game only.

**Commands:** `look`, `examine`, `read`, `go`, `take`, `harvest`, `attack`, `wear`.

## Extension ideas

- `@effect` mudwalking boots that soften `on_enter` damage in gas rooms.
- `@resource-spawner` on new harvest nodes for an alchemy crafting chain.
- Swap the warden for `react=warn` dialogue and a quest item instead of combat.
- `@trigger on_kill` on the warden that opens a gate in your own world.
- Extra `@schedule` on the gas pocket for louder weather every N room ticks.