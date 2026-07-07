# Adventure Modules & Expansion Packs

Project MUDL ships self-contained **expansion packs** — single `.mudl` files that add areas, items, NPCs, effects, spawners, and scripted events to an existing world. Each pack is designed to drop in with minimal host-world wiring: a portal exit, a signpost, or a named location the expansion can attach to.

This guide is for **builders and players** who want to know what each module feels like, how to install it, and how to extend it. It avoids puzzle solutions, optimal routes, and reward specifics so discovery stays in-game.

---

## How expansions work

Every pack begins with `@expansion` metadata and loads through `@import` in your world's `world.mudl`:

```mudl
@import expansions/haunted_forest.mudl
```

Paths resolve relative to the world directory (`worlds/<name>/`). You can also fetch remote packs:

```mudl
@import https://example.com/mudl/expansions/haunted_forest.mudl
```

An expansion typically bundles:

| Piece | Purpose |
|-------|---------|
| **Areas** | New rooms with descriptions, exits, `@trigger` scripts |
| **Prototypes & items** | Gear, containers, portals, harvest nodes |
| **Effects & behaviors** | Temporary buffs, creature personalities |
| **Spawners** | Loot, resources, ambient creatures |
| **NPCs** | Fixed characters with scripted lines |
| **Integration hooks** | Items placed in host-world locations, portal objects, signs |

The `integrates=` line in `@expansion` lists **host locations** the pack expects (for validation and documentation). Your map must expose matching `base_name` areas or the expansion must place portal objects that register new exit directions.

**After changing `world.mudl`**, restart the REPL or run `module reload` (wizard) so bootstrap picks up new imports. On a fresh database, all imported content loads automatically.

---

## Quick-start install (any module)

### 1. Copy or reference the file

Place the `.mudl` file under your world's `expansions/` folder, or host it at a URL.

```
worlds/my_world/
  world.mudl
  map.mudl
  expansions/
    haunted_forest.mudl
```

### 2. Import in `world.mudl`

```mudl
@world my_world
  starting_location=the-void
@end
@include map.mudl
@import expansions/haunted_forest.mudl
```

### 3. Wire the host map (minimal hooks)

Add exits or named locations the pack's header comments describe. Example for **Haunted Forest**:

```mudl
type: area
base_name: forest-path
exits:
  in: haunted-entry    # expansion defines haunted-entry and places the hollow oak portal
```

Many packs also place **portal objects** (`@item` with `door_direction` / `door_destination`) so you do not need a static map exit — the object registers the direction when bootstrap runs.

### 4. Optional integration items

Some packs expect named host objects (e.g. `scene-chest` in the default world's travel chest). Either keep those `base_name` IDs or edit the expansion's `@item` blocks to point at your equivalents.

### 5. Run and verify

```bash
MUDL_MODULE=modules/default cargo run --bin repl
```

From the starting area, follow signs and obvious paths toward the module's entrance. Use `look` and `examine` on plaques, markers, and unusual objects — they are written for in-character discovery.

---

## Default world: where modules connect

The stock `default_world` already imports all official packs. From **West Clearing** (`the-void`):

| Direction / action | Leads toward |
|--------------------|--------------|
| `north` | Forest Path — gateways to Haunted Forest, Poisonous Swamp, and Glimmerfen |
| `south` | Sandy Trail — Sandy Shoals Resort |
| Forest Path `down` | Poisonous Swamp |
| Forest Path `in` | Haunted Forest (hollow oak) |
| Forest Path `through` | Glimmerfen (mushroom ring) |
| Beach Shore `mist` | Glimmerfen (moonmist arch) |
| Swamp Dry Mound `in` | Giant Spider Den (when both swamp and den are loaded) |

Modules can **scatter** you back to the clearing, forest, or beach when you leave their heart area — a soft return without retracing every step.

---

## Module guide

### West Clearing (starting scene)

**File:** `map.mudl`, `items.mudl` (core world, not an expansion)

**Theme:** Edge-of-nowhere Zork-adjacent clearing — worn mailbox, locked travel chest, paths into forest and cottage.

**Atmosphere:** Quiet, slightly mysterious, grounded. A good hub before stranger biomes.

**What to expect:** Light exploration, a locked chest puzzle tied to items in the clearing, cottage doors and windows, no combat in the clearing itself. Gear here prepares you for harsher modules.

**Install:** Already in `default_world`. For a custom world, copy `map.mudl` hooks and starting items or retarget `starting_location`.

**Extension ideas:**

- Add a fourth exit to your own expansion's trailhead.
- Replace the travel chest loot with campaign-specific prototypes.
- `@npc` a wandering merchant in the clearing who hints at nearby modules without solving them.

---

### Haunted Forest

**File:** `expansions/haunted_forest.mudl` · **ID:** `haunted_forest`

**Theme:** Silver mist, wrong paths, and a wood that remembers those who listen.

**Atmosphere:** Gothic folktale — moonlit glades, ash hollows, mirror water, forgotten shrines. Uneasy but beautiful. Tension comes from navigation and lurking presences, not jump scares.

**What to expect:**

- A **threshold maze**: many paths look alike; some loop you back gently.
- **Way markers** and readable clues scattered through the wood (pay attention to symbols and order language — the forest rewards readers).
- **Hidden objects** and harvestable nodes; breaking certain objects changes the room.
- **Ambient phantoms** and **lurkers** that may attack if you blunder into them unprepared.
- **Healing** near the deepest calm of the wood; a **reward container** for those who find the true heart and exit deliberately.
- Items in the **starting clearing** and **travel chest** tie into the forest — explore the hub before diving in.

**Danger:** Medium. Combat possible; stealth and survival skills help.

**Install:**

```mudl
@import expansions/haunted_forest.mudl
```

Host hooks (default world already has these):

- `forest-path` exit `in: haunted-entry`
- `the-void` — mossy boulder (readable clue)
- `scene-chest` — forest-related key item for the hollow oak

**Extension ideas:**

- Add a second portal from your own forest area using `@prototype` portal + `@item` with `door_direction`.
- `@schedule` new weather events on mist rooms for seasonal horror.
- Chain a custom `@loot-spawner` on the heart chest for your campaign's artifact.
- `@spawn-template` friendly ghost with `react=ignore` for comic relief between scares.

---

### Poisonous Swamp

**File:** `expansions/poisonous_swamp.mudl` · **ID:** `poisonous_swamp`

**Theme:** Stinking sink — bitter fen, sweet reeds, dry fungus, and a bog that keeps what it steals.

**Atmosphere:** Unpleasant, tactile, survival-horror adjacent. Green fog, gas pockets, leeches, and a boss-tier warden at the deep heart. The swamp *teaches* through damage and antidote scarcity.

**What to expect:**

- **Environmental harm** on entry to harsh rooms; gear and effects mitigate pressure.
- **Navigation puzzle** using carved stakes and readable warnings (taste and texture matter).
- **Harvestable** roots and reeds; **breakable** spore pods; **hidden caches** with salves.
- **Creature spawns** — gas wisps, leeches, crawlers — with ambush and awareness rules.
- **Fixed warden NPC** at the heart; defeating it grants a lasting resilience effect.
- **Reward coffer** at the deep heart; **scatter exit** back to the forest or clearing.
- Optional **portal deeper** when Giant Spider Den is loaded.

**Danger:** High. Damage over time, poison themes, mandatory combat at the heart.

**Install:**

```mudl
@import expansions/poisonous_swamp.mudl
```

Host hooks:

- `forest-path` exit `down: swamp-entry` (return `up: forest-path`)
- Warning post placed on forest path by the expansion

**Extension ideas:**

- `@effect` mudwalking boots that reduce enter damage on gas rooms.
- `@resource-spawner` new harvestables for alchemy crafting pipelines.
- Replace warden with a negotiable NPC (`react=warn`) for a pacifist variant.
- Link `swamp-dry` `in` exit to your own dungeon instead of (or alongside) the spider den.

---

### Giant Spider Den

**File:** `expansions/giant_spider_den.mudl` · **ID:** `giant_spider_den`

**Theme:** Shelob-inspired brood crawl — silk, eggs, fangs, and a crown of webs.

**Atmosphere:** Claustrophobic, vertical, predatory. Pale silk, iron gates, starlight wards, and silence that is never empty. Best entered after the swamp when you want a dungeon crescendo.

**What to expect:**

- **Web-slowed** movement and perception effects in oppressive rooms.
- **Thread-themed navigation** with bone peg markers and a warning plaque (read everything).
- **Lurking brood** — hatchlings, broodlings, ceiling lurkers — awareness and ambush apply.
- **Breakable** cocoons and swellable objects; light and keys matter.
- **Boss encounter** at the crown; kill reward includes a sharp senses effect.
- **Weighted loot** in the crown chest; scatter exit to forest or clearing.

**Danger:** High. Stealth, light sources, and combat skill strongly recommended.

**Install:**

```mudl
@import expansions/giant_spider_den.mudl
```

Host hooks:

- `swamp-dry` (from Poisonous Swamp) — webbed fissure portal placed by `@item`
- Works standalone if you add your own portal `@item` pointing at `spider-entry`

**Extension ideas:**

- `@prototype` torch that grants `starlight_ward` while wielded.
- Add non-hostile `@spawn-template` cave moths for atmosphere without combat.
- Branch the crown exit into your megadungeon's next tier.
- `@trigger on_kill` on the queen that opens a gate elsewhere in the swamp.

---

### Sandy Shoals Resort (Beach Resort)

**File:** `expansions/beach_resort.mudl` · **ID:** `beach_resort`

**Theme:** Whimsical tropical resort — tiki bar, tidepools, hammocks, pearl inlet.

**Atmosphere:** Restorative, sunny, slightly silly. A palate cleanser after haunted woods and bogs. No combat; the worst outcome is a sunburn and better stories.

**What to expect:**

- **Short sandy trail** south from the clearing to a resort gate.
- **Way markers** along a taste-themed route (the sign in the clearing sets expectations without spoiling the path).
- **Tiki bar** with stackable cocktails and a **drunk mechanism** — effects stack, dexterity drops, NPCs and rooms react differently.
- **Healing** on verandas, tidepools, and the pearl inlet; **hammock attendant** tends tired travelers.
- **Harvest** shells and tidepool gifts; **hidden** sand caches and tidepool bundles.
- **Friendly NPCs** only (bartender, attendant, pier hermit).
- **Pearl coffer** reward at the inlet; gentle wrong turns loop to the gate.
- **Scatter exit** back to clearing, forest, or trail.

**Danger:** None.

**Install:**

```mudl
@import expansions/beach_resort.mudl
```

Host hooks:

- `the-void` exit `south: beach-trail` (return `north: the-void`)
- Resort trail sign placed in the clearing by the expansion

**Extension ideas:**

- `@schedule` sunset `on_weather` lines on the shore for time-of-day flavor.
- New `@prototype` mocktails that grant custom `@effect` buffs instead of drunk effects.
- `@npc` surf instructor who trains survival skill through `on_enter` `mod-skill`.
- Link the pearl inlet scatter to your coastal city instead of the clearing.

---

### Glimmerfen (Magical Fairy Land)

**File:** `expansions/fey_glade.mudl` · **ID:** `fey_glade` · **Display name:** Glimmerfen

**Theme:** Rivendell-meets-Legend fey vale — moonpetals, singing reeds, glowcaps, elves, gnomes, and pixies.

**Atmosphere:** Pristine, enchanted, peaceful. Silver grass, chiming willows, fairy rings, and starlight pools. Wonder first; challenge is observation and patience, not blades.

**What to expect:**

- **Two portals in:** mushroom ring on the forest path (`through`), moonmist arch on the beach shore (`mist`).
- **Threshold maze** with kind wrong turns — mist, bramble, mirror glades that return you softly.
- **Hush-themed navigation** with runestone markers (the forest plaque explains the vocabulary, not the route).
- **Magical effects** granted by areas and consumables — vigor, songpeace, luminance, rootwise, grace.
- **Harvestable plants** in every major glade; **hidden** fairy nest and gnome cache.
- **Ambient fey spawns** (moths, wisps, song spirits) — shy or wandering, never aggressive.
- **NPCs:** fairy guide, gnome gardener, elder elf sage — greetings, healing, light buffs.
- **Grace coffer** at the starlight pool; scatter exit to clearing, forest, or beach trail.

**Danger:** None.

**Install:**

```mudl
@import expansions/fey_glade.mudl
```

Host hooks:

- `forest-path` — mushroom ring portal (`through` → `fey-threshold`)
- `beach-shore` — moonmist arch (`mist` → `fey-threshold`) — requires Beach Resort loaded for that entrance
- Fey crossing plaque on forest path

**Extension ideas:**

- `@effect` seasonal blessings toggled by `@schedule` on the grace pool.
- `@spawn-template` unicorns with `fey_wanderer` behavior for rare on_enter sightings.
- A third portal from your castle courtyard using the same `fey-threshold` destination.
- Player-conduct triggers: `when skill stealth at_least 3` on fairy ring for bonus dust (pattern already used in hidden discoveries).

---

## Suggested play order (default world)

No canon required — modules are independent — but this route respects difficulty and tone:

1. **West Clearing** — gear and clues  
2. **Sandy Shoals** or **Glimmerfen** — recover morale, learn triggers and harvest  
3. **Haunted Forest** — navigation and lurkers  
4. **Poisonous Swamp** — survival pressure and warden  
5. **Giant Spider Den** — capstone crawl (from swamp dry mound)

Return to the resort or vale anytime for healing and buffs between harder runs.

---

## Building your own expansion

Use the official packs as templates:

1. Copy the nearest tone neighbor (`beach_resort.mudl` for peaceful, `haunted_forest.mudl` for maze + triggers).
2. Tag `@expansion your_id` with `integrates=` host `base_name` list.
3. Define `@prototype` / `@item` / `type: area` blocks in one file.
4. Place portals via `@item` + `door_direction` + `door_destination` when you do not want to edit host `map.mudl`.
5. Use `loop_to` for benign wrong turns; `scatter_to` + `scatter_direction` for one-way exits home.
6. Run `cargo test` — bootstrap tests validate spawner targets and integration IDs.

**Spawner cheat sheet:**

| Block | Use for |
|-------|---------|
| `@loot-spawner` | Chests, shrines, one-shot rewards |
| `@resource-spawner` | Harvest nodes (`harvest <object>`) |
| `@spawner` | Ambient or hostile creatures |
| `@schedule` | Periodic `on_weather` / `on_respawn` room events |

Language reference: [LANGUAGE.md](LANGUAGE.md) (events, triggers, effects). Builder tools: [BUILDER.md](BUILDER.md). Commands: [COMMANDS.md](COMMANDS.md).

---

## File index

| Module | File |
|--------|------|
| Haunted Forest | `modules/default/worlds/default_world/expansions/haunted_forest.mudl` |
| Poisonous Swamp | `modules/default/worlds/default_world/expansions/poisonous_swamp.mudl` |
| Giant Spider Den | `modules/default/worlds/default_world/expansions/giant_spider_den.mudl` |
| Sandy Shoals Resort | `modules/default/worlds/default_world/expansions/beach_resort.mudl` |
| Glimmerfen | `modules/default/worlds/default_world/expansions/fey_glade.mudl` |

Default world entrypoint: `modules/default/worlds/default_world/world.mudl`