# Object Model

This document defines the core data structures and rules that power the MUDL world.

**Status**: Draft (MVP)

## Philosophy
- **Everything is an Object** — rooms, items, players, NPCs, exits, abstract systems, even the world itself.
- **Composition over inheritance** — capabilities are **roles** (Container, Wearable, Creature, Stackable) stored as properties, composed at creation time via `ObjectFactory` or MUDL `MudlRoleProps`.
- **Prototype-based inheritance** — objects inherit from a parent (like classic MOO or JavaScript prototypes) for shared defaults and stackable item templates.
- **Runtime modifiable** — the world can add, change, or remove properties, verbs, and behaviors while running.
- **Secure by default** — all mutations go through the API Gateway + RBAC checks.

## Core Types

### 1. Object
The fundamental unit.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Object {
    pub id: ObjectId,                    // Unique identifier (e.g. "room:kitchen")
    pub name: String,
    pub aliases: Vec<String>,
    
    pub location: Option<ObjectId>,      // Where this object is (None for abstract)
    pub prototype: Option<ObjectId>,     // Parent for inheritance
    
    pub owner: ObjectId,                 // Who owns this (player or wizard)
    pub permissions: PermissionFlags,    // Who can modify
    
    pub properties: HashMap<String, Property>,
    pub verbs: HashMap<String, Verb>,
    pub event_handlers: HashMap<String, Vec<Behavior>>, // event name -> handlers
}
```

### 2. Property
Data + optional attached behavior.
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Property {
    pub name: String,
    pub value: Value,
    pub permissions: PermissionFlags,
    
    // Optional behavior (e.g. "bag_of_holding" effect)
    pub behavior: Option<Behavior>,
}
```

### 3. Verb (Behavior)
Executable action.
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Verb {
    pub name: String,
    pub code: String,                    // DSL source (or compiled form later)
    pub permissions: PermissionFlags,
    // Argument spec can be added later
}
```

### 4. Value (Property data)
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Value {
    String(String),
    Int(i64),
    Bool(bool),
    List(Vec<Value>),
    ObjectRef(ObjectId),
    Map(HashMap<String, Value>),
    // Future: Function, Timer, Custom types
}
```

### 5. Event Handlers
Simple mapping from event name to list of Behaviors (Verbs).
Common events: on_enter, on_say, on_use, on_tick, on_create.

### PermissionFlags
```rust
bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub struct PermissionFlags: u8 {
        const OWNER    = 1 << 0;
        const BUILDER  = 1 << 1;
        const WIZARD   = 1 << 2;
        const EVERYONE = 1 << 3;
    }
}
```

## Self-Modification API (exposed to DSL)
These functions are available inside verb/event code:
* add_property(obj, name, value, permissions)
* set_property(obj, name, value)
* add_verb(obj, name, code)
* add_event_handler(obj, event_name, code)
* set_prototype(obj, parent_id)
* Reflection helpers: list_properties(obj), list_verbs(obj), get_owner(obj)

## Examples
### Simple Room
```mudl
room "Cozy Kitchen" {
    description: "Warm and inviting..."
    owner: "player:neale"
    permissions: OWNER | BUILDER
}
```

### Property with Behavior
```mudl
property "bag_of_holding" on container {
    capacity: "infinite"
    on_add_item(item) {
        // custom logic
    }
}
```

### Inheritance
A magic sword that inherits from a generic weapon but adds a new verb.

## Permission Examples
* Regular player tries to modify someone else's room → denied (unless EVERYONE flag).
* Builder modifies a public area → allowed if BUILDER flag is set.
* Owner changes their own item → allowed.
* Wizard does anything → allowed.

### ObjectId

Every object has a unique, immutable internal identifier.

**Format**: `type:base-name-counter`

- `type`: Category (room, item, npc, exit, player, abstract, etc.)
- `base-name`: Slug derived from the **display name only** (lowercase, hyphenated, max **16 characters**). Creation options (`capacity=3`, etc.) are never included.
- `counter`: 3-digit hexadecimal (000–FFF). Automatically increments when duplicates are created. Extends to 4+ digits if needed.

**Display name vs ID base**: `create container purse capacity=3` sets `name = "purse"` and generates `item:purse-001`. Role parameters are stored as properties, not appended to the name or ID.

**Examples**:
- `room:cozy-kitchen-001`
- `item:purse-001` (from display name "purse")
- `item:silver-sword-00a`
- `npc:old-mage-0f3`
- `exit:north-042`
- `player:brains-007`

## Display and Presentation

To support both developer introspection and player-friendly interfaces:

### DisplayMode
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayMode {
    /// Clean, immersive output for normal play
    Player,
    /// Builder/wizard mode: shows ownership, properties, etc.
    Builder,
    /// Full internal dump (for debugging/coding)
    Debug,
}
```

### DisplayContext
```rust
#[derive(Debug, Clone)]
pub struct DisplayContext {
    /// Rendering mode
    pub mode: DisplayMode,
    /// Who is observing (for permission checks, personalization)
    pub observer: ObjectId,
    /// Recursion/detail level
    pub depth: u8,
    /// Additional flags (dark room, etc.)
    pub flags: DisplayFlags,
}

bitflags! {
    pub struct DisplayFlags: u32 {
        const DARK = 1 << 0;
        const BRIEF = 1 << 1;
        // etc.
    }
}
```

### Describable Trait
```rust
pub trait Describable {
    /// Basic description suitable for "look"
    fn describe(&self, ctx: &DisplayContext) -> String;

    /// Detailed view (exits, contents, properties)
    fn describe_detailed(&self, ctx: &DisplayContext) -> String;

    /// Full internal representation (Debug mode)
    fn dump(&self) -> String;
}
```

Implementations:
- **Room**: name + short_desc + obvious exits + visible contents
- **Player/Thing**: name + description, owner info in Builder mode
- Default fallback to property-based rendering.

## Composable Roles (Milestone 1)

Roles are property bundles applied via `Object::apply_*_role()` or `MudlRoleProps::apply_to()`. An object may hold multiple roles (e.g. a wearable backpack that is also a container).

| Role | Key properties | Factory helper |
|------|----------------|----------------|
| **Creature** | `creature`, `gender`, `body_slots` | `create_player` |
| **Container** | `is_container`, `contents`, `capacity`, `max_weight`, `max_volume` | `create_container`, `create_container_with_spec` |
| **Wearable** | `is_wearable`, `wear_slot`, `weight`, `volume` | `create_wearable` |
| **Stackable** | `stackable`, `stack_count`, `max_stack` | `create_stackable_item` |
| **Item (base)** | `weight`, `volume`, `is_pocketable`, `hand_slot` | `create_item` |

**LocationRef** (`src/object/location.rs`) types where an object resides:

```rust
pub enum LocationRef {
    Room(ObjectId),
    Inventory(ObjectId),
    Container(ObjectId, Option<String>),
    BodySlot(ObjectId, String),
    Nowhere,
}
```

**MoveManager** (`src/world/move_manager.rs`) implements `move_object(src, dst, obj)` with capacity, weight, and volume checks. **ContainerFit** (`src/world/container_fit.rs`) computes how many stackable units fit (by slot count, `max_weight`, and `max_volume` using `unit_weight * count`). Partial puts split stacks; compatible stacks merge by prototype or name. Inventory verbs delegate here. An `on_move` hook stub exists for future event triggers.

**Dirty tracking**: `DirtyTracker` records mutated object IDs; `persist_dirty()` saves only those rows.

## Anatomy and Inventory

Body plans are defined in MUDL (`creatures.mudl`) and loaded into an `AnatomyRegistry`. Players reference a plan via `creature` / `body_plan` and track occupancy in `body_slots` (a map of slot name → item ID, including worn items).

| Property | On | Type | Purpose |
|----------|-----|------|---------|
| `creature` | player | String | Loaded plan name (e.g. `human`) |
| `gender` | player | String | Description hint from player template |
| `body_slots` | player | Map\<String, ObjectRef\> | Occupied anatomical slots (grasp + wear) |
| `contents` | container | List\<ObjectRef\> | Items inside a container |
| `capacity` | container | Int | Max discrete items the container holds |
| `max_weight` | container | Int | Max total weight of contents (optional) |
| `max_volume` | container | Int | Max total volume of contents (optional) |
| `weight` | item | Int | Unit weight (× `stack_count` when stackable) |
| `volume` | item | Int | Unit volume (× `stack_count` when stackable) |
| `stackable` | item | Bool | Identical units collapse into one object |
| `stack_count` | item | Int | Number of identical units in a stack |
| `max_stack` | item | Int | Maximum stack size |
| `carried_slot` | item | String | Body slot name when held/worn |
| `is_wearable` | item | Bool | Can be worn on a `wear` slot |
| `wear_slot` | item | String | Target slot (e.g. `torso`, `head`) |
| `is_container` | item | Bool | Holds other items |
| `hand_slot` | item | String | `left`, `right`, or `both` (grasp slots) |

Default naked humans have **no pockets** — only biological `grasp`, `wear`, and `limb` slots from the human body plan. Pockets will come from clothing in a follow-up.

Factory helpers: `create_player`, `create_item`, `create_container`, `create_wearable`, `create_stackable_item`, `create_item_instances`. MUDL integration: `MudlRoleProps` in `src/mudl/roles.rs`. Move operations: `src/world/move_manager.rs`. Inventory commands: `src/inventory/mod.rs`.

## Persistence Notes
- Use ObjectFactory for creation.
- Serialize full objects for Debug; store key display fields for efficiency.

This design allows `look` to be player-friendly while `@examine`/`@dump` expose internals.

#### Why This Scheme?
- Human-readable for debugging and logging.
- Guarantees uniqueness even when many objects share the same name.
- Compact and sortable.
- Easy to generate and parse.

#### Creation
When an object is created, the engine automatically generates its ID using the `generate_id(type, base_name)` helper. The player only sees and uses the friendly name/aliases; the ID is used internally for references, persistence, and lookups.

#### Usage in Code / DSL
- Internal references always use the full ID.
- Players usually interact via name (the engine resolves contextually).
- Wizards/builders can use full IDs when needed for precision (`@examine room:cozy-kitchen-001`).

This design balances readability with the requirement that every object must have a stable, unique identity — especially important for a self-modifying world with possible duplicate names.

#### Creation
When an object is created, the engine automatically generates its ID using the `generate_id(type, base_name)` helper. The player only sees and uses the friendly name/aliases; the ID is used internally for references, persistence, and lookups.

#### Usage in Code / DSL
- Internal references always use the full ID.
- Players usually interact via name (the engine resolves contextually).
- Wizards/builders can use full IDs when needed for precision (`@examine room:cozy-kitchen-001`).

This design balances readability with the requirement that every object must have a stable, unique identity — especially important for a self-modifying world with possible duplicate names.
## Implementation Notes
* Objects are stored in a central WorldState (HashMap<ObjectId, Object> + spatial index for locations).
* Inheritance resolution is recursive with caching.
* All mutations are validated by the Gateway before reaching the engine.
* Serialization is straightforward via serde for persistence and GitHub export.
