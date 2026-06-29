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
@include creatures.mudl
@include players.mudl
@include items.mudl
@include objects.mudl
```

- `@include` paths are relative to the **world** directory.
- `@include-world <name>` loads `worlds/<name>/world.mudl` from the universe root.
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

## Items and Inventory (REPL)

Items are objects with `location` set to a place or player. The REPL supports basic pickup:

```
> create item boots
Created: boots (item:boots-001) at area:the-void-001
> look
The Void
...
You see: boots
> take boots
You take the boots.
> look self
Admin
You are holding boots in your right hand.
```

- `create` places new objects at the player's current location when one is set.
- `take` / `get` moves items from the current area/room into grasp slots from the player's `@creature` definition.
- Items may set `hand_slot` to `left`, `right`, or `both` (two-handed).

## Future Extensions
* LLM-friendly generation (clear grammar + examples in prompts).
* Meta-programming (objects modifying the language/runtime).
* Visual / procedural helpers.
* Import/export formats.
