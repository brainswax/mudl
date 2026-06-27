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
