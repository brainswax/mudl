# Giant Spider Den

A brood crawl beneath the earth — silk galleries, egg chambers, iron gates, and silence that clicks. Pale strands, swollen cocoons, and something vast turning in its sleep at the crown. Light, keys, and nerve matter as much as steel.

## Quick Install

Stand in any room and paste:

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/giant_spider_den/giant_spider_den.mudl
@create portal "Spider Den" prototype=webbed-fissure door_direction=in door_destination=spider-entry
@link in spider-entry --return out
```

Then `go in`.

## Detailed description

**Module:** `@expansion giant_spider_den` · entry `spider-entry` · portal `in` / return `out`

**Areas**

| base_name | Role |
|-----------|------|
| `spider-entry` | Entry; north/east/south/west to route and wrong turns; `out` → `swamp-dry` (`exit_returns`: `in`) |
| `spider-silk` | Main route — silk galleries; east → `spider-egg` |
| `spider-egg` | Egg chamber; south → `spider-fang` |
| `spider-fang` | Fang antechamber; brood iron gate west → `spider-crown` |
| `spider-crown` | Finale; `out` scatters (`scatter_to`: `forest-path`, `the-void`, `swamp-dry`) |
| `spider-gloom`, `spider-pit`, `spider-hatch`, `spider-nest`, `spider-drown`, `spider-shrine` | Wrong turns → `loop_to: spider-entry` |

**Tone:** High danger. Web-slowed movement, lurkers with awareness checks, hatchling swarms, brood queen boss. Stealth, light, and survival strongly recommended.

**Features:** `@effect` webbed_slow, starlight_ward, spider_sense. Brood lantern, starlight ward gear. Breakable cocoons and swellable egg sacs. Brood iron gate (`lock_id=brood-fang-key`). Weighted crown coffer. Ceiling lurkers, broodlings, hatchlings; brood queen NPC with attack behavior. Scheduled tremors in egg chamber.

**Hidden:** Silk cache at `spider-silk` (`hidden_until_discovered`).

**Puzzles:** Warning plaque and bone peg markers teach a thread vocabulary (SILK, EGG, FANG, CROWN). Gate and thread order are for in-game signage — not listed here.

**Commands:** `look`, `examine`, `read`, `go`, `take`, `attack`, `open`, `unlock`, `wield`.

## Extension ideas

- `@prototype` hand torch granting `starlight_ward` while wielded.
- Harmless `@spawn-template` cave moths (`react=ignore`) for atmosphere.
- `@link` the crown `out` scatter target to your megadungeon's next tier.
- `@trigger on_kill` on the queen that spawns a key or opens a distant gate.
- `@schedule` `on_weather` tremors in the crown on a faster interval for live events.