# Giant Spider Den

A brood crawl beneath the earth — silk galleries, egg chambers, iron gates, and silence that clicks. Pale strands, swollen cocoons, and something vast turning in its sleep at the crown. Light, keys, and nerve matter as much as steel.

## Quick Install

Paste the `@import` line into `world.mudl`, the two rooms into `map.mudl`, set `starting_location=start`, then run:

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/giant_spider_den/giant_spider_den.mudl

type: area
base_name: start
name: Start
exits:
  north: swamp-dry
exit_returns:
  north: south

type: area
base_name: swamp-dry
name: Dry Mound
description: A rare rise of cracked peat. Wet silk strings a web-choked fissure.
exits:
  south: start
  in: spider-entry
exit_returns:
  south: north
  in: out
```

```bash
cargo run --bin repl
```

```text
module reload
go north
go in
```

## Details

**Tone:** High danger. Web-slowed movement, lurkers with awareness checks, hatchling swarms, and a brood queen boss at the crown. Stealth, light, and survival skills strongly recommended.

**Inside the den:** A webbed threshold with wrong crawls that loop back, thread-themed navigation via bone peg markers, and silk/egg/fang/crown chambers with escalating pressure. Brood lantern and starlight ward gear; `@effect` webbed_slow, starlight_ward, spider_sense. Breakable cocoons, hidden silk cache, iron gate with key mechanics, and a weighted crown coffer. Ceiling lurkers, broodlings, hatchlings, and the brood queen.

**Commands:** `look`, `examine`, `read`, `go`, `take`, `attack`, `open`, `unlock`, `wield`.

## Extension ideas

- `@prototype` hand torch granting `starlight_ward` while wielded.
- Harmless `@spawn-template` cave moths (`react=ignore`) for atmosphere.
- `@link` the crown `out` scatter target to your megadungeon's next tier.
- `@trigger on_kill` on the queen that spawns a key or opens a distant gate.
- `@schedule` `on_weather` tremors in the crown on a faster interval for live events.