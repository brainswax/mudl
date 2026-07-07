# Giant Spider Den

**File:** [../giant_spider_den.mudl](../giant_spider_den.mudl) · **Expansion ID:** `giant_spider_den`

## Teaser

A Shelob-inspired brood crawl — silk galleries, egg chambers, iron gates, and silence that is never empty. Claustrophobic and predatory. Best after the swamp when you want a dungeon crescendo. Light, keys, and stealth matter as much as steel.

**Danger:** High (ambush lurkers, boss queen). **Spoilers:** None below.

## Install

### Import from GitHub

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/giant_spider_den.mudl
```

### Host map — via swamp (recommended)

Load [Poisonous Swamp](../poisonous_swamp/README.md) first. The den places a **webbed fissure** on `swamp-dry`:

```mudl
type: area
base_name: swamp-dry
exits:
  in: spider-entry
exit_returns:
  in: out
```

### Standalone (custom entry)

Create any host area and link or portal to `spider-entry`:

```mudl
@item my-spider-fissure
  prototype=webbed-fissure
  location=my-cave-mouth
  door_direction=in
  door_destination=spider-entry
@end
```

```text
> @link my-cave-mouth in spider-entry --return out
```

### Play (default world)

```mudl
@import .../poisonous_swamp.mudl
@import .../giant_spider_den.mudl
```

```text
> go north
> go down
> … reach swamp dry mound …
> go in
> look
> read plaque
> examine marker
```

Wield a lantern; `attack` when broodlings discover you; `examine` cocoons and swellable objects carefully.

## Extension ideas

- `@prototype` torch granting `starlight_ward` while wielded.
- Harmless `@spawn-template` cave moths for atmosphere.
- Crown exit branches into your megadungeon via `@link`.
- `@trigger on_kill` on the queen opens a gate elsewhere.

## See also

- [Poisonous Swamp](../poisonous_swamp/README.md)
- [Expansions index](../README.md)
- [MODULES.md](../../../../../MODULES.md)