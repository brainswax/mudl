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

## Future Extensions
* LLM-friendly generation (clear grammar + examples in prompts).
* Meta-programming (objects modifying the language/runtime).
* Visual / procedural helpers.
* Import/export formats.
