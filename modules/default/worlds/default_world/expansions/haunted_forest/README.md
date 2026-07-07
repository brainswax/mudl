# Haunted Forest

Silver mist, moonlit glades, and a wood that remembers those who listen. Gothic folktale beauty — uneasy, not loud. Wrong paths feel almost polite; something watches from between the trees. Bring a light, patience, and time to read what the forest left behind.

## Quick Install

Stand in any room and paste:

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/haunted_forest/haunted_forest.mudl
@create portal "Haunted Forest" door_direction=in door_destination=haunted-entry
@link in haunted-entry --return out
```

Then `go in`.

## Detailed description

**Module:** `@expansion haunted_forest` · entry `haunted-entry` · portal `in` / return `out`

**Areas**

| base_name | Role |
|-----------|------|
| `haunted-entry` | Entry; north/east/south/west to route and wrong turns |
| `haunted-moon` | Main route — moonlit theme; east → `haunted-ember` |
| `haunted-ember` | Main route — ember theme; south → `haunted-mirror` |
| `haunted-mirror` | Main route — mirror theme; west → `haunted-ash` |
| `haunted-ash` | Main route — ash theme; north → `haunted-heart` |
| `haunted-heart` | Finale; `out` scatters (`scatter_to`: `the-void`, `forest-path`, `cottage-rear`) |
| `haunted-wither`, `haunted-mist`, `haunted-moss`, `haunted-thicket`, `haunted-pool`, `haunted-bones`, `haunted-shrine` | Wrong turns → `loop_to: haunted-entry` |

**Tone:** Medium danger. Navigation tension, ambient phantoms, lurkers with awareness checks. Stealth and survival help.

**Features:** Hollow oak portal (`prototype=hollow-oak-portal`, consumable lock `oak-whisper`). Whisper charm key. Breakable clay pots, harvestable moss, shrine offering, rootbound reward chest at the heart. Wisps and lurkers via `@spawner` on enter and periodic ticks. Weather thickens mist in select glades.

**Hidden:** Supply cache at `haunted-moss` (`hidden_until_discovered`).

**Puzzles:** Mossy boulder, mailbox gear, and way markers reward careful reading; symbol order matters in play. No route charted here.

**Commands:** `look`, `examine`, `read`, `go`, `take`, `harvest`, `attack`, `open`, `unlock`.

## Extension ideas

- `@schedule` extra `on_weather` lines on mist rooms for seasonal horror.
- `@create item` a second hollow oak on another host path with a new `door_direction`.
- `@spawn-template` friendly ghost with `react=ignore` between scares.
- `@loot-spawner` entries on the heart chest for campaign artifacts.
- `@trigger on_discovered` on your own hidden caches for lore-only drops.