# Language Specification (MUDL)

**MUDL** — The domain-specific language for building programmable, self-modifying MUD/MOO worlds in this project.

**Status**: Draft / MVP skeleton. This is a living document that will evolve with the interpreter.

## Goals
- Accessible to non-programmers (builders) while powerful enough for complex behaviors.
- First-class support for MUD concepts: rooms, objects, verbs (actions), events, persistence.
- Safe for live execution (IRC, LLM-generated code).
- Extensible and self-modifying — the world can add new behaviors at runtime.
- Easy to serialize (files, GitHub) and generate (LLM).

## Core Concepts

### 1. Everything is an Object
- Rooms, items, players, NPCs, exits, even abstract systems.
- Objects have:
  - Unique ID
  - Name + aliases
  - Location (or null)
  - Prototype / parent (inheritance)
  - Owner + permissions
  - Properties
  - Verbs / behaviors
  - Event handlers

### 2. Properties
Key-value data attached to objects.

**Syntax sketch**:
```mudl
property "description" on room {
    value: "A cozy kitchen..."
    type: string
}

property "bag_of_holding" on container {
    capacity: infinite
    weightless: true
   
    on_add_item(item) {
        // behavior code
    }
}
```
### 3. Verbs / BehaviorsExecutable actions attached to objects.

```mudl
verb "bake bread" on kitchen {
    requires: ["flour", "yeast"]
    
    execute(player) {
        if (player.has("flour")) {
            say("You bake a fresh loaf!")
            create item "fresh bread" in this
        } else {
            say("Missing ingredients...")
        }
    }
}
```
### 4. Events & HooksNamed triggers that run code.Built-in examples: on_enter, on_say, on_use, on_tick.Custom events can be defined.
```mudl
on_enter(room) {
    if (random(10) > 7) {
        say("The fire crackles warmly.")
    }
}
```
### 5. Built-in Primitives
* say(msg), tell(player, msg)
* move(thing, destination)
* create object ...
* get_property(obj, name), set_property(...)
* add_verb(obj, name, code)
* Reflection: list_properties(obj), list_verbs(obj)

## Fundamental vs Extensible
Fundamental (implemented in core engine):
* Object model
* Basic property/verb/event system
* Core types (string, int, bool, list, object ref)
* Sandboxed execution environment
* Persistence hooks

Extensible (defined in MUDL):
* New property behaviors
* Custom events
* Timers / scheduled actions
* LLM-generated content
* Self-modifying logic

## Syntax Philosophy
* Readable, English-like where possible.
* Support both declarative (for builders) and imperative (for complex logic) styles.
* Multi-line friendly for files/GitHub.
* Compact prefixes for live IRC input (e.g. !verb ...)

## Example Full Object

```mudl
room "Cozy Kitchen" {
    description: "Warm and inviting, with the smell of fresh bread."
    
    exit north to "Living Room"
    
    object "oven" {
        verb "bake" {
            // implementation
        }
    }
    
    on_enter {
        // welcome message
    }
}
```
## Safety & Sandboxing
* All user/LLM code runs in a restricted environment.
* No direct file/system access.
* Permission checks enforced by the Gateway.
* Resource limits (CPU, loops, memory).

## Universes, Worlds, and Entrypoints

Game content lives under `modules/<universe>/`. A universe holds one or more worlds:

```mudl
# universe.mudl
@universe default
  default_world=default_world
@end
@include-world default_world
```

Each world uses a flat set of `.mudl` files under `worlds/<name>/`, composed from `world.mudl`:

```mudl
# worlds/default_world/world.mudl
@world default_world
  starting_location=the-void
@end
@include map.mudl
@import expansions/haunted_forest.mudl
@include creatures.mudl
@include players.mudl
@include items.mudl
@include objects.mudl
```

- `@include` paths are relative to the **world** directory (built-in content shipped with the world).
- `@import` loads expansion packs from a **local path** or **URL** (fetched at load time). Resolution order for relative paths: directory of the importing file → world root → universe root. Supports `http://`, `https://`, and `file://` URLs.
- `@include-world <name>` loads `worlds/<name>/world.mudl` from the universe root.

### Expansion packs

Self-contained `.mudl` files bundle areas, items, and hooks for drop-in world extension. Tag them with `@expansion` metadata:

```mudl
@expansion haunted_forest
  name=Haunted Forest
  version=1
  integrates=forest-path,the-void,scene-chest
@end
```

Import from `world.mudl`:

```mudl
@import expansions/haunted_forest.mudl
@import https://example.com/mudl/expansions/haunted_forest.mudl
```

The host world keeps minimal map hooks (e.g. `forest-path` with `in: haunted-entry`); the expansion places items and defines puzzle areas.

Places may set `loop_to: <base_name>` so entering that room silently returns the player to another place (no movement message). Useful for maze wrong turns.
- Set `MUDL_MODULE=modules/default` (or `MUDL_UNIVERSE` to a specific file) to load a universe.
- Set `MUDL_WORLD=<name>` to select which world to bootstrap (defaults to the universe's `default_world`).

Fork `modules/default/` to add custom worlds — e.g. a feline campaign with `creature=cat` in `players.mudl`.

## Creatures and Anatomy

Creature anatomy is defined in `creatures.mudl` via `@creature` blocks. Player templates in `players.mudl` reference a creature:

```mudl
@creature human
  @slot left_hand capacity=1 type=grasp hands=1
  @slot right_hand capacity=1 type=grasp hands=1
  @slot head capacity=1 type=wear
  @slot torso capacity=1 type=wear
@end
```

```mudl
@player-template default
  creature=human
  gender=neutral
@end
```

**Creature vitals and stats** (Milestone 3):

```mudl
@creature human
  max_health=100
  base_max_weight=90
  @stat strength 10
  @skill survival 0
  @slot left_hand capacity=1 type=grasp hands=1
@end

@effect weary
  mod_encumbrance=1.1
  mod_max_weight=-5
  mod_stat_dexterity=-2
@end
```

- `base_max_weight` plus `strength` sets starting carry capacity at bootstrap.
- `@effect` defines reusable conditions; creatures track `active_effects` at runtime.
- `@slot` may set `effect=` for slot-tagged body-plan conditions (future wound hooks).

**Equipment modifiers** (wearable and wielded gear stack):

```mudl
@prototype chipped-blade
  hand_slot=right
  @mod-stat strength 2
@end

@prototype leather-vest
  is_wearable=true
  wear_slot=torso
  mod_max_health=5
  @mod-stat constitution 2
  @mod-skill survival 1
@end

@prototype boots-of-carrying
  is_wearable=true
  wear_slot=left_foot
  mod_max_weight=25
  mod_encumbrance=0.85
@end

@prototype iron-lantern
  hand_slot=right
  @grant-effect iron_lantern_aura
@end

@effect iron_lantern_aura
  mod_encumbrance=0.95
@end

@effect regeneration
  regen_on_enter=2
@end
```

- `@mod-stat` / `@mod-skill` — additive bonuses while equipped (worn or wielded in grasp slots).
- `mod_max_weight`, `mod_encumbrance`, `mod_max_health` — carry capacity, encumbrance feel, and health ceiling.
- `@grant-effect` — apply a defined `@effect` while the item is equipped (regeneration, auras, etc.).
- Modifiers from multiple worn items **stack**; granted effects compose with direct item bonuses.

**Behavior templates** (reusable, composable personalities):

```mudl
@behavior-template guard
  react=warn
  on_enter=say Halt! Who goes there?
@end

@behavior-template aggressive
  react=attack
  on_enter=say You should not have come here.
  attack_damage=12
@end

@behavior-template skittish
  react=flee
  on_enter=emote scrambles away from you.
@end

@behavior-template wanderer
  react=wander
  wander_interval=3
  on_enter=emote paces the area restlessly.
@end
```

- `react` — how the creature responds when a player enters: `ignore`/`passive`, `warn`/`guard`, `attack`/`aggressive`, `flee`/`skittish`, `wander`/`roam`.
- `on_enter` — optional scripted line (`say`, `emote`, `say_to`) fired alongside the react.
- `attack_damage` — damage dealt on `attack` react (default 8).
- `wander_interval` — emote every N player entries for `wander` react (default 3).

Creatures support **multiple simultaneous behaviors** — combine `@use-behavior` templates with inline `@behavior` scripts for unique personalities.

**NPCs and behaviors**:

```mudl
@npc path-watcher
  name=Path Watcher
  creature=human
  location=forest-path
  @use-behavior guard
  @behavior on_enter say The trees seem to lean closer when you pass.
@end
```

Supported script actions: `say`, `say_to`, `emote`. `on_enter` runs when a player enters the NPC's room.

Builders can attach templates at runtime: `@addbehavior <creature> <template>`, `@listbehaviors <creature>`.

**Creature spawners** (locations only spawn randomly when a spawner is attached):

```mudl
@spawn-template mist-wisp
  name=Mist Wisp
  creature=human
  @use-behavior wanderer
  @behavior on_enter emote drifts through the air.
@end

@spawner haunted-moon-phantoms
  location=haunted-moon
  trigger=on_enter
  chance=0.7
  max_active=1
  @entry mist-wisp weight=3
  @entry pale-lurker weight=1
@end
```

- `trigger=on_enter` — roll on each player entry; `trigger=periodic` with `periodic_interval=N` — every Nth entry.
- `chance` — spawn attempt probability (0.0–1.0). `max_active` — cap concurrent spawned creatures per spawner.
- No spawner on a location → no random spawns (only explicit `@npc` or MUDL-placed creatures).

**Slot types** (MVP):
- `grasp` — hands; items with `hand_slot: left`, `right`, or `both` occupy these
- `wear` — clothing/armor/containers worn on the body
- `limb` — biological parts (descriptive; not used for inventory yet)
- `pocket` / `container` — reserved for clothing-provided capacity (future)

**Player properties** (set by engine from template):
- `creature` — name of the loaded creature definition (e.g. `human`)
- `gender` — for descriptions (`neutral`, `male`, `female`, etc.)
- `body_slots` — map of slot name → held/worn item ID

`@body-plan` and `body_plan=` are accepted as aliases during migration. Default players are **naked humans**: biological slots only, no pockets or clothing until equipped.

## Map and Locations

Locations are defined in `map.mudl`. Default locations use `type=area`:

```mudl
type: area
base_name: the-void
name: The Void
description: You are in a featureless void.

exits:
  north: north-passage
```

## Player-Facing Output

MUDL separates **what the world knows** from **what players read**. The engine tracks stable object IDs, types, and JSON state internally; frontends render MOO-style narrative text.

### Three display tiers

| Tier | Audience | Commands | Shows |
|------|----------|----------|-------|
| **Player** | Everyone playing | `look`, `take`, `create`, `go`, `inventory`, … | Immersive prose only — names, descriptions, exits, natural inventory |
| **Builder** | World authors | `examine`, `add_prop`, `add_verb`, `load`, `save`, … | Contextual detail — owners, properties, verbs, exit *names* (not raw IDs) |
| **Debug** | Engine developers | `@dump`, logs (`RUST_LOG`) | Full JSON, IDs, persistence paths, bootstrap diagnostics |

**Rules:**

- Player commands never print raw IDs, type prefixes, or struct dumps.
- Builder feedback uses in-world phrasing where possible (`You weave … into being`, `You inscribe … upon …`) while remaining informative.
- Technical details go to `tracing` logs, not the REPL prompt or future IRC channel.
- Future MUDL verbs may override default messages per object or command via properties / event hooks.

### Example session (player tier)

```text
> create sword Rusty Sword
You forge a Rusty Sword, and it clatters to the ground in The Void.
> look
The Void
You are in a featureless void.
You see: Rusty Sword
> take rusty sword
You pick up the Rusty Sword.
> look self
Admin
You are completely naked.
You are holding Rusty Sword in your right hand.
> inventory
You are completely naked.
You are carrying:
  Rusty Sword — in your right hand
```

Object IDs still exist internally (`Rusty Sword` → `sword:rusty-sword-001`); use `@dump` or `RUST_LOG=info` when you need them.

## Items and Inventory (REPL)

Items are objects with `location` set to a place or player. The REPL supports basic pickup:

- `create <type> <name...>` — everything after the type is the display name (spaces allowed). Quoted names work: `create sword "Rusty Sword"`.
- Object IDs use lowercase hyphenated slugs derived from the name (`Rusty Sword` → `sword:rusty-sword-001`). Display names keep original capitalization.
- `create` places new objects at your current location when one is set.
- `take` / `get` moves items from the ground in your current location into grasp slots. Items you already carry are ignored when resolving the target, so `take sword` picks up a ground sword even if you're holding another.
- Items may set `hand_slot` to `left`, `right`, or `both` (two-handed).

## Persistence

Every `Object` is stored as JSON in SQLite. State changes from `take`, `drop`, `go`, and `create` are saved immediately. Objects are never hard-deleted — wizard `@delete` sets `is_deleted` and `@undelete <id>` restores them.

## Future Extensions
* LLM-friendly generation (clear grammar + examples in prompts).
* Meta-programming (objects modifying the language/runtime).
* Visual / procedural helpers.
* Import/export formats.
