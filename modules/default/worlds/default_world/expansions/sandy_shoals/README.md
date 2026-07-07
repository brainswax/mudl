# Sandy Shoals Resort

**Pack:** `sandy_shoals/sandy_shoals.mudl` · **ID:** `beach_resort` · **Entry:** `beach-trail` (via `beach-gate`)

---

## 1. Theme teaser

Whimsical tropical resort — tiki bar, tidepools, striped hammocks, and a pearl inlet where the surf applauds politely. Restorative, sunny, slightly silly. The house special is confidence on the rocks; the worst injury is ego.

---

## 2. Quick install

### Import (GitHub)

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/sandy_shoals/sandy_shoals.mudl
```

### Minimal host map

The pack places a **resort trail sign** on `the-void`. Any hub with that name works, or edit the sign's `@item location=` in a fork.

```mudl
type: area
base_name: the-void
name: West Clearing
description: A clearing at the edge of nowhere. A painted arrow points south along sand.
exits:
  south: beach-trail
exit_returns:
  south: north
```

### Run

```bash
cargo run --bin repl
```

### Link (wizard, in-game)

From your hub (rename as needed):

```text
> go the-void
> @link south beach-trail --return north
```

No portal object required — a sandy path exit is enough.

### Play

```text
> look
> read sign
> go south
> look
> examine marker
> go east
> take spritz
> go south
```

---

## 3. What to expect

**Tone & danger:** None. No combat spawns; pressure is optional cocktail chaos and gentle navigation loops.

**The resort:**

- **Sandy trail** into a **gate** with four outward paths — some are scenic dead ends that loop to the gate with humor.
- **Way markers** along a taste-themed route (the clearing sign sets vocabulary; you supply the footsteps).
- **Open shore**, **sunbed veranda**, **tiki bar**, **tidepool shelf**, and **pearl inlet** — each with healing or buff triggers on entry.
- **Weather schedules** on shore and tidepool; **bar respawn** refreshes garnish narrative.

**Drunk mechanism:**

- Stackable **cocktails** (`on_take` grants layered effects: tipsy → buzz → three sheets).
- Dexterity and charisma shift; **bar, bartender, and hidden finds** react differently when you are unsteady.
- NPC lines change with your stats — revisit rooms after a drink.

**NPCs (friendly only):**

- **Tiki bartender**, **hammock attendant**, **pier hermit** — greet or idle behaviors, healing lines, no attacks.

**Objects & interactions:**

- **Harvestable** shell clusters and tidepool ledges — conches, pearls, salves, charms.
- **Hidden** buried bottle on the shore and **tidepool cache** under the ledge (perception + drunk-aware narration).
- **Message in a bottle** at the shell shrine — readable, openable, with loot inside.
- **Pearl coffer** at the inlet — weighted resort gear (swimsuit, sandals, jewelry, map, salves).
- **Hammocks**, cabanas, pier, jetty, shallows — flavor and wrong turns.

**Leaving:**

- **Out** from the pearl inlet scatters to clearing, forest path, or sandy trail.

**Commands to know:** `look`, `examine`, `read`, `go`, `take`, `harvest`, `open`, `inventory`.

*Resort route order and coffer contents are discoverable in play — this doc does not chart the path.*

---

## 4. Extension ideas

- `@schedule` sunset `on_weather` on the open shore for time-of-day flavor.
- Mocktail `@prototype` items with custom `@effect` buffs instead of drunk stacks.
- `@npc` surf instructor granting `mod-skill survival` on `on_enter`.
- Retarget pearl inlet `scatter_to` to your coastal city `base_name`.
- `@resource-spawner` on a new beach harvest node for event-only souvenirs.