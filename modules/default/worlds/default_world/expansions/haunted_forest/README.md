# Haunted Forest

Silver mist, moonlit glades, and a wood that remembers those who listen. Gothic folktale beauty — uneasy, not loud. Wrong paths feel almost polite; something watches from between the trees. Bring a light, patience, and time to read what the forest left behind.

## Quick Install

Paste the `@import` line into `world.mudl`, the two rooms into `map.mudl`, set `starting_location=start`, then run:

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/haunted_forest/haunted_forest.mudl

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
description: An ancient split oak hunches beside the trail.
exits:
  south: start
  in: haunted-entry
exit_returns:
  south: north
  in: out
```

```bash
cargo run --bin repl
```

```text
module reload
go north
go in
```

## Details

**Tone:** Medium danger. Navigation tension, ambient phantoms, and lurkers that may attack if you blunder in unprepared. Stealth and survival help.

**Inside the wood:** A threshold where many paths look equally inviting, way markers whose wording rewards careful reading, and themed regions (moonlit, ember, mirror, ash) with weather and respawn schedules. Locked hollow oak portal, breakable pots, harvestable moss, hidden supply cache, shrine offering, and a reward chest at the deep heart. Wisps and lurkers spawn on enter and on periodic ticks.

**Commands:** `look`, `examine`, `read`, `go`, `take`, `harvest`, `attack`, `open`, `unlock`.

## Extension ideas

- `@schedule` extra `on_weather` lines on mist rooms for seasonal horror.
- `@create item` a second hollow oak on another host path with a new `door_direction`.
- `@spawn-template` friendly ghost with `react=ignore` between scares.
- `@loot-spawner` entries on the heart chest for campaign artifacts.
- `@trigger on_discovered` on your own hidden caches for lore-only drops.