# Sandy Shoals Resort

**File:** [../beach_resort.mudl](../beach_resort.mudl) · **Expansion ID:** `beach_resort`

## Teaser

Whimsical tropical resort — tiki bar, tidepools, hammocks, and a pearl inlet where the surf forgets to rush. Restorative and slightly silly. No combat; cocktails grant layered **drunk effects** that change NPC banter and room narration. A palate cleanser between harder modules.

**Danger:** None. **Spoilers:** None below.

## Install

### Import from GitHub

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/beach_resort.mudl
```

### Host map (minimum)

```mudl
type: area
base_name: the-void
name: West Clearing
description: A clearing at the edge of nowhere.
exits:
  south: beach-trail
exit_returns:
  south: north
```

The pack places a **resort trail sign** on `the-void` and defines all `beach-*` areas. Use any hub `base_name` if you edit the sign's `@item location=`.

### Link at runtime (wizard)

```text
> go the-void
> @link south beach-trail --return north
```

### Play (default world)

```text
> look
> read sign
> go south
> look
> examine marker
> go east
> take spritz
> go south
```

Try `take` / `drink` cocktails at the bar; `harvest` shells and tidepool nodes; `examine` sand and ledges for hidden finds; revisit rooms while tipsy for different lines.

## Extension ideas

- `@schedule` sunset `on_weather` on the open shore.
- Mocktail `@prototype` items with custom `@effect` instead of drunk stacks.
- `@npc` surf instructor granting `mod-skill survival` on `on_enter`.
- Retarget pearl inlet `scatter_to` to your coastal city.

## See also

- [Glimmerfen](../fey_glade/README.md) (beach shore portal into the fey vale)
- [Expansions index](../README.md)
- [MODULES.md](../../../../../MODULES.md)