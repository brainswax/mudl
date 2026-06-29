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
- Properties: key-value data with optional behaviors.
- Verbs/Behaviors: executable code attached to objects.
- Events/Hooks: `on_enter`, `on_say`, `on_use`, custom events.
- Prototype/parent system for inheritance.

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
│   ├── object/             # Object model + ObjectFactory
│   ├── mudl/               # MUDL parser, anatomy, @include loader
│   ├── world/              # Module bootstrap + packaging
│   ├── command/            # Shared command/bootstrap helpers
│   ├── display/            # Player/builder/debug presentation
│   ├── inventory/          # Body-slot inventory operations
│   ├── persistence/        # SQLite abstraction
│   └── bin/repl.rs         # Development REPL
├── modules/                # MUDL game data (not Rust)
│   └── default/            # Official baseline universe
│       ├── universe.mudl   # Universe entrypoint (@universe, @include-world)
│       └── worlds/
│           └── default_world/
│               ├── world.mudl       # World entrypoint (@world, @include)
│               ├── anatomy/         # Body plans (human, etc.)
│               ├── players/         # Player spawn templates
│               ├── locations/       # Rooms and areas
│               ├── creatures/       # NPC/creature templates (future)
│               ├── items/           # Item definitions (future)
│               └── objects/         # Shared object prototypes (future)
└── examples/               # Alternative universe packs
```

**MUDL-first**: All game content (anatomy, rooms, templates) is defined in `.mudl` files. Rust provides loaders, runtime, and persistence — not hardcoded world data.

## Universe and World Hierarchy

A **Universe** is the top-level container. It holds one or more **Worlds**, each a self-contained game setting (locations, creatures, items, anatomy, player templates).

```
Universe (modules/default/)
  └── World (worlds/default_world/)
        ├── locations/   rooms, areas
        ├── anatomy/     body plans
        ├── players/     spawn templates
        ├── creatures/   NPC definitions
        ├── items/       item prototypes
        └── objects/     shared prototypes
```

- `universe.mudl` declares the universe name, default world, and which worlds to load via `@include-world`.
- Each world's `world.mudl` declares `starting_location` and composes content with `@include` (paths relative to the world root).
- `MUDL_WORLD` selects which world to bootstrap and play in; defaults to the universe's `default_world`.

Custom worlds can override defaults by forking `worlds/default_world/` or defining a new world that `@include`s shared anatomy/locations and replaces specific files.

## Module Loading

1. Engine resolves `MUDL_MODULE` (default: `modules/default`) or `MUDL_UNIVERSE`.
2. `universe.mudl` is parsed; `@include-world` directives load each `worlds/<name>/world.mudl`.
3. World entrypoints use `@include` to pull anatomy, locations, players, etc. (relative to the world directory).
4. `bootstrap_world()` creates world objects and a naked human player from the active world's templates.
5. `bundle_module()` packages the universe tree + `manifest.json` for distribution.

## Customization and Prototype Inheritance

Builders/DMs can fork `modules/default/` to create custom universe packs:

- **Add worlds**: Create `worlds/my_campaign/world.mudl` and add `@include-world my_campaign` to `universe.mudl`.
- **Swap body plans**: Change `body_plan=human` to `body_plan=cat` in a player template after defining `@body-plan cat` in that world's `anatomy/`.
- **Override locations**: Replace `locations/world_locations.mudl` or split rooms into `locations/rooms/` and `@include` them from `world.mudl`.
- **Inherit and override**: A custom world can `@include` the default world's anatomy file, then add a local override file loaded afterward.

The object model's prototype/parent system (`prototype: Option<ObjectId>`) is the runtime foundation for this — MUDL modules define the authoritative data; the engine resolves inheritance when spawning and displaying objects.

## Future Directions
- Full LLM content generation pipeline.
- Advanced self-modification (world rewriting its own rules).
- Multi-agent development support within the MUD itself.
- WebSocket/web client.
- Rich event system and procedural generation.
