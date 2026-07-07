# Poisonous Swamp

**File:** [../poisonous_swamp.mudl](../poisonous_swamp.mudl) · **Expansion ID:** `poisonous_swamp`

## Teaser

A stinking sink of bitter fen, sweet reeds, and black water that keeps what it steals. Survival-horror adjacent — green fog, leeches, gas pockets, and a warden at the deep heart. The bog teaches through pressure; antidotes and masks are worth finding.

**Danger:** High (environmental damage, creatures, boss fight). **Spoilers:** None below.

## Install

### Import from GitHub

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/poisonous_swamp.mudl
```

### Host map (minimum)

```mudl
type: area
base_name: forest-path
name: Forest Path
description: A path into the trees. The ground sounds hollow to the east.
exits:
  down: swamp-entry
exit_returns:
  down: up
```

The pack places a **warning post** on `forest-path` and defines all `swamp-*` areas.

### Optional host

| Hook | Purpose |
|------|---------|
| `the-void` | Scatter exit from swamp heart can land here |
| `swamp-dry` exit `in: spider-entry` | Links to Giant Spider Den when that pack is loaded |

### Link at runtime (wizard)

```text
> go forest-path
> @link down swamp-entry --return up
```

### Play (default world)

```text
> go north
> look
> read warning
> go down
> look
> examine marker
> harvest root
```

Carry salves when you find them; `examine` stakes and signs; avoid rushing — wrong turns loop to the threshold.

## Extension ideas

- `@effect` mudwalking boots reducing `on_enter` damage in gas rooms.
- `@resource-spawner` new harvestables for an alchemy chain.
- Replace warden with `react=warn` NPC for a pacifist variant.
- Point `swamp-dry` `in` at your own dungeon alongside the spider den.

## See also

- [Expansions index](../README.md)
- [Giant Spider Den](../giant_spider_den/README.md) (optional sequel)
- [MODULES.md](../../../../../MODULES.md)