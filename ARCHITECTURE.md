# Architecture

**MUDL** (working name) — An IRC-first, programmable MUD/MOO with a custom domain-specific language (DSL), self-modifying world capabilities, and multi-modal authoring (IRC chat, REPL, files, GitHub).

**Status**: High-level design + **Milestones 1–3 implemented**, **Milestone 4 (Events & Triggers) largely implemented** — `@trigger` bus, spawners, scheduler, conditions (DoT/HoT). **437** unit/integration tests. This document tracks target architecture, as-built state per milestone, and remaining M4+ debt.

## Milestone Summary (as-built)

| Milestone | Delivered | Primary modules |
|-----------|-----------|-----------------|
| **M1** | Object graph, `MoveManager`, inventory verbs, SQLite roundtrip, REPL `Session` | `object/`, `inventory/`, `display/`, `persistence/`, `world/move_manager` |
| **M2** | MUDL loader, bootstrap pipeline, map/items/NPCs, `@dig`/`@link`, expansion packs | `mudl/`, `world/bootstrap`, `world/place_builder` |
| **M3** | Creature vitals/stats/effects, equipment modifiers, combat/death, behaviors, awareness, spawners, loot | `creature/`, `loot/` |
| **M4** (largely done) | `@trigger` bus, spawners/loot/resources, scheduler, conditions (DoT/HoT), discovery/harvest | `world/events`, `world/event_script`, `creature/conditions` |

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
│ Creature (M3)       │         │ Events (M4) + conditions                    │
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

## Milestone 4 — Events, triggers & conditions

M4 adds a builder-facing **`@trigger`** system on places, objects, NPCs, and spawn-templates, plus timed **conditions** on creatures.

| Component | Role |
|-----------|------|
| `trigger_def.rs` | Parse `@trigger <event> <action> [text…]` |
| `events.rs` | `EventContext`, `attach_triggers`, `execute_event` |
| `event_script.rs` | Script actions: `narrate`, `say`, `emote`, `react`, `damage`/`heal`, `mod-stat`/`mod-skill`, `set-property`, `grant-effect`/`remove-effect`/`cure-tag`, `teleport`, `spawn creature`/`item`, `when`/`if` conditionals, `stop` |
| `creature/conditions.rs` | `@effect` DoT/HoT, `duration_ticks`, `tick_on` (default `on_enter`), `condition_ticks` persistence |
| **Wired events** | `on_enter`/`on_leave` (movement), `on_take`/`on_drop`/`on_move` (inventory), `on_break`, `on_harvest` (resource nodes), `on_death`/`on_kill` (combat), `on_discovered` (perception + triggers), `on_unlock`/`on_open` (gates, narrative-only) |

**Room entry order** (`Session::go`):

```
portal prep → on_leave (place) → move player → execute_event(on_enter)
  → subscribers: scheduler tick, due @schedule jobs, creature/loot/resource spawners
  → host @trigger scripts (registration order; `stop` halts remainder)
  → creature behaviors (on_enter) → room look → equipment regen → condition ticks
```

**Dispatch robustness** (`world/events.rs`):

- Re-entrant depth cap (32 frames) and cycle detection (same host + event in flight).
- Inactive/missing hosts record an error and skip dispatch; missing handlers are a silent no-op.
- `EventOutcome::errors` collects subscriber/script failures; player lines stay separate.
- `EventOutcome::dirty` is a `HashSet` — O(1) dedupe for persistence marking.
- Scheduled jobs call `execute_host_event` only (no subscriber re-entry).
- Session model is single-threaded per REPL; dispatch guard uses thread-local stack (not `Send` across tasks).
- Conditions (`active_effects`, `condition_ticks`) and scheduler state persist as normal object properties.
- **`EventContext`**: `actor_id` (who caused the event), `host_id` (whose `@trigger` handlers run), optional `target_id` (victim, item, etc.). Distinct from `ScriptTarget::Host` in script lines (defaults to the dispatch host).

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

## Architectural Review (M1–M4)

*Review date: July 2026. **437** tests. Milestones 1–3 complete; Milestone 4 largely complete.*

### Executive summary

MUDL has a **coherent core** for a single-player REPL: one object graph, one move authority, MUDL-first content, and a unified event bus for room scripts and spawners. The architecture matches the stated principle — **content in MUDL, physics in Rust** — and five expansion packs prove extensibility without engine forks.

The main gaps are **scale and multi-user readiness**: `repl::Session` merges world state and player view, `src/bin/repl.rs` is a 1.6k-line command router, `bootstrap.rs` is a 2.5k-line god module, and RBAC/concurrency are stubbed. None of these block continued content work; they **do** block IRC (M5) without a focused refactor pass.

### Strengths

| # | Area | Why it matters |
|---|------|----------------|
| 1 | **Single move authority** | `MoveManager` + `LocationRef` keep `location`, `contents`, and `body_slots` consistent; inventory verbs never bypass it. |
| 2 | **MUDL-first bootstrap** | Geography, creatures, behaviors, spawners, and expansions load from flat files — no Rust fork per adventure. |
| 3 | **Composable roles** | `MudlRoleProps` bridges parser → `ObjectFactory`; containers, wearables, portals, breakables stack cleanly. |
| 4 | **Unified event bus (M4)** | `execute_event` → subscribers (scheduler, spawners) → host `@trigger` scripts; depth/cycle guard and `EventOutcome::errors`. |
| 5 | **Hybrid scripting model** | Narrative scripts in `event_handlers`; AI tactics in `creature_behaviors` — bootstrap migrates legacy `@behavior` lines to triggers. |
| 6 | **Presentation tiers** | Player / builder / debug modes are consistent across commands; `resolve_object` is possession-first. |
| 7 | **Persistence abstraction** | `Persistence` trait + JSON blobs; `DirtyTracker` + incremental `persist_dirty` wired through `Session::persist_changes`. |
| 8 | **Integration tests** | Full paths: bootstrap → movement → combat → kill loot → conditions → persist (haunted forest, expansion packs). |

### Issues and technical debt (prioritized)

#### P0 — Blocks M5 (multi-user / IRC)

| Issue | Location | Impact | Recommendation |
|-------|----------|--------|----------------|
| **World + player conflated** | `repl::Session` holds full `HashMap<ObjectId, Object>` *and* `player_id` | Two IRC nicks cannot safely share one world without a shared `WorldState` + per-connection player view | Split `WorldState` (authoritative graph) from `PlayerSession` (actor, location cache, display prefs). IRC bot holds `Arc<RwLock<WorldState>>` + one lightweight session per nick. |
| **No concurrency control** | Single-threaded REPL; `DISPATCH_STACK` is `thread_local` | Parallel commands → data races, duplicate spawns, lost updates | Per-world `DispatchGuard` on `WorldState`; serialize mutations per room or per world with `tokio::sync::Mutex`. Wrap multi-object moves in SQLite transactions. |
| **RBAC stubbed** | `has_wizard_permission()` always `true` | Any IRC nick could run `@set` / `@delete` | Gateway checks `PermissionFlags` on actor object; map IRC auth → player ID. Enforce builder vs wizard tiers before meta-commands. |
| **Last-write-wins persistence** | `SqlitePersistence::save_object` per row, no version field | Concurrent edits to same object silently overwrite | Add `revision` or `updated_at` on `Object`; optimistic lock on save; retry on conflict. |

#### P1 — Quality / maintainability (pre- or early M5)

| Issue | Location | Impact | Recommendation |
|-------|----------|--------|----------------|
| **Fat frontend adapter** | `src/bin/repl.rs` (~1.6k lines) | IRC would duplicate routing logic | Introduce `CommandDispatcher` in `src/command/` (or `src/gateway/`) returning `CommandResult { lines, dirty }`; REPL and IRC bot call the same API. |
| **God-module bootstrap** | `world/bootstrap.rs` (~2.5k lines) | Hard to extend spawn phases or test in isolation | Split: `bootstrap/places.rs`, `bootstrap/creatures.rs`, `bootstrap/spawners.rs`, orchestrator only. |
| **`event_script` growth** | `world/event_script.rs` (~1.3k lines) | Every new action needs Rust | Cap M4 actions; plan M6 sandbox. Short term: register actions via enum + `register_action` table driven from MUDL metadata. |
| **Dual AI execution path** | `run_creature_behaviors()` after `execute_event(on_enter)` | Tactics (flee/attack/wander) still outside the bus; ordering is implicit in `Session::go` | Document ordering contract (done in room-entry diagram). Long term: optional `react` as subscriber or phase-3 of `on_enter`. |
| **Inventory persist fallback** | `persist_inventory_dirty` → `persist_all` when dirty empty | Accidental full-graph writes if dirty not marked | Audit inventory/move paths; mark dirty in `MoveManager`; remove full-graph fallback in production builds. |
| **Duplicate parsers** | `parse_behavior_line` in `npc_def.rs` and `spawner_def.rs` | Drift risk | Extract `mudl/behavior_line.rs` (listed in §4.4). |

#### P2 — Correctness / extensibility (can parallel content work)

| Issue | Location | Impact | Recommendation |
|-------|----------|--------|----------------|
| **`object` → `display` coupling** | `object/mod.rs` imports `Describable` | Core depends on presentation | Move `Describable` impl to `display/` via extension trait or wrapper. |
| **No graph validator** | Ad-hoc prune on load | Orphan refs, dangling `contents` possible after bugs | `validate_graph(objects) -> Vec<GraphError>` on hydrate and after bootstrap. |
| **Prototype resolver** | Factory copies at spawn only | Runtime `@set prototype` and display inheritance can diverge | Central `resolve_prototype_chain(id)` in world layer. |
| **Legacy `npc_behaviors`** | `behavior.rs::legacy_npc_behaviors` | Dead code path for old content | Migrate remaining content; delete fallback. |
| **Exits as property maps** | `exits` on place objects | No first-class exit objects (keys, locks per direction) | Defer until builder demand; `@link` works for MVP. |

#### P3 — Nice to have

| Issue | Recommendation |
|-------|----------------|
| `DisplayContext` clones full object map on `go` look | Pass `&HashMap` + arena; avoid clone per room render |
| `AnatomyRegistry` per session | Share `Arc<AnatomyRegistry>` from universe load (immutable) |
| Combat/formula hard-coding | `@formula` or data tables in MUDL when sandbox exists |
| `move_manager` "stub" comment on move hooks | Update comment; hooks are live via `emit_on_move_event` |

### Hard-coded vs data-driven — assessment

The split is **healthy for MVP**:

- **Correctly data-driven:** map, items, creatures, behaviors, spawners, loot tables, `@trigger` reactions, conditions, expansion packs.
- **Correctly hard-coded:** movement validation, stack merge, combat math, awareness contests, weighted spawn rolls, `event_script` interpreter.
- **Risk zone:** `event_script.rs` — each new builder-facing verb requires a Rust match arm. Without a sandbox, MUDL cannot be truly self-modifying; LLM-generated *scripts* are limited to the fixed action vocabulary.

### M5 readiness (IRC / multi-user)

```
Today (M4)                         Target (M5)
──────────                         ───────────

repl.rs ──► Session               IRC bot ──┐
              (world + player)              ├──► Gateway (auth, RBAC, rate limit)
              full object map               │         │
              thread_local events           │         ▼
              SQLite per-save               └──► WorldState (shared, locked)
                        │                            │
                        ▼                            ▼
                   repl.db                    PlayerSession × N
                                              (actor_id, location, prefs)
```

| Ready today | Not ready |
|-------------|-----------|
| `repl::Session::go` orchestration reusable | Shared world with multiple simultaneous actors |
| `Persistence` trait async-ready | Transactions across multi-object commands |
| `EventContext` (actor/host/target) | Per-connection permission enforcement |
| `command/` parsers shared | Thin transport adapters (REPL is thick) |
| Incremental dirty persist | Optimistic concurrency / revision fields |
| 437 tests (single-player assumptions) | Multi-player integration tests |

**Minimum M5 slice:** (1) `WorldState` + `PlayerSession` split, (2) `Gateway::dispatch(actor, line) -> Outcome`, (3) real RBAC, (4) world `Mutex` + transactional saves, (5) IRC adapter ~200 lines calling gateway.

### Recommended next steps

1. **M4 tail (1–2 PRs):** shared `behavior_line` parser; remove `npc_behaviors` legacy; fix persist fallback; graph validator (warn-only).
2. **M5 foundation (before IRC):** `WorldState`, `CommandDispatcher`, RBAC on `PermissionFlags`, SQLite transactions in `MoveManager`.
3. **M5 transport:** IRC bot as thin client; session registry `HashMap<Nick, PlayerSession>`.
4. **Post-M5:** sandboxed verb runtime; prototype resolver; `object`/`display` decoupling.

## Resolved M4 issues (historical)

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
| Resource spawners | `on_enter`, `on_harvest`, `timer` | `dispatch_resource_spawners_for_event` (subscriber on matching host events) |

`execute_event()` runs subscribers first (scheduler tick + spawners/loot/resources), then host `@trigger` scripts. Session `go`, inventory open/break/harvest, and combat kill all emit through this single path.

### 3. Two event execution modes — **resolved (M4)**

- **`execute_event`** — full semantics (react, teleport, spawn, stat mods, loot subscribers) — used for gates, rooms, items, creatures
- **`run_event_handlers_on`** — read-only narrative preview (builder dry-run / formatting); production paths use `execute_event`

### 4. Inconsistencies to fix in M4

| Issue | Location | Fix |
|-------|----------|-----|
| ~~`@trigger react attack` uses hardcoded damage 10~~ | ~~`event_script.rs`~~ | Done — `creature_attack_damage()` shared helper |
| Duplicate `parse_behavior_line` | `npc_def.rs`, `spawner_def.rs` | Shared `mudl/behavior_line.rs` |
| Legacy `npc_behaviors` fallback | `behavior.rs` | Remove after migration |
| ~~`on_discovered` on generic objects~~ | ~~—~~ | Done — `world/discovery.rs`, `hidden_until_discovered` role |
| ~~No central scheduler~~ | ~~spawner `periodic`, loot `timer`~~ | Done — `world/scheduler.rs`, room `scheduler_tick_on_enter` |
| ~~Resource/crafting spawners~~ | ~~`loot_spawner_def.rs` TODO~~ | Done — `@resource-spawner`, `on_harvest` event bus |

### 5. Carried debt (see prioritized table above)

- `object` → `display` coupling (P2)
- No graph validator on load (P2)
- No SQLite transactions around multi-object moves (P0 for M5)
- Prototype inheritance resolver not in world state (P2)
- Fat `repl.rs` / `bootstrap.rs` (P1)

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

### 2. DSL (MUDL) — loader + script interpreter
- **Loader** (`src/mudl/`): line-oriented parsers for `@creature`, `@trigger`, map blocks, spawners, expansions — no pest/chumsky yet.
- **Script runtime** (`world/event_script.rs`): fixed vocabulary of actions (`narrate`, `spawn`, `grant-effect`, `when … then …`). Validated at attach time; not a general sandbox.
- **Planned**: sandboxed verb/event code for true self-modification and LLM-generated logic.

### 3. World State & Persistence
- In-memory graph of objects + locations.
- SQLite for durability (or JSON snapshots).
- Git-friendly export/import.

### 4. Event & Timer System (M4)
- **`@trigger`** scripts stored in `Object.event_handlers`; executed by `world/event_script.rs`.
- **`execute_event`** dispatch order: subscribers (scheduler → spawners) then host handlers; `stop`/`cancel` halts remaining handlers; errors collected in `EventOutcome::errors`; depth/cycle guard on re-entrant dispatch.
- **`EventScheduler`** (`world/scheduler.rs`) — room-scoped ticks, named property counters, and `@schedule` jobs that fire host triggers on interval.
- **`@resource-spawner`** — renewable harvest nodes on `on_harvest` / `on_enter` / `timer`; player command `harvest <object>`.

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

## Technology Stack (as-built)
- **Language**: Rust (2021 edition).
- **Persistence**: SQLite via `sqlx` + serde JSON blobs.
- **Async**: Tokio (`#[tokio::main]` in REPL; `Persistence` is async-ready).
- **MUDL parsing**: Custom line/block parsers in `src/mudl/` (not pest/chumsky).
- **IRC (M5)**: `irc` crate or similar — not yet integrated.

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
| `take`, `drop`, `put`, `remove`, `wield`, `wear` | Dirty-marked objects via `Session::persist_changes`; `persist_inventory_dirty` still falls back to `persist_all` if dirty set empty (see P1 debt) |
| `go` | Player `location` |
| `@set`, `@unset`, `save` | Target object |
| Bootstrap | World areas, exits, default player (idempotent) |

**Startup**: `bootstrap_world()` ensures MUDL-defined content exists, then `restore_session()` hydrates all active objects from the DB and restores the player's `current_location` from their persisted `location` field.

**Roundtrip guarantee (M1)**: `milestone1_complex_scene_persist_reload_identical` builds a post-play graph (worn container, nested stack, two-handed wield, split ground piles), runs `persist_all` → `hydrate_world`, and asserts byte-identical `Object` equality for every node plus reference integrity across the graph.

**Incremental saves**: `DirtyTracker` marks touched IDs; REPL uses `Session::persist_changes` → `persist_dirty` after most commands. Remaining risk: `persist_inventory_dirty` full-graph fallback when dirty is empty — audit move/inventory paths before multi-user scale.

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

### M4 — Events & Triggers (remaining)

| Priority | Task | Rationale |
|----------|------|-----------|
| ~~**P0**~~ | ~~Unify creature `@behavior` scripts into `@trigger` / single executor~~ | Done — §4.1 |
| ~~**P0**~~ | ~~Route spawner + loot dispatch through event bus~~ | Done — §4.2 |
| ~~**P1**~~ | ~~`gate_events` → `execute_event` (mutating door scripts)~~ | Done — §4.3 |
| ~~**P1**~~ | ~~Align `@trigger react attack` with `attack_damage`~~ | Done — `creature_attack_damage()` |
| ~~**P2**~~ | ~~Conditions / DoT / HoT / cures~~ | Done — `creature/conditions.rs`, expansion examples |
| **P1** | Shared behavior-line parser; drop `npc_behaviors` legacy | §4.4 |
| ~~**P2**~~ | ~~`on_discovered` on arbitrary objects~~ | Done — `hidden_until_discovered`, `run_discovery_on_look` |
| ~~**P2**~~ | ~~Central `EventScheduler` (replace periodic/timer counters)~~ | Done — `world/scheduler.rs` |
| ~~**P2**~~ | ~~`@resource-spawner` / harvest triggers~~ | Done — `resource/spawner.rs`, `harvest` command |

### M5 — Multi-user / IRC (next milestone)

| Priority | Task | Rationale |
|----------|------|-----------|
| **P0** | `WorldState` + `PlayerSession` split | Shared world, per-connection actor |
| **P0** | `Gateway` + real RBAC | `has_wizard_permission` → `PermissionFlags` on actor |
| **P0** | World-level lock + SQLite transactions | Safe concurrent commands |
| **P0** | Optimistic revision on `Object` save | Prevent silent overwrites |
| **P1** | `CommandDispatcher` shared by REPL and IRC | DRY transport layer |
| **P1** | Per-world `DispatchGuard` (not `thread_local`) | Re-entrant events under async |
| **P2** | Multi-player integration tests | Two actors, same room, combat, take/drop races |

### Defer (M6+)

- Sandboxed DSL interpreter (replace `event_script` hardcoded actions)
- Prototype inheritance resolver in world state (not just factory copy)
- Location/exits as first-class exit objects (beyond `exits` map)
- `object` → `display` decoupling (`Describable` trait relocation)
- Full LLM content pipeline (validates MUDL before apply)
- WebSocket/web client

## Future Directions

- IRC gateway with per-nick `PlayerSession` registry over shared `WorldState`
- Procedural generation driven by `@trigger` + spawner composition
- Advanced self-modification via sandboxed runtime (beyond fixed `event_script` actions)
