# Sandy Shoals Resort

Whimsical tropical resort — tiki bar, tidepools, striped hammocks, and a pearl inlet where the surf applauds politely. Restorative, sunny, slightly silly. The house special is confidence on the rocks; the worst injury is ego.

## Quick Install

Stand in any room and paste:

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/sandy_shoals/sandy_shoals.mudl
@create portal Shoals door_direction=south door_destination=beach-trail
@link south beach-trail --return north
```

Then `go south`.

## Details

**Entry:** `beach-trail` → `beach-gate` — four outward paths, some scenic dead ends that loop back with humor. Taste-themed way markers (salt, sun, sip, sand). Rooms include open shore, sunbed veranda, tiki bar, tidepool shelf, and pearl inlet with healing or buff triggers on entry.

**Tone:** No combat. Optional cocktail chaos and gentle navigation loops.

**Notable features:** Stackable cocktails grant layered drunk effects (tipsy → buzz → three sheets); NPCs react differently when unsteady. Tiki bartender, hammock attendant, pier hermit — friendly only. Harvestable shells and tidepool ledges, hidden bottle and cache finds, message in a bottle, pearl coffer with resort gear. `out` from the pearl inlet scatters to clearing, forest path, or sandy trail.

**Commands:** `look`, `examine`, `read`, `go`, `take`, `harvest`, `open`, `inventory`.

## Extension ideas

- `@schedule` sunset `on_weather` on the open shore for time-of-day flavor.
- Mocktail `@prototype` items with custom `@effect` buffs instead of drunk stacks.
- `@npc` surf instructor granting `mod-skill survival` on `on_enter`.
- Retarget pearl inlet `scatter_to` to your coastal city `base_name`.
- `@resource-spawner` on a new beach harvest node for event-only souvenirs.