# Poisonous Swamp

**Pack:** `poisonous_swamp.mudl` · **ID:** `poisonous_swamp` · **Entry:** `swamp-entry`

---

## 1. Theme teaser

A stinking sink where bitter fen, sweet reeds, and black water teach hard lessons. Green fog beads on your sleeves; leeches ripple without breaking the surface. The bog keeps what it steals — and something warden-sized waits where the peat runs deepest.

---

## 2. Quick install

### Import (GitHub)

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/poisonous_swamp.mudl
```

### Minimal host map

The pack places a **warning post** on `forest-path`. Link downward into the bog:

```mudl
type: area
base_name: my-hub
name: My Hub
description: A path north leads into pine trees.
exits:
  north: forest-path
exit_returns:
  north: south

type: area
base_name: forest-path
name: Forest Path
description: The ground sounds hollow toward the east.
exits:
  south: my-hub
  down: swamp-entry
exit_returns:
  south: north
  down: up
```

Optional: a clearing named `the-void` — the heart's scatter exit can return you there.

### Run

```bash
cargo run --bin repl
```

### Link (wizard, in-game)

```text
> go forest-path
> @link down swamp-entry --return up
```

No door object required — this module uses a map descent. To use a custom hub name, stand there and:

```text
> @link down swamp-entry --return up
```

### Play

```text
> look
> go north
> read warning
> go down
> look
> examine marker
> harvest root
> examine cache
```

---

## 3. What to expect

**Tone & danger:** High. Environmental damage on harsh rooms, creature ambushes, antidote scarcity, and a **fixed warden NPC** at the deep heart with attack behavior. Defeating the warden grants a lasting resilience effect.

**The bog:**

- A **threshold** bowl of stagnant air — wrong paths loop back to the entry.
- **Carved stakes** and a warning post whose text hints at tastes and textures (read everything; the fen rewards attention, not speed).
- **Bitter, sweet, dry, and deep** themed regions — each with distinct ground, harvestables, and spawn tables.
- **Gas pockets** with scheduled weather belches; **quicksand**, snares, and drowned-glare wrong turns.

**Gear & effects:**

- **Reed breather mask** and **reed-walker boots** as wearable rewards.
- **Antidote salve** (stackable) from harvests, hidden caches, and breakables.
- `@effect` **reed_breathing** and **bog_resilience** defined in the pack.

**Objects & interactions:**

- **Harvestable** bitter roots and sweet reed beds.
- **Breakable** spore pods with choking break text and loot spawners.
- **Hidden mossy cache** in the sweet stand — discovered via perception.
- **Gas grate** (readable) in the gas pocket.
- **Heartwood coffer** at the deep heart — weighted loot on open.

**Creatures:**

- Gas wisps, bog leeches, mire crawlers — weighted spawns on enter and periodic ticks in fen rooms.

**Leaving:**

- **Up** from the heart scatters to forest or clearing; wrong turns use silent `loop_to` back to the threshold.

**Commands to know:** `look`, `examine`, `read`, `go`, `take`, `harvest`, `attack`, `wear` (mask/boots when found).

*Puzzle order and safe routes stay in-game — stakes and warnings tease the logic without this doc solving it.*

---

## 4. Extension ideas

- `@effect` mudwalking boots that soften `on_enter` damage in gas rooms.
- `@resource-spawner` on new harvest nodes for an alchemy crafting chain.
- Swap the warden for `react=warn` dialogue and a quest item instead of combat.
- Add `@trigger on_kill` on the warden that opens a gate in your own world.
- Extra `@schedule` on the gas pocket for louder weather every N room ticks.