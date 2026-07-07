# Giant Spider Den

**Pack:** `giant_spider_den.mudl` · **ID:** `giant_spider_den` · **Entry:** `spider-entry`

---

## 1. Theme teaser

A brood crawl beneath the earth — silk galleries, egg chambers, iron gates, and silence that clicks. Shelob-adjacent dread: pale strands, swollen cocoons, and something vast turning in its sleep at the crown. Light, keys, and nerve matter as much as steel.

---

## 2. Quick install

### Import (GitHub)

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/giant_spider_den.mudl
```

### Minimal host map

The pack places a **webbed fissure** portal on a location named `swamp-dry`. Create that host (name matters for auto-placement):

```mudl
type: area
base_name: my-approach
name: Muddy Rise
description: Cracked peat rises here. A web-choked crack mouths open in the wall.
exits:
  south: my-hub
  in: spider-entry
exit_returns:
  south: north
  in: out

type: area
base_name: swamp-dry
name: Dry Mound
description: A rare rise of cracked peat. Wet silk strings a fissure.
exits:
  north: my-approach
  in: spider-entry
exit_returns:
  in: out
```

Or use only `swamp-dry` attached to your hub — the fissure object registers the `in` direction.

### Run

```bash
cargo run --bin repl
```

### Create portal & link (wizard)

On any host room if you skip `swamp-dry`:

```text
> go my-approach
> @create item "Webbed Fissure" prototype=webbed-fissure door_direction=in door_destination=spider-entry
> @link in spider-entry --return out
```

### Play

```text
> look
> go in
> look
> read plaque
> examine marker
> go north
```

---

## 3. What to expect

**Tone & danger:** High. Web-slowed movement, lurkers with awareness checks, hatchling swarms, and a **brood queen** boss at the crown. Stealth, light, and survival skills strongly recommended.

**The den:**

- A **webbed threshold** — wrong crawls loop back silently.
- **Thread-themed navigation** with bone peg markers and a warning plaque (read every word; the brood's vocabulary is in the signage).
- **Silk, egg, fang, and crown** themed chambers — escalating pressure.
- **Scheduled tremors and silk respawn** — ceilings bead fresh strands; something stirs in the egg chamber.

**Gear & effects:**

- **Brood lantern** and **starlight ward** gear; `@effect` **webbed_slow**, **starlight_ward**, **spider_sense**.
- **Brood iron gate** with key/consumable mechanics.
- **Weighted crown coffer** on open.

**Objects & interactions:**

- **Breakable** cocoons and swellable egg sacs.
- **Hidden silk cache** — perception discovery with triggers.
- **Readable** plaques and pegs throughout.

**Creatures:**

- Ceiling lurkers, broodlings, hatchlings — on-enter, periodic, and target-attached spawners.
- **Brood queen** NPC with aggressive behavior and kill reward effect.

**Leaving:**

- **Out** from the crown scatters to forest, clearing, or dry mound — your feet may not remember the crawl out.

**Commands to know:** `look`, `examine`, `read`, `go`, `take`, `attack`, `open`/`unlock`, `wield` (lantern).

*Thread order and gate sequences are for in-game markers to teach — not listed here.*

---

## 4. Extension ideas

- `@prototype` hand torch granting `starlight_ward` while wielded.
- Harmless `@spawn-template` cave moths (`react=ignore`) for atmosphere.
- `@link` the crown `out` scatter target to your megadungeon's next tier.
- `@trigger on_kill` on the queen that spawns a key or opens a distant gate.
- `@schedule` `on_weather` tremors in the crown on a faster interval for live events.