# Sandy Shoals Resort

Whimsical tropical resort — tiki bar, tidepools, striped hammocks, and a pearl inlet where the surf applauds politely. Restorative, sunny, slightly silly. The house special is confidence on the rocks; the worst injury is ego.

## Quick Install

Paste the `@import` line into `world.mudl`, the room into `map.mudl`, set `starting_location=start`, then run:

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/sandy_shoals/sandy_shoals.mudl

type: area
base_name: start
name: Start
description: A painted arrow points south along sand.
exits:
  south: beach-trail
exit_returns:
  south: north
```

```bash
cargo run --bin repl
```

```text
module reload
go south
```

## Details

**Tone:** No combat. Pressure is optional cocktail chaos and gentle navigation loops.

**Inside the resort:** Sandy trail into a gate with scenic dead ends, taste-themed way markers, and shore/veranda/bar/tidepool/inlet rooms with healing or buff triggers on entry. Stackable cocktails grant layered drunk effects that shift dexterity and charisma; NPCs react differently when you are unsteady. Tiki bartender, hammock attendant, and pier hermit — friendly only. Harvestable shells and tidepool ledges, hidden bottle and cache finds, message in a bottle, and pearl coffer with resort gear.

**Commands:** `look`, `examine`, `read`, `go`, `take`, `harvest`, `open`, `inventory`.

## Extension ideas

- `@schedule` sunset `on_weather` on the open shore for time-of-day flavor.
- Mocktail `@prototype` items with custom `@effect` buffs instead of drunk stacks.
- `@npc` surf instructor granting `mod-skill survival` on `on_enter`.
- Retarget pearl inlet `scatter_to` to your coastal city `base_name`.
- `@resource-spawner` on a new beach harvest node for event-only souvenirs.