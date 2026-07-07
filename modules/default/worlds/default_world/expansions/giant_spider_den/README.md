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

## Details

**Entry:** `spider-entry` — a webbed threshold; wrong crawls loop back silently. Thread-themed navigation via bone peg markers and a warning plaque (silk, egg, fang, crown). Chambers escalate through silk galleries, egg nests, fang halls, and the crown where the brood queen waits.

**Tone:** High danger. Web-slowed movement, lurkers with awareness checks, hatchling swarms, and a brood queen boss. Stealth, light, and survival skills strongly recommended.

**Notable features:** Brood lantern and starlight ward gear; `@effect` webbed_slow, starlight_ward, spider_sense. Breakable cocoons, hidden silk cache, brood iron gate with key mechanics, weighted crown coffer. `out` from the crown scatters to forest, clearing, or dry mound.

**Commands:** `look`, `examine`, `read`, `go`, `take`, `attack`, `open`, `unlock`, `wield`.

## Extension ideas

- `@prototype` hand torch granting `starlight_ward` while wielded.
- Harmless `@spawn-template` cave moths (`react=ignore`) for atmosphere.
- `@link` the crown `out` scatter target to your megadungeon's next tier.
- `@trigger on_kill` on the queen that spawns a key or opens a distant gate.
- `@schedule` `on_weather` tremors in the crown on a faster interval for live events.