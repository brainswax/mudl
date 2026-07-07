# Glimmerfen

A pristine fey vale — silver grass, singing reeds, glowcap counsel, and starlight that pools without rushing. Elves, gnomes, and pixies attend without fuss. Wonder first; the challenge is noticing what the vale left for the gentle-footed.

## Quick Install

Paste the `@import` line into `world.mudl`, the two rooms into `map.mudl`, set `starting_location=start`, then run:

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/glimmerfen/glimmerfen.mudl

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
description: Pale mushroom caps ring the trail in a perfect circle.
exits:
  south: start
exit_returns:
  south: north
```

```bash
cargo run --bin repl
```

```text
module reload
go north
go through
```

## Details

**Tone:** Restorative. No attack behaviors; ambient fey spawns emote and wander.

**Inside the vale:** A threshold with gentle wrong turns, hush-themed navigation via runestone markers, and glades that grant magical buffs and healing on entry. Harvest moonpetals, reeds, glowcaps, and starroots for teas, charms, and wearables. Fairy guide, gnome gardener, and elder elf sage offer greetings and light aid. Leaving from the grace pool scatters you back to familiar ground.

**Commands:** `look`, `examine`, `read`, `go`, `take`, `harvest`, `inventory`, `wear`.

## Extension ideas

- `@schedule` seasonal `@trigger on_weather` blessings on the grace pool.
- `@spawn-template` unicorn with `fey_wanderer` for rare `on_enter` sightings.
- `@create item` a second portal with `prototype=moonmist-arch` and `door_direction=mist` for a beach entrance.
- `when skill stealth at_least N` triggers on your own fairy rings.
- `@effect` festival_merriment toggled by wizard `@set` on the threshold for live events.