# Haunted Forest

**File:** [../haunted_forest.mudl](../haunted_forest.mudl) · **Expansion ID:** `haunted_forest`

## Teaser

Silver mist, moonlit glades, and a wood that remembers those who listen. Gothic folktale atmosphere — uneasy but beautiful. Tension comes from navigation and unseen presences, not cheap scares. Bring patience, a light source, and time to read what the forest left for you.

**Danger:** Medium (lurkers, optional combat). **Spoilers:** None below — routes and puzzles stay in-game.

## Install

### Import from GitHub

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/haunted_forest.mudl
```

### Host map (minimum)

```mudl
type: area
base_name: forest-path
name: Forest Path
description: A path into the trees.
exits:
  in: haunted-entry
exit_returns:
  in: out
```

The pack places the **hollow oak** portal on `forest-path` and defines all `haunted-*` areas.

### Full integration (default-world style)

| Hook | Purpose |
|------|---------|
| `forest-path` | Portal + exit `in: haunted-entry` |
| `the-void` | Mossy boulder (readable clue) placed by pack |
| `scene-chest` | Travel chest — holds forest-related key item |

```mudl
type: area
base_name: the-void
exits:
  north: forest-path
```

### Link at runtime (wizard)

```text
> go forest-path
> @link in haunted-entry --return out
```

### Play (default world)

```text
> look
> go north
> examine boulder
> examine chest
> go in
> look
> examine marker
> go north
```

Use `read` on plaques and stones, `examine` on hidden-looking objects, `harvest` where nodes exist, `attack` only if something lunges.

## Extension ideas

- `@schedule` seasonal `on_weather` mist on wrong-turn rooms.
- Second portal from your own forest using `hollow-oak-portal` + `door_direction`.
- `@spawn-template` friendly ghost with `react=ignore` between scares.
- `@loot-spawner` on the heart chest for campaign artifacts.
- Custom `@trigger on_discovered` on hidden caches for your lore drops.

## See also

- [Expansions index](../README.md)
- [MODULES.md](../../../../../MODULES.md)