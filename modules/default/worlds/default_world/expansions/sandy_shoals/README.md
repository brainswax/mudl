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

## Detailed description

**Module:** `@expansion beach_resort` (Sandy Shoals Resort) · entry `beach-trail` · portal `south` / return `north` · auto-hooks `the-void`, `forest-path`

**Areas**

| base_name | Role |
|-----------|------|
| `beach-trail` | Entry path; south → `beach-gate`; `north` (`exit_returns`: `south`) |
| `beach-gate` | Resort hub; east/south/west/northeast to shore, cabanas, pier, dunes |
| `beach-shore` | Open shore; east → `beach-sunbeds` |
| `beach-sunbeds` | Sunbed veranda; south → `beach-bar` |
| `beach-bar` | Tiki bar; west → `beach-tidepool` |
| `beach-tidepool` | Tidepool shelf; north → `beach-pearl` |
| `beach-pearl` | Finale; `out` scatters (`scatter_to`: `the-void`, `forest-path`, `beach-trail`) |
| `beach-dunes`, `beach-pier`, `beach-jetty`, `beach-shallows`, `beach-cabanas`, `beach-shrine` | Scenic wrong turns → `loop_to: beach-gate` |

**Tone:** No combat. Optional cocktail chaos and gentle navigation loops.

**Features:** `@effect` tipsy, beach_buzz, three_sheets, sun_kissed, resort_serenity, tidepool_clarity. Stackable cocktails (`on_take` drunk stacks). NPCs: tiki bartender, hammock attendant, pier hermit — greet only. Harvestable shell clusters and tidepool ledges; pearl coffer with resort gear. Message in a bottle at shell shrine. Weather schedules on shore and tidepool; bar respawn garnish narrative.

**Hidden:** Buried bottle at `beach-shore`; tidepool cache at `beach-tidepool` (`hidden_until_discovered`).

**Puzzles:** Resort trail sign and driftwood markers use a taste vocabulary (SALT, SUN, SIP, SAND). Sequence is learned in play.

**Commands:** `look`, `examine`, `read`, `go`, `take`, `harvest`, `open`, `inventory`.

## Extension ideas

- `@schedule` sunset `on_weather` on the open shore for time-of-day flavor.
- Mocktail `@prototype` items with custom `@effect` buffs instead of drunk stacks.
- `@npc` surf instructor granting `mod-skill survival` on `on_enter`.
- Retarget pearl inlet `scatter_to` to your coastal city `base_name`.
- `@resource-spawner` on a new beach harvest node for event-only souvenirs.