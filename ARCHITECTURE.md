# Architecture

**MUDL** (working name) — An IRC-first, programmable MUD/MOO with a custom domain-specific language (DSL), self-modifying world capabilities, and multi-modal authoring (IRC chat, REPL, files, GitHub).

**Status**: High-level design (MVP phase). This document will evolve as we implement.

## Vision
A living, collaborative text world where:
- Players interact via IRC (no special client needed).
- Builders program the world using a custom DSL.
- The world can modify itself at runtime.
- Content can be generated or extended by LLMs.
- All changes are version-controlled via GitHub.

The system emphasizes **separation of concerns**, extensibility, and safety (especially for live/LLM-generated code).

## High-Level Architecture
```
┌─────────────────────────────────────────────────────────────┐
│                    Frontends / Input Layers                  │
│  • IRC Bot (primary)                                         │
│  • CLI REPL / Interactive Prompt                             │
│  • File Loader (.mudl scripts)                               │
│  • GitHub Importer (raw files + webhooks)                    │
│  • Future: Web UI, LLM Generator                             │
└──────────────────────────────┬──────────────────────────────┘
                               │ (Commands + DSL snippets)
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                    API Gateway / Auth Layer                  │
│  • Authentication (nick, tokens, etc.)                       │
│  • Authorization (RBAC: Player / Builder / Wizard)           │
│  • Rate limiting, validation, auditing                       │
│  • Single point of entry for all world modifications         │
└──────────────────────────────┬──────────────────────────────┘
                               │ (Authorized calls)
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                      Core MUD Engine                         │
│  • World State Manager (objects, locations, inventory)       │
│  • Object Model + Prototype Inheritance                      │
│  • DSL Parser + Interpreter / Runtime                        │
│  • Command Dispatcher                                        │
│  • Event System + Scheduler (timers, hooks)                  │
│  • Persistence Layer (SQLite / JSON)                         │
└──────────────────────────────────────────────────────────────┘
```
**Key Principles**:
- Core Engine is pure (no knowledge of IRC or auth).
- All modifications go through the Gateway.
- Frontends are thin adapters.
- Self-modification and LLM generation are built on top of the DSL/runtime.

## Core Components

### 1. Object Model (Fundamental)
- Everything is an **Object** (rooms, items, players, NPCs, abstract concepts).
- **Composable roles** (not deep inheritance): `Container`, `Wearable`, `Creature`, `Stackable`, plus location types (`room`, `area`, …).
- `LocationRef` enum models the object graph: `Room`, `Inventory`, `Container`, `BodySlot`, `Nowhere`.
- Properties: key-value data with optional behaviors (`weight`, `volume`, `capacity`, `contents`, `body_slots`, …).
- Verbs/Behaviors: executable code attached to objects.
- Events/Hooks: `on_enter`, `on_say`, `on_use`, custom events; `MoveManager` stubs `on_move` for future triggers.
- Prototype/parent system for inheritance and stackable/identical items.

### 2. DSL Interpreter
- Custom language ("MUDL") designed for MUD concepts.
- Supports live execution, self-modification, and safe sandboxing.
- Parser: pest or chumsky (Rust).
- Runtime: Safe execution environment with restricted globals.

### 3. World State & Persistence
- In-memory graph of objects + locations.
- SQLite for durability (or JSON snapshots).
- Git-friendly export/import.

### 4. Event & Timer System
- Event-driven: objects can register handlers.
- Scheduler for delayed/recurring actions.

### 5. API Gateway / RBAC
- Enforces permissions before any state change.
- Roles: Player, Builder, Wizard (expandable).
- Logging and undo support for self-modification.

### 6. Frontends
- **IRC Bot**: Command parsing, world interaction, live DSL input.
- **REPL**: Development and testing.
- **Loaders**: File + GitHub integration.

## Data Flow Example (Player Command)
1. IRC Bot receives message → forwards to Gateway.
2. Gateway authenticates + authorizes.
3. Gateway calls Engine → dispatches to relevant Verb/Event.
4. Engine executes DSL code (sandboxed).
5. Results sent back through Gateway → IRC Bot.

## Self-Modification & Extensibility
- **Fundamental** (hard-coded in core): Objects, Properties, Verbs, Events, basic types.
- **Extensible** (defined in DSL): Property behaviors, custom events, new verb types, timers, LLM-generated content.
- Objects can add/remove properties/verbs at runtime.
- LLM integration will generate valid DSL that the runtime applies (with validation).

## Technology Stack (MVP)
- **Language**: Rust (performance, safety, modern ecosystem).
- **IRC**: `irc` crate or `ircbot`.
- **Parser**: pest or chumsky.
- **Persistence**: SQLite + serde.
- **Async**: Tokio.

## Repository Layout

```
mudl/
├── src/                    # Rust engine only
│   ├── object/             # Object model, roles, LocationRef, ObjectFactory
│   ├── mudl/               # MUDL parser, anatomy, role props, @include loader
│   ├── world/              # Bootstrap, MoveManager, dirty tracking, session
│   ├── command/            # Shared command/bootstrap helpers
│   ├── display/            # Player/builder/debug presentation
│   ├── inventory/          # Body-slot inventory (delegates to MoveManager)
│   ├── persistence/        # SQLite abstraction
│   └── bin/repl.rs         # Development REPL
├── modules/                # MUDL game data (not Rust)
│   └── default/            # Official baseline universe
│       ├── universe.mudl   # Universe entrypoint (@universe, @include-world)
│       └── worlds/
│           └── default_world/   # Flat MUDL files (no subfolders for now)
│               ├── world.mudl   # World entrypoint (@world, @include)
│               ├── map.mudl     # Areas/locations (type=area)
│               ├── creatures.mudl
│               ├── players.mudl
│               ├── items.mudl
│               └── objects.mudl
└── examples/               # Alternative universe packs
```

**MUDL-first**: All game content (creatures, map, templates) is defined in `.mudl` files. Rust provides loaders, runtime, and persistence — not hardcoded world data.

## Universe and World Hierarchy

A **Universe** is the top-level container. It holds one or more **Worlds**, each a self-contained game setting (locations, creatures, items, player templates).

```
Universe (modules/default/)
  └── World (worlds/default_world/)
        ├── world.mudl      entrypoint
        ├── map.mudl        areas and exits
        ├── creatures.mudl  @creature anatomy (slots)
        ├── players.mudl    @player-template (creature=human)
        ├── items.mudl      item prototypes
        └── objects.mudl    shared prototypes
```

**Flat layout (temporary)**: Each world keeps related definitions in a handful of sibling `.mudl` files. `world.mudl` `@include`s them explicitly. Nested subfolders (e.g. `locations/rooms/`) can return when content volume warrants it.

- `universe.mudl` declares the universe name, default world, and which worlds to load via `@include-world`.
- Each world's `world.mudl` declares `starting_location` and composes content with `@include` (paths relative to the world directory).
- `MUDL_WORLD` selects which world to bootstrap and play in; defaults to the universe's `default_world`.
- Locations default to `type=area`; bootstrap creates IDs like `area:the-void-001`.
- Players reference a creature via `creature=human` in `@player-template`; anatomy slots live in `@creature` blocks in `creatures.mudl`.

Custom worlds can fork `worlds/default_world/` and override individual flat files.

## Module Loading

1. Engine resolves `MUDL_MODULE` (default: `modules/default`) or `MUDL_UNIVERSE`.
2. `universe.mudl` is parsed; `@include-world` directives load each `worlds/<name>/world.mudl`.
3. World entrypoints use `@include` to pull anatomy, locations, players, etc. (relative to the world directory).
4. `bootstrap_world()` creates world objects and a naked human player from the active world's templates.
5. `bundle_module()` packages the universe tree + `manifest.json` for distribution.

## Customization and Prototype Inheritance

Builders/DMs can fork `modules/default/` to create custom universe packs:

- **Add worlds**: Create `worlds/my_campaign/world.mudl` plus flat `map.mudl`, `creatures.mudl`, etc., and add `@include-world my_campaign` to `universe.mudl`.
- **Swap creatures**: Change `creature=human` to `creature=cat` in `players.mudl` after defining `@creature cat` in `creatures.mudl`.
- **Override map**: Edit `map.mudl` or split into multiple files and `@include` them from `world.mudl`.
- **Inherit and override**: A custom world can `@include` another world's `creatures.mudl`, then add local overrides in additional included files.

The object model's prototype/parent system (`prototype: Option<ObjectId>`) is the runtime foundation for this — MUDL modules define the authoritative data; the engine resolves inheritance when spawning and displaying objects.

## Player Commands (REPL / MVP)

See **[COMMANDS.md](COMMANDS.md)** for the full command reference.

- **`create <type> <name> [key=value...]`** — Creates an object via `ObjectFactory`. The display name is parsed separately from options (`capacity=3`, `max_weight=10`, etc.); options become properties, not part of `name` or the ID slug. ID base names are slugified and capped at 16 characters (`purse` → `item:purse-001`). When the player has a current location, the new object is placed there automatically.
- **`take` / `get <item>`** — Picks up a visible item from the ground in the current location (carried items are excluded from target resolution). Uses grasp slots from the player's creature anatomy. One ground match takes silently; multiple ground matches disambiguate with short IDs. Failure messages: *"You don't see any X here."*, *"Your hands are full."*, etc.
- **`look`** — Short immersive view (`DisplayFlags::BRIEF`): name, description, container contents, room exits. **`look self`** → `You are holding: purse, coins. Wearing: backpack.` (no hand slots, weights, or nested contents).
- **`examine`** — In-game detail: weight, capacity, full grasp/worn summary on self.
- **`@examine`** / **`@dump`** — Wizard structured view / raw JSON.
- **`inventory`** — Full slot-by-slot listing (use `examine self` for weight totals).

### Command conventions (`@` meta-commands)

Player verbs have no prefix (`look`, `examine`, `take`, …). Wizard/builder meta-commands use a leading **`@`**:

| Player | Wizard |
|--------|--------|
| `examine coins` | `@examine coins` |
| `create sword …` | `@create container … capacity=3` |
| — | `@dump`, `@delete`, `@undelete` |

The parser (`src/command/parse.rs`) strips `@`, lowercases the verb, and routes to meta handlers after a permission check (`has_wizard_permission`, stubbed). Future meta-commands (`@set`, …) follow the same pattern.

**Target resolution** (`src/display/resolve.rs`) is centralized for `look`, `examine`, `get`, `put`, and related verbs:

1. Immediate possession (body slots)
2. Nested containers carried/worn by the player (BFS queue — no deep recursion)
3. Ground in the current room (player-owned first)
4. Global fallback (any active object)

Multiple matches in the same tier prompt disambiguation: `Which coins do you mean?` with lines like `coins-042 (in purse)`. Possession is searched before room scans to avoid full-world iteration.

Command helpers live in `src/command/`; inventory slot logic in `src/inventory/`; presentation in `src/display/` and `Object::is_location()`.

## Move Semantics

`MoveManager` (`src/world/move_manager.rs`) is the single authority for relocating objects:

- `move_object(src: LocationRef, dst: LocationRef, obj: ObjId)` validates source placement, checks destination capacity/weight/volume, updates holder lists (`contents`, `body_slots`), and fires `on_move` hooks.
- Inventory commands (`take`, `drop`, `put`, `remove`, `wear`) delegate to `MoveManager` convenience wrappers.
- `ObjectFactory::create_stackable_item` creates one instance with `stack_count`; `create_item_instances` spawns separate IDs for non-stacked duplicates.
- `put [count] <item> in <container>` transfers a specific stack quantity; omitting count moves as many units as fit (weight/volume/slots). Remainder stays carried with feedback (`5 won't fit.`).
- `look <container>` shows `Inside the purse: 20 coins` using stack-aware labels (`src/display/container.rs`).

## Persistence Strategy

All world state is stored in SQLite as JSON-serialized `Object` rows plus an ID counter table. New role fields (`weight`, `volume`, `max_weight`, `stack_count`, etc.) live inside the JSON blob — no schema migration required.

| When | What is saved |
|------|----------------|
| `ObjectFactory::create*` | New object immediately (`save_object`) |
| `create` / `create_at_location` / `@create` | Object + updated `location` |
| `take`, `drop`, `put`, `remove`, `wield`, `wear` | Full active object graph after mutation (`persist_all`); `DirtyTracker` + `persist_dirty` available for incremental saves |
| `go` | Player `location` |
| `add_prop`, `add_verb`, `save` | Target object |
| Bootstrap | World areas, exits, default player (idempotent) |

**Startup**: `bootstrap_world()` ensures MUDL-defined content exists, then `restore_session()` hydrates all active objects from the DB and restores the player's `current_location` from their persisted `location` field.

**Soft deletes**: Objects are never hard-deleted. `is_deleted` and `deleted_at` on `Object` mark removal; `list_objects(false)` hides them from normal play. Wizard commands `@delete <target>` and `@undelete <id>` toggle the flag. Deleted objects remain loadable by ID for recovery.

**Schema**: `objects(id, data, is_deleted, deleted_at)` and `counters(type_base, counter)`. Older DB files are migrated with `ALTER TABLE` on connect.

## Future Directions
- Full LLM content generation pipeline.
- Advanced self-modification (world rewriting its own rules).
- Multi-agent development support within the MUD itself.
- WebSocket/web client.
- Rich event system and procedural generation.
