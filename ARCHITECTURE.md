# Architecture

**MUDL** (working name) — An IRC-first, programmable MUD/MOO with a custom domain-specific language (DSL), self-modifying world capabilities, and multi-modal authoring (IRC chat, REPL, files, GitHub).

**Status**: High-level design + **Milestones 1–3 implemented**, **Milestone 4 (Events & Triggers) in progress**. ~401 unit tests. This document tracks target architecture, as-built state per milestone, and consolidation priorities for M4+.

## Milestone Summary (as-built)

| Milestone | Delivered | Primary modules |
|-----------|-----------|-----------------|
| **M1** | Object graph, `MoveManager`, inventory verbs, SQLite roundtrip, REPL `Session` | `object/`, `inventory/`, `display/`, `persistence/`, `world/move_manager` |
| **M2** | MUDL loader, bootstrap pipeline, map/items/NPCs, `@dig`/`@link`, expansion packs | `mudl/`, `world/bootstrap`, `world/place_builder` |
| **M3** | Creature vitals/stats/effects, equipment modifiers, combat/death, behaviors, awareness, spawners, loot | `creature/`, `loot/` |
| **M4** (partial) | `@trigger` on places/objects/NPCs/spawn-templates; `execute_event` script bus | `world/events`, `world/event_script` |

## Vision
A living, collaborative text world where:
- Players interact via IRC (no special client needed).
- Builders program the world using a custom DSL.
- The world can modify itself at runtime.
- Content can be generated or extended by LLMs.
- All changes are version-controlled via GitHub.

The system emphasizes **separation of concerns**, extensibility, and safety (especially for live/LLM-generated code).

## Milestone 1 — As Built (2026)

The diagram below shows **actual** module dependencies today (solid = implemented, dashed = planned).

```
┌─────────────────────────────────────────────────────────────────────────┐
│  Frontends: REPL (src/bin/repl.rs)          IRC / Gateway (planned)    │
└───────────────────────────────┬─────────────────────────────────────────┘
                                │ repl::Session (graph, location, anatomy)
                                ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  Command layer (src/command/) — parse, @meta, @set/@unset, @dig         │
└───────┬─────────────────────────────┬───────────────────────────────────┘
        │                             │
        ▼                             ▼
┌───────────────────┐       ┌─────────────────────────────────────────────┐
│ Inventory         │       │ Display (src/display/)                      │
│ take/drop/break/  │──────▶│ resolve, look/examine, combat/creature text │
│ unlock/open       │       │                                             │
└─────────┬─────────┘       └─────────────────────────────────────────────┘
          │ delegates                    ▲
          ▼                                │ narrative lines
┌─────────────────────────────────────────────────────────────────────────┐
│  MoveManager — single authority for moves + on_move @trigger hooks      │
│  possession, stack_transfer, portals/doors/exits                        │
└─────────┬───────────────────────────────┬───────────────────────────────┘
          │                               │
          ▼                               ▼
┌─────────────────────┐         ┌─────────────────────────────────────────┐
│ Object model        │         │ MUDL loader + parsers (src/mudl/)         │
│ roles, factory      │◀────────│ map, items, npcs, behaviors, spawners,  │
│ event_handlers      │         │ loot-spawners, triggers, expansions     │
└─────────┬───────────┘         └─────────────────────────────────────────┘
          │                               │
          ▼                               ▼
┌─────────────────────┐         ┌─────────────────────────────────────────┐
│ Creature (M3)       │         │ Events (M4 partial)                       │
│ combat, behavior,   │         │ execute_event / event_script            │
│ tactics, spawner    │         │ event_subscribers (spawner/loot bus)    │
└─────────┬───────────┘         └─────────────────────────────────────────┘
          │
          ▼
┌─────────────────────┐         ┌─────────────────────────────────────────┐
│ Loot spawners (M3)  │         │ Persistence → SqlitePersistence         │
└─────────────────────┘         │ hydrate_world / DirtyTracker            │
                                └─────────────────────────────────────────┘
```

### M1 strengths

| Area | What works well |
|------|-----------------|
| **Movement** | `MoveManager` owns validation, stack merge/split, capacity/weight/volume; `move_to_grasp` / `possession` handle hand placement |
| **Roles** | Composable properties (`is_container`, `stackable`, `body_slots`, …) + `MudlRoleProps` bridge |
| **Anatomy** | Creature slots loaded from MUDL; grasp/wear resolution uses `BodyPlan` |
| **Persistence** | Full JSON roundtrip verified; complex graphs (containers, stacks, slots) reload identically |
| **Factory** | `ObjectFactory<P: Persistence>` abstracts creation + ID counters |
| **Presentation** | Clean split: player (`look`) vs builder (`@examine`); centralized `resolve_object` |

### M1 known gaps (carried forward)

- **`object` → `display` coupling** (`Describable` on `Object`) — core imports presentation
- **No gateway or multi-user session isolation** yet (IRC needs per-connection `Session` registry)
- **Graph invariants** (`location`, `contents`, `body_slots`) enforced by ad-hoc prune/clear, not a single validator
- **`DirtyTracker`** exists; REPL uses incremental persist but some paths still call `persist_all`

## Milestone 2 — As Built (MUDL world bootstrap)

M2 makes game content **MUDL-first**: universes, worlds, flat file includes, and idempotent bootstrap.

| Area | What works |
|------|------------|
| **Loader** | `load_universe` / `load_module` composes `LoadedWorld` from `@include`, `@import`, `@expansion` |
| **Map** | Legacy `type: area` blocks + exits, aliases, scatter/loop; `@trigger` on places |
| **Items** | `@prototype` / `@item` with `MudlRoleProps` (containers, keys, doors, breakables, wearables) |
| **Bootstrap** | `bootstrap_world()` — places → items → NPCs → spawners → loot; exit graph validation |
| **Place builder** | `@dig`, `@link`, `@unlink` via `place_builder` + `Session` |
| **Expansions** | Self-contained packs (e.g. `haunted_forest.mudl`) hook host-world locations |

**Hard-coded in Rust (acceptable for now):** default admin player ID/name, `{type}:{base}-001` ID scheme, exit validation rules.

## Milestone 3 — As Built (creatures & combat)

M3 adds living creatures with MUDL-defined personalities, weighted spawns, and turn-based combat.

| Area | What works |
|------|------------|
| **Vitality** | `@stat`, `@skill`, `@effect`, health, encumbrance, equipment regen |
| **Behaviors** | `@behavior-template`, `@use-behavior` → tactics in `creature_behaviors`; scripts via `@trigger` |
| **Awareness** | Bilateral stealth/perception on enter; hidden lurkers; ambush/surprise damage |
| **Combat** | `attack <npc>`, initiative, crits, counter-attack, corpses, player respawn at `home_location` |
| **Spawners** | `@spawn-template` / `@spawner` (on_enter, periodic) — hidden `is_spawner` objects |
| **Loot** | `@loot-spawner` (on_enter, on_open, on_kill, on_break, timer) — separate dispatch |

**Hybrid (MUDL inputs, Rust formulas):** damage mitigation, surprise/crit thresholds, initiative contests, XP curves. Documented in `LANGUAGE.md`; candidates for `@formula` or data tables later.

## Milestone 4 — In Progress (events & triggers)

M4 introduces a builder-facing **`@trigger`** system on places, objects, NPCs, and spawn-templates.

| Component | Role |
|-----------|------|
| `trigger_def.rs` | Parse `@trigger <event> <action> [text…]` |
| `events.rs` | `EventContext`, `attach_triggers`, `execute_event` |
| `event_script.rs` | Script actions: `narrate`, `emote`, `react`, `damage`, `heal`, `mod-stat`, `teleport`, `spawn` |
| **Wired events** | `on_enter`/`on_leave` (movement), `on_take`/`on_drop`/`on_move` (inventory), `on_break`, `on_death`/`on_kill` (combat), `on_discovered` (perception + triggers), `on_unlock`/`on_open` (gates, narrative-only) |

**Room entry order** (`Session::go`):

```
portal prep → on_leave (place) → move player → spawners → loot spawners
  → on_enter (place @trigger) → creature behaviors (on_enter)
  → room look → equipment regen
```

## Hard-coded vs MUDL-driven

| Concern | MUDL-driven | Engine hard-coded |
|---------|-------------|-------------------|
| Map, exits, scatter/loop | `map.mudl`, expansions | Exit reciprocity validation |
| Items, prototypes | `items.mudl`, `objects.mudl` | Role defaults, weight math |
| Creature anatomy/stats | `creatures.mudl`, `@effect` | Constitution→health scaling |
| NPC placement | `npcs.mudl` | — |
| AI personalities | `behaviors.mudl`, `@use-behavior` | React execution (flee, attack, wander) |
| Spawns / loot tables | `@spawner`, `@loot-spawner` | Weighted pick, chance rolls, counters |
| Place/object scripts | `@trigger` → `event_handlers` | `event_script` action interpreter |
| Combat feel | `attack_damage`, stats, gear | Damage formula, crit/surprise rules |
| Default player | `players.mudl` template | Admin player bootstrap, naked respawn |

**Principle:** World *content* and *reactions* belong in MUDL; *physics* (movement rules, combat math, awareness contests) stays in Rust until a sandboxed DSL runtime exists.

## Architectural Review — Strengths (M1–M3)

1. **Single move authority** — `MoveManager` + `LocationRef` keep the object graph coherent; inventory verbs delegate correctly.
2. **MUDL-first bootstrap** — No hardcoded world geography; haunted forest is a drop-in expansion, not a Rust fork.
3. **Composable roles** — Containers, wearables, portals, breakables stack via properties; `MudlRoleProps` bridges parser → factory.
4. **Session as play authority** — `repl::Session` owns graph + dirty state; movement orchestrates spawners, loot, triggers, and behaviors in one place.
5. **Presentation split** — Player (`look`) vs builder (`@examine`) vs debug (`@dump`) is clean and extensible.
6. **Test coverage** — Integration tests exercise full bootstrap → play → combat → persist paths (haunted forest adventure, path watcher kill loot).

## Architectural Review — Anti-patterns & Gaps (roll into M4+)

### 1. Dual scripting buses — **resolved (M4)**

Creatures now use a **single script surface** with split storage:

| Layer | Storage | Syntax | Executor |
|-------|---------|--------|----------|
| **Scripts** (say, emote, narrate, react via trigger) | `event_handlers` map | `@trigger` (preferred); legacy `@behavior` scripts auto-migrate at bootstrap) | `execute_event()` / `event_script` |
| **Tactics** (AI personality) | `creature_behaviors` property | `@behavior-template`, `@use-behavior`, `@behavior … react …` | `run_creature_behaviors()` awareness + react |

`bootstrap_creature_behavior_system()` converts template `on_enter=` / `on_discovered=` lines and inline `@behavior` say/emote scripts into `@trigger` handlers. `run_creature_behaviors()` calls `execute_host_event()` per creature before running template-driven reacts (flee, attack, wander).

### 2. Three parallel trigger vocabularies — **resolved (M4)**

| System | Triggers | Dispatch |
|--------|----------|----------|
| `@trigger` / `event_handlers` | `on_enter`, `on_kill`, … | `execute_host_event` (via `execute_event`) |
| Creature spawners | `on_enter`, `periodic` | `dispatch_creature_spawners_for_event` (subscriber on room `on_enter`) |
| Loot spawners | `on_enter`, `on_open`, `on_kill`, `on_break`, `timer` | `dispatch_loot_spawners_for_event` (subscriber on matching host events) |

`execute_event()` runs subscribers first (spawners/loot), then host `@trigger` scripts. Session `go`, inventory open/break, and combat kill all emit through this single path.

### 3. Two event execution modes — **resolved (M4)**

- **`execute_event`** — full semantics (react, teleport, spawn, stat mods, loot subscribers) — used for gates, rooms, items, creatures
- **`run_event_handlers_on`** — read-only narrative preview (builder dry-run / formatting); production paths use `execute_event`

### 4. Inconsistencies to fix in M4

| Issue | Location | Fix |
|-------|----------|-----|
| ~~`@trigger react attack` uses hardcoded damage 10~~ | ~~`event_script.rs`~~ | Done — `creature_attack_damage()` shared helper |
| Duplicate `parse_behavior_line` | `npc_def.rs`, `spawner_def.rs` | Shared `mudl/behavior_line.rs` |
| Legacy `npc_behaviors` fallback | `behavior.rs` | Remove after migration |
| `on_discovered` on generic objects | — | Not wired; LANGUAGE.md marks "coming" |
| No central scheduler | spawner `periodic`, loot `timer` | Property counters today; future `EventScheduler` |
| Resource/crafting spawners | `loot_spawner_def.rs` TODO | New `@resource-spawner` or `@trigger on_harvest` |

### 5. M1 debt (unchanged)

- `object` → `display` coupling
- No graph validator on load
- No SQLite transactions around multi-object moves
- Prototype inheritance resolver not in world state (factory copy only)

## High-Level Architecture (target)
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
- Events/Hooks: `event_handlers` map on every `Object`; MUDL `@trigger` attaches scripts for places, items, and creatures. `MoveManager` fires `on_move` via `emit_on_move_event`. Creature tactics (awareness, react) run through `run_creature_behaviors()` after per-creature `execute_host_event()` (see §4.1).
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

### 4. Event & Timer System (M4 partial)
- **`@trigger`** scripts stored in `Object.event_handlers`; executed by `world/event_script.rs`.
- **Creature spawner `periodic`** and **loot `timer`** use per-object entry counters — not yet a unified scheduler.
- **Planned:** `EventScheduler` for delayed/recurring actions; spawner/loot triggers subscribe to the same bus.

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
│   ├── world/              # Bootstrap, MoveManager, possession, dirty tracking, session
│   ├── command/            # Shared command/bootstrap helpers
│   ├── display/            # Player/builder/debug presentation
│   ├── creature/           # Vitals, combat, behaviors, tactics, spawners (M3)
│   ├── loot/               # Loot spawner runtime (M3)
│   ├── inventory/          # Body-slot inventory (delegates to MoveManager)
│   ├── repl/               # Per-player Session (REPL + future IRC)
│   ├── persistence/        # SQLite abstraction
│   └── bin/repl.rs         # Development REPL (thin adapter over repl::Session)
├── modules/                # MUDL game data (not Rust)
│   └── default/            # Official baseline universe
│       ├── universe.mudl   # Universe entrypoint (@universe, @include-world)
│       └── worlds/
│           └── default_world/   # Flat MUDL files (no subfolders for now)
│               ├── world.mudl   # World entrypoint (@world, @include, @import)
│               ├── map.mudl     # Areas/locations (type=area)
│               ├── creatures.mudl
│               ├── behaviors.mudl  # @behavior-template (M3)
│               ├── npcs.mudl       # @npc instances (M3)
│               ├── players.mudl
│               ├── items.mudl
│               ├── objects.mudl
│               └── expansions/     # Drop-in packs (e.g. haunted_forest.mudl)
└── examples/               # Alternative universe packs
```

**MUDL-first**: All game content (creatures, map, templates) is defined in `.mudl` files. Rust provides loaders, runtime, and persistence — not hardcoded world data.

## Universe and World Hierarchy

A **Universe** is the top-level container. It holds one or more **Worlds**, each a self-contained game setting (locations, creatures, items, player templates).

```
Universe (modules/default/)
  └── World (worlds/default_world/)
        ├── world.mudl      entrypoint (@import expansions)
        ├── map.mudl        areas and exits
        ├── creatures.mudl  @creature anatomy + stats
        ├── behaviors.mudl  @behavior-template personalities
        ├── npcs.mudl       @npc + @loot-spawner attachments
        ├── players.mudl    @player-template (creature=human)
        ├── items.mudl      @prototype / @item scene objects
        ├── objects.mudl    shared prototypes
        └── expansions/     optional self-contained adventure packs
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

## Builder & Wizard Tools

See **[BUILDER.md](BUILDER.md)** for the builder/wizard command design: `@set` / `@unset`, the Properties / State / Status model, permissions, and `@examine` format.

## Player Commands (REPL / MVP)

See **[COMMANDS.md](COMMANDS.md)** for the full command reference.

- **`create <type> <name> [key=value...]`** — Creates an object via `ObjectFactory`. The display name is parsed separately from options (`capacity=3`, `max_weight=10`, etc.); options become properties, not part of `name` or the ID slug. ID base names are slugified and capped at 16 characters (`purse` → `item:purse-001`). When the player has a current location, the new object is placed there automatically.
- **`take` / `get <item>`** — Picks up a visible item from the ground in the current location (carried items are excluded from target resolution). Uses grasp slots from the player's creature anatomy. One ground match takes silently; multiple ground matches disambiguate with short IDs. Failure messages: *"You don't see any X here."*, *"Your hands are full."*, etc.
- **`look`** / **`examine`** — In-character, IRC-friendly natural language (`DisplayFlags::BRIEF` for look). No leading object name on items. Containers: `The backpack contains 20 coins.` `look self`: one gear sentence. `examine self`: creature + gear prose, slot occupancy, and weight. See `COMMANDS.md` style guidelines.
- **`@look`** / **`@examine`** — Out-of-character builder views (`DisplayMode::Builder`): structured properties, state, status.
- **`@dump`** — Raw JSON debug dump.
- **`inventory`** — Full slot-by-slot listing (use `examine self` for weight totals).

### Command conventions (`@` meta-commands)

Player verbs have no prefix (`look`, `examine`, `take`, …). Wizard/builder meta-commands use a leading **`@`**:

| Player (in-character) | Wizard (out-of-character) |
|--------|--------|
| `look backpack` | `@look backpack` |
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

Command helpers live in `src/command/`; possession graph logic in `src/world/possession.rs`; inventory verbs in `src/inventory/`; presentation in `src/display/`.

## Move Semantics

`MoveManager` (`src/world/move_manager.rs`) is the single authority for relocating objects:

- `move_object(src: LocationRef, dst: LocationRef, obj: ObjId)` validates source placement, checks destination capacity/weight/volume, updates holder lists (`contents`, `body_slots`), and fires `on_move` hooks.
- Inventory commands (`take`, `drop`, `put`, `remove`, `wear`) delegate to `MoveManager` convenience wrappers.
- `ObjectFactory::create_stackable_item` creates one instance with `stack_count`; `create_item_instances` spawns separate IDs for non-stacked duplicates.
- `put [count] <item> in <container>` transfers a specific stack quantity; omitting count moves as many units as fit (weight/volume/slots). Remainder stays carried with feedback (`5 won't fit.`).
- `look <container>` shows `The purse contains 20 coins.` using stack-aware labels (`src/display/container.rs`).

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

**Roundtrip guarantee (M1)**: `milestone1_complex_scene_persist_reload_identical` builds a post-play graph (worn container, nested stack, two-handed wield, split ground piles), runs `persist_all` → `hydrate_world`, and asserts byte-identical `Object` equality for every node plus reference integrity across the graph.

**Incremental saves**: `MoveContext.dirty` + `DirtyTracker` mark touched IDs during moves; REPL still calls `persist_all` after inventory verbs — wire dirty tracking through REPL before scaling object counts.

**Soft deletes**: Objects are never hard-deleted. `is_deleted` and `deleted_at` on `Object` mark removal; `list_objects(false)` hides them from normal play. Wizard commands `@delete <target>` and `@undelete <id>` toggle the flag. Deleted objects remain loadable by ID for recovery.

**Schema**: `objects(id, data, is_deleted, deleted_at)` and `counters(type_base, counter)`. Older DB files are migrated with `ALTER TABLE` on connect.

## Refactor Roadmap

### Completed (M1–M3)

1. ~~Unify wield through MoveManager~~
2. ~~REPL session model (`repl::Session`)~~
3. ~~Factory ordering pipeline~~
4. ~~Populate `items.mudl` + bootstrap spawn~~
5. ~~Creature vitals, equipment, combat, behaviors, spawners, loot (M3)~~
6. ~~Event bus foundation (M4 partial): `world::events`, `@trigger`, `execute_event`~~

### M4 — Events & Triggers (active)

| Priority | Task | Rationale |
|----------|------|-----------|
| ~~**P0**~~ | ~~Unify creature `@behavior` scripts into `@trigger` / single executor~~ | Done — §4.1 |
| ~~**P0**~~ | ~~Route spawner + loot dispatch through event bus~~ | Done — §4.2 |
| ~~**P1**~~ | ~~`gate_events` → `execute_event` (mutating door scripts)~~ | Done — §4.3 |
| ~~**P1**~~ | ~~Align `@trigger react attack` with `attack_damage`~~ | Done — `creature_attack_damage()` |
| **P1** | Shared behavior-line parser; drop `npc_behaviors` legacy | §4.4 |
| **P2** | `on_discovered` on arbitrary objects | Builder traps, hidden items |
| **P2** | Central `EventScheduler` (replace periodic/timer counters) | §4.4 |
| **P2** | `@resource-spawner` / harvest triggers | Crafting pipeline |

### Defer (post-M4)

- Gateway + per-player world views (multi-user / IRC)
- Sandboxed DSL interpreter (replace `event_script` hardcoded actions)
- Prototype inheritance resolver in world state (not just factory copy)
- Location/exits as first-class exit objects (beyond `exits` map)
- Graph consistency validator on load
- SQLite transactions wrapping multi-object moves
- `object` → `display` decoupling (`Describable` trait relocation)

## Future Directions

- IRC gateway with per-nick `Session` registry
- Full LLM content generation pipeline (validates MUDL before apply)
- Advanced self-modification (world rewriting its own rules via sandboxed runtime)
- WebSocket/web client
- Procedural generation driven by `@trigger` + spawner composition
