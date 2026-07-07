# Haunted Forest

**Pack:** `haunted_forest.mudl` · **ID:** `haunted_forest` · **Entry:** `haunted-entry`

---

## 1. Theme teaser

Silver mist, moonlit glades, and a wood that remembers those who listen. Gothic folktale beauty — uneasy, not loud. Wrong paths feel almost polite; something watches from between the trees. Bring a light, patience, and time to read what the forest left behind.

---

## 2. Quick install

### Import (GitHub)

Add to your `world.mudl`:

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/haunted_forest.mudl
```

### Minimal host map

The pack expects a location named `forest-path` (it places the hollow oak portal there). Add a hub that reaches it:

```mudl
type: area
base_name: my-hub
name: My Hub
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
  south: my-hub
  in: haunted-entry
exit_returns:
  south: north
  in: out
```

Optional richness: a clearing named `the-void` (mossy boulder clue) and a travel chest named `scene-chest` (forest key item) — the pack places items there automatically.

### Run

```bash
cargo run --bin repl
```

### Link & portal (wizard, in-game)

If you skipped the map exit, stand in `forest-path`:

```text
> go forest-path
> @link in haunted-entry --return out
```

Or create a portal from the pack's prototype:

```text
> @create item "Hollow Oak" prototype=hollow-oak-portal door_direction=in door_destination=haunted-entry
> @link in haunted-entry --return out
```

### Play

```text
> look
> go north
> look
> examine split oak
> examine boulder
> examine chest
> go in
> look
> read marker
> go north
```

---

## 3. What to expect

**Tone & danger:** Medium. Navigation tension, ambient phantoms, and lurkers that may attack if you blunder in unprepared. Stealth and survival help; combat is possible but not constant.

**The wood:**

- A **threshold** where many paths look equally inviting — some are wrong turns that loop you back without fanfare.
- **Way markers** and readable stones whose wording rewards careful reading (symbols and order matter — the forest will not say which order).
- **Moonlit, ember, mirror, and ash** themed regions — each with its own air and hazards.
- **Weather and respawn schedules** — mist thickens; silver motes gather again in certain glades.

**Objects & interactions:**

- A **locked hollow oak** portal (consumable key mechanics).
- **Breakable** clay pots with scripted shatter lines.
- **Harvestable** moss patches with renewable drops.
- **Hidden supply cache** revealed by perception — triggers fire on discovery.
- **Shrine** with a first-visit offering.
- **Reward chest** at the deep heart for those who earn the true exit.

**Creatures:**

- Weighted **on-enter and periodic spawns** — drifting wisps and lurkers with awareness checks.
- Lurkers stay hidden until you discover them; surprise and counter-attack rules apply.

**Leaving:**

- A deliberate **out** exit from the heart can scatter you to familiar ground elsewhere in the world — you do not have to walk back through every wrong turn.

**Commands to know:** `look`, `examine`, `read`, `go`, `take`, `harvest`, `attack` (if something lunges), `open` / `unlock` on doors and chests.

*No route or puzzle solutions here — the boulder, mailbox gear, and markers are for you to connect in play.*

---

## 4. Extension ideas

- `@schedule` extra `on_weather` lines on mist rooms for seasonal horror.
- Duplicate the hollow oak portal on a second host path with a new `door_direction`.
- `@spawn-template` friendly ghost with `react=ignore` between scares.
- `@loot-spawner` entries on the heart chest for campaign artifacts.
- `@trigger on_discovered` on your own hidden caches for lore-only drops.