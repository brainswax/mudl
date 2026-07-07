# Poisonous Swamp

A stinking sink where bitter fen, sweet reeds, and black water teach hard lessons. Green fog beads on your sleeves; leeches ripple without breaking the surface. The bog keeps what it steals — and something warden-sized waits where the peat runs deepest.

## Quick Install

Paste the `@import` line into `world.mudl`, the two rooms into `map.mudl`, set `starting_location=start`, then run:

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/poisonous_swamp/poisonous_swamp.mudl

type: area
base_name: start
name: Start
exits:
  north: forest-path
exit_returns:
  north: south

type: area
base_name: forest-path
name: Forest Path
description: The ground sounds hollow toward the east.
exits:
  south: start
  down: swamp-entry
exit_returns:
  south: north
  down: up
```

```bash
cargo run --bin repl
```

```text
module reload
go north
go down
```

## Details

**Tone:** High danger. Environmental damage on harsh rooms, creature ambushes, antidote scarcity, and a fixed warden NPC at the deep heart. Defeating the warden grants a lasting resilience effect.

**Inside the bog:** A threshold where wrong paths loop back, carved stakes and a warning post, and bitter/sweet/dry/deep themed regions with gas pockets, quicksand, and snares. Reed breather mask and bog-walker boots as wearable rewards; antidote salves from harvests and breakables. Gas wisps, bog leeches, and mire crawlers spawn throughout.

**Commands:** `look`, `examine`, `read`, `go`, `take`, `harvest`, `attack`, `wear`.

## Extension ideas

- `@effect` mudwalking boots that soften `on_enter` damage in gas rooms.
- `@resource-spawner` on new harvest nodes for an alchemy crafting chain.
- Swap the warden for `react=warn` dialogue and a quest item instead of combat.
- `@trigger on_kill` on the warden that opens a gate in your own world.
- Extra `@schedule` on the gas pocket for louder weather every N room ticks.