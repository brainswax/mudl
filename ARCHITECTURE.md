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

## Future Directions
- Full LLM content generation pipeline.
- Advanced self-modification (world rewriting its own rules).
- Multi-agent development support within the MUD itself.
- WebSocket/web client.
- Rich event system and procedural generation.
