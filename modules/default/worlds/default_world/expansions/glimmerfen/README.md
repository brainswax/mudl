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

## Detailed description

**Module:** `@expansion fey_glade` · entry `fey-threshold` · portal `through` / return `through`

**Areas**

| base_name | Role |
|-----------|------|
| `fey-threshold` | Entry; north/east/south/west to main route and wrong turns; `through` / `mist` host exits when integrated |
| `fey-dewglade` | Main route — dew theme; east → `fey-songbower` |
| `fey-songbower` | Main route — song theme; south → `fey-glowfen` |
| `fey-glowfen` | Main route — glow theme; west → `fey-rootbridge` |
| `fey-rootbridge` | Main route — root theme; north → `fey-grace` |
| `fey-grace` | Finale; `out` scatters (`scatter_to`: `the-void`, `forest-path`, `beach-trail`) |
| `fey-mist`, `fey-bramble`, `fey-mirror`, `fey-thorn`, `fey-wisp`, `fey-glimmer`, `fey-fern`, `fey-shrine` | Wrong turns → `loop_to: fey-threshold` |

**Tone:** Restorative. No attack behaviors; ambient fey spawns emote and wander.

**Features:** `@effect` dewdrop_vigor, songpeace, glowcap_luminance, rootwise, fey_grace, wonderstruck, pixie_dust. Harvest moonpetals, singing reeds, glowcaps, starroots. Luminous willow heals on approach; grace coffer with weighted elven gear. NPCs: fairy guide (`fey-threshold`), gnome gardener (`fey-dewglade`), elder elf sage (`fey-grace`). Weather/respawn schedules on mist and songbower.

**Hidden:** Gnome cache at `fey-rootbridge`; fairy nest at `fey-dewglade` (`hidden_until_discovered`).

**Puzzles:** Crossing plaque and way markers teach a hush vocabulary (DEW, SONG, GLOW, ROOT, GRACE). Marker sequence and grace path are in the stones — not documented here.

**Commands:** `look`, `examine`, `read`, `go`, `take`, `harvest`, `inventory`, `wear`.

## Extension ideas

- `@schedule` seasonal `@trigger on_weather` blessings on the grace pool.
- `@spawn-template` unicorn with `fey_wanderer` for rare `on_enter` sightings.
- `@create item` with `prototype=moonmist-arch` and `door_direction=mist` for a second entrance to `fey-threshold`.
- `when skill stealth at_least N` triggers on your own fairy rings.
- `@effect` festival_merriment toggled by wizard `@set` on the threshold for live events.