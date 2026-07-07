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

## Details

**Entry:** `haunted-entry` — a threshold where many paths look equally inviting; wrong turns loop back without fanfare. Way markers and readable stones reward careful reading. Themed regions: moonlit, ember, mirror, and ash glades with weather and respawn schedules.

**Tone:** Medium danger. Navigation tension, ambient phantoms, and lurkers that may attack if you blunder in unprepared. Stealth and survival help.

**Notable features:** Locked hollow oak portal (consumable key via whisper charm), breakable clay pots, harvestable moss, hidden supply cache, shrine offering, and rootbound reward chest at the deep heart. Wisps and lurkers spawn on enter and on periodic ticks. A deliberate `out` exit from the heart scatters you to familiar ground.

**Commands:** `look`, `examine`, `read`, `go`, `take`, `harvest`, `attack`, `open`, `unlock`.

## Extension ideas

- `@schedule` extra `on_weather` lines on mist rooms for seasonal horror.
- `@create item` a second hollow oak on another host path with a new `door_direction`.
- `@spawn-template` friendly ghost with `react=ignore` between scares.
- `@loot-spawner` entries on the heart chest for campaign artifacts.
- `@trigger on_discovered` on your own hidden caches for lore-only drops.