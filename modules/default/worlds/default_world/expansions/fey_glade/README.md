# Glimmerfen (Magical Fairy Land)

**File:** [../fey_glade.mudl](../fey_glade.mudl) · **Expansion ID:** `fey_glade` · **Display name:** Glimmerfen

## Teaser

A pristine fey vale — Rivendell meets Legend. Silver grass, singing reeds, glowcap mushrooms, fairy rings, elves, gnomes, and pixies. Wonder and healing first; challenge is observation and patience, not blades. Two portals in: forest mushroom ring and beach moonmist arch.

**Danger:** None. **Spoilers:** None below.

## Install

### Import from GitHub

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/fey_glade.mudl
```

### Host map — forest entrance

```mudl
type: area
base_name: forest-path
name: Forest Path
description: A path into the trees.
exits:
  through: fey-threshold
exit_returns:
  through: through
```

The pack places the **mushroom ring** portal (`door_direction=through`) and a **fey crossing plaque** on `forest-path`.

### Beach entrance (optional)

Requires a `beach-shore` area — easiest if [Sandy Shoals Resort](../beach_resort/README.md) is loaded:

```mudl
type: area
base_name: beach-shore
exits:
  mist: fey-threshold
exit_returns:
  mist: mist
```

The pack places the **moonmist arch** on `beach-shore`.

### Link at runtime (wizard)

```text
> go forest-path
> @link through fey-threshold --return through
```

### Play (default world)

```text
> go north
> read plaque
> go through
> look
> examine marker
> go north
> harvest moonpetals
> examine willow
```

`harvest` plants in each glade; `examine` ferns and rings; high `stealth` may help hidden discoveries; no `attack` needed.

## Extension ideas

- `@schedule` seasonal blessings on the grace pool.
- `@spawn-template` unicorn with `fey_wanderer` for rare sightings.
- Third portal from your castle courtyard → `fey-threshold`.
- `when skill stealth at_least N` triggers on fairy ring (pattern in pack).

## See also

- [Sandy Shoals Resort](../beach_resort/README.md)
- [Expansions index](../README.md)
- [MODULES.md](../../../../../MODULES.md)