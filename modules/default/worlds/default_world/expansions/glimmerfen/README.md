# Glimmerfen

A pristine fey vale — silver grass, singing reeds, glowcap counsel, and starlight that pools without rushing. Elves, gnomes, and pixies attend without fuss. Wonder first; the challenge is noticing what the vale left for the gentle-footed.

## Quick Install

Stand in any room and paste:

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/glimmerfen/glimmerfen.mudl
@create portal Glimmerfen prototype=mushroom-ring-portal door_direction=through door_destination=fey-threshold
@link through fey-threshold --return through
```

Then `go through`.

## Details

**Entry:** `fey-threshold` — four outward paths with gentle wrong turns (mist, bramble, mirror glade, wisp hollow, fern cathedral, petal shrine) that loop back softly. Hush-themed navigation via runestone markers; the crossing plaque teaches vocabulary, not the route.

**Tone:** Restorative. No attack behaviors; ambient fey spawns emote and wander. Glades grant magical `@effect` buffs on entry and healing when worn down — dewdrop_vigor, songpeace, glowcap_luminance, rootwise, fey_grace, and others.

**Notable areas:** Dewglade, songbower, glowfen, rootbridge, grace pool. Harvest moonpetals, singing reeds, glowcaps, and starroots. NPCs: fairy guide, gnome gardener, elder elf sage. Leaving from the grace pool scatters to clearing, forest, or beach trail.

**Optional second entrance:** `prototype=moonmist-arch` with `door_direction=mist` on a shore room links to the same threshold.

**Commands:** `look`, `examine`, `read`, `go`, `take`, `harvest`, `inventory`, `wear`.

## Extension ideas

- `@schedule` seasonal `@trigger on_weather` blessings on the grace pool.
- `@spawn-template` unicorn with `fey_wanderer` for rare `on_enter` sightings.
- Third portal from a custom courtyard with a new `door_direction`.
- `when skill stealth at_least N` triggers on your own fairy rings.
- `@effect` festival_merriment toggled by wizard `@set` on the threshold for live events.