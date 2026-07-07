# Poisonous Swamp

A stinking sink where bitter fen, sweet reeds, and black water teach hard lessons. Green fog beads on your sleeves; leeches ripple without breaking the surface. The bog keeps what it steals — and something warden-sized waits where the peat runs deepest.

## Quick Install

Stand in any room and paste:

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/poisonous_swamp/poisonous_swamp.mudl
@create portal Swamp door_direction=down door_destination=swamp-entry
@link down swamp-entry --return up
```

Then `go down`.

## Details

**Entry:** `swamp-entry` — a threshold bowl of stagnant air; wrong paths loop back. Carved stakes and a warning post hint at tastes and textures (bitter, sweet, dry, deep). Regions include bitter fen, sweet stand, dry rise, and deep heart with gas pockets, quicksand, and snares.

**Tone:** High danger. Environmental damage on harsh rooms, creature ambushes, antidote scarcity, and a fixed warden NPC at the deep heart. Defeating the warden grants `bog_resilience`.

**Notable features:** Reed breather mask and reed-walker boots; antidote salves from harvests, hidden caches, and breakable spore pods. Gas wisps, bog leeches, and mire crawlers throughout. `up` from the heart scatters to forest or clearing.

**Commands:** `look`, `examine`, `read`, `go`, `take`, `harvest`, `attack`, `wear`.

## Extension ideas

- `@effect` mudwalking boots that soften `on_enter` damage in gas rooms.
- `@resource-spawner` on new harvest nodes for an alchemy crafting chain.
- Swap the warden for `react=warn` dialogue and a quest item instead of combat.
- `@trigger on_kill` on the warden that opens a gate in your own world.
- Extra `@schedule` on the gas pocket for louder weather every N room ticks.