# Glimmerfen

**Pack:** `fey_glade.mudl` · **ID:** `fey_glade` · **Display name:** Glimmerfen · **Entry:** `fey-threshold`

---

## 1. Theme teaser

A pristine fey vale — silver grass, singing reeds, glowcap counsel, and starlight that pools without rushing. Elves, gnomes, and pixies attend without fuss. Wonder first; the challenge is noticing what the vale left for the gentle-footed.

---

## 2. Quick install

### Import (GitHub)

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/fey_glade.mudl
```

### Minimal host map (forest entrance)

The pack places a **mushroom ring** portal and **fey crossing plaque** on `forest-path`:

```mudl
type: area
base_name: my-hub
name: My Hub
exits:
  north: forest-path
exit_returns:
  north: south

type: area
base_name: forest-path
name: Forest Path
description: Pale caps ring the trail in a perfect circle.
exits:
  south: my-hub
  through: fey-threshold
exit_returns:
  south: north
  through: through
```

### Second entrance (optional beach portal)

For the **moonmist arch**, add a shore named `beach-shore` — the pack places the arch there:

```mudl
type: area
base_name: beach-shore
name: Open Shore
description: Silver fog gathers in an arch of driftwood and shell.
exits:
  mist: fey-threshold
exit_returns:
  mist: mist
```

### Run

```bash
cargo run --bin repl
```

### Create portal & link (wizard)

Forest ring (if you skipped map exit):

```text
> go forest-path
> @create item "Mushroom Ring" prototype=mushroom-ring-portal door_direction=through door_destination=fey-threshold
> @link through fey-threshold --return through
> go through
```

Beach arch:

```text
> go beach-shore
> @create item "Moonmist Arch" prototype=moonmist-arch door_direction=mist door_destination=fey-threshold
> @link mist fey-threshold --return mist
> go mist
```

### Play

```text
> look
> read plaque
> go through
> look
> examine marker
> go north
> harvest moonpetals
> examine willow
```

---

## 3. What to expect

**Tone & danger:** None. No attack behaviors; ambient fey spawns only emote and wander.

**The vale:**

- **Threshold** with four outward paths — kind wrong turns (mist, bramble, mirror glade, wisp hollow, fern cathedral, petal shrine) loop back softly.
- **Hush-themed navigation** with runestone markers (the crossing plaque teaches vocabulary, not the route).
- **Dewglade, songbower, glowfen, rootbridge, grace pool** — each grants magical `@effect` buffs on entry and healing when you are worn down.
- **Weather and respawn schedules** — mist rearranges; songbower composes anew; moths and wisps respawn.

**Magical effects (examples):**

- `dewdrop_vigor`, `songpeace`, `glowcap_luminance`, `rootwise`, `fey_grace`, `wonderstruck`, `pixie_dust` — from areas, consumables, and wearables.

**NPCs:**

- **Fairy guide** at the threshold, **gnome gardener** in the dewglade, **elder elf sage** at the grace pool — greetings, healing, light buffs.

**Objects & interactions:**

- **Harvestable** moonpetals, singing reeds, glowcaps, starroots — teas, tinctures, charms, cloaks.
- **Luminous willow** heals on approach; **fairy ring** may yield dust on a chance trigger.
- **Hidden fairy nest** (stealth-flavored discovery) and **gnome cache** with knife and lore scroll.
- **Grace coffer** — weighted elven circlet, fey cloak, starbloom charm, dust, scroll, salves.
- **Consumables** (`on_take`): moonpetal tea, glowcap tincture, pixie dust vial.

**Ambient life:**

- Glimmer moths, pixie wisps, song spirits — shy or wandering spawns in glades.

**Leaving:**

- **Out** from grace pool scatters to clearing, forest, or beach trail.

**Commands to know:** `look`, `examine`, `read`, `go`, `take`, `harvest`, `inventory`, `wear`.

*Marker sequence and grace path stay in the stones — not in this file.*

---

## 4. Extension ideas

- `@schedule` seasonal `@trigger on_weather` blessings on the grace pool.
- `@spawn-template` unicorn with `fey_wanderer` for rare `on_enter` sightings.
- Third portal from a custom courtyard → `fey-threshold` with a new `door_direction`.
- `when skill stealth at_least N` triggers on your own fairy rings (pattern used in hidden nest).
- `@effect` festival_merriment toggled by wizard `@set` on the threshold for live events.