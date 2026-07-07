# Adventure Modules & Expansion Packs

Spoiler-free overview of official MUDL adventure modules. **Install guides, GitHub URLs, and per-module READMEs** live next to each pack:

**[modules/default/worlds/default_world/expansions/README.md](modules/default/worlds/default_world/expansions/README.md)**

## Load any pack from GitHub

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/haunted_forest.mudl
```

See the [expansions README](modules/default/worlds/default_world/expansions/README.md) for a full quick-start (`world.mudl`, map exits, `@link`, portal `@item`, and play commands).

## Module index

| Module | Teaser | Doc |
|--------|--------|-----|
| **Haunted Forest** | Silver mist, wrong paths, lurking presences — gothic folktale navigation | [README](modules/default/worlds/default_world/expansions/haunted_forest/README.md) |
| **Poisonous Swamp** | Stinking sink, gas and leeches, warden at the deep heart | [README](modules/default/worlds/default_world/expansions/poisonous_swamp/README.md) |
| **Giant Spider Den** | Brood crawl — silk, eggs, crown of webs | [README](modules/default/worlds/default_world/expansions/giant_spider_den/README.md) |
| **Sandy Shoals Resort** | Tiki bar, hammocks, cocktails, zero combat | [README](modules/default/worlds/default_world/expansions/beach_resort/README.md) |
| **Glimmerfen** | Fey vale — elves, gnomes, pixies, harvest and wonder | [README](modules/default/worlds/default_world/expansions/fey_glade/README.md) |

## Default world connections

From **West Clearing** (`the-void`) in the stock `default_world`:

| Action | Module |
|--------|--------|
| `go north` → forest path | Hub for forest, swamp, fey portals |
| `go south` | Sandy Shoals Resort |
| Forest `go in` | Haunted Forest |
| Forest `go down` | Poisonous Swamp |
| Forest `go through` | Glimmerfen |
| Beach shore `go mist` | Glimmerfen (with beach loaded) |
| Swamp dry `go in` | Giant Spider Den (with den loaded) |

## Suggested play order

1. West Clearing — gear and clues  
2. Sandy Shoals or Glimmerfen — rest, learn triggers  
3. Haunted Forest — navigation and lurkers  
4. Poisonous Swamp — survival pressure  
5. Giant Spider Den — capstone crawl  

## Building your own pack

Copy the nearest neighbor `.mudl`, tag `@expansion`, list `integrates=` host `base_name` values, add a `README.md` folder beside your file. Reference: [LANGUAGE.md](LANGUAGE.md) · [BUILDER.md](BUILDER.md) · [COMMANDS.md](COMMANDS.md).