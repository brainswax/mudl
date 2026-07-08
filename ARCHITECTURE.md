# Architecture

**MUDL** (working name) — An IRC-first, programmable MUD/MOO with a custom domain-specific language (DSL), self-modifying world capabilities, and multi-modal authoring (IRC chat, REPL, files, GitHub).

**Status**: High-level design + **Milestones 1–5 implemented** (multi-user IRC via `SessionManager`, `IrcBot`, IRCv3/TLS, per-nick sessions over one `SharedWorld`, room visibility, optimistic `revision` saves). **532** unit/integration tests. This document tracks as-built state (M1–M5), technical debt, and the forward roadmap (M6–M12).

## Milestone Summary

### As-built (M1–M5)

| Milestone | Delivered | Primary modules |
|-----------|-----------|-----------------|
| **M1** | Object graph, `MoveManager`, inventory verbs, SQLite roundtrip, REPL `Session` | `object/`, `inventory/`, `display/`, `persistence/`, `world/move_manager` |
| **M2** | MUDL loader, bootstrap pipeline, map/items/NPCs, `@dig`/`@link`, expansion packs | `mudl/`, `world/bootstrap`, `world/place_builder` |
| **M3** | Creature vitals/stats/effects, equipment modifiers, combat/death, behaviors, awareness, spawners, loot | `creature/`, `loot/` |
| **M4** (largely done) | `@trigger` bus, spawners/loot/resources, scheduler, conditions (DoT/HoT), discovery/harvest | `world/events`, `world/event_script`, `creature/conditions` |
| **M5** | Multi-user IRC: `SessionManager`, `IrcBot`, TLS/IRCv3, room channels, tells, OOC, disconnect persist, concurrency tests | `gateway/`, `irc/`, `repl/player_session.rs`, `world/world_state.rs` |

### Planned (M6–M12)

| Milestone | Focus | Target deliverables |
|-----------|-------|---------------------|
| **M6** | Slack integration | Slack app transport for group playtesting; reuses `SessionManager` + gateway dispatch; thread/channel mapping for OOC vs in-character play |
| **M7** | Wizard tools & persistence | Full builder surface over IRC/Slack; undo/audit; module hot-reload and GitHub webhooks; graph validator; transactional world edits |
| **M8** | Gameplay modules | Optional MUDL packs + engine hooks — economy (currency, shops), combat polish, magic (spells, mana), crafting (recipes, workstations) |
| **M9** | Polish & extensibility | Sandboxed DSL runtime; `CommandDispatcher` DRY across transports; rate limiting; prototype resolver; `object`/`display` decoupling; WebSocket client |
| **M10** | LLM world builder | Prompt → validated MUDL; diff review before apply; copilot for rooms, items, `@trigger` scripts, and expansion scaffolding |
| **M11** | LLM NPCs | Dynamic in-character dialogue bounded by creature templates, `@behavior`, and RBAC; session-scoped NPC memory |
| **M12** | LLM JIT world generation | Runtime procedural rooms, quests, and loot; `@trigger` + spawner composition; validated apply with rollback |

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
│  Frontends: REPL (repl.rs)   IRC bot (irc.rs)   Slack bot (M6, planned) │
└───────────────────────────────┬─────────────────────────────────────────┘
                                │ repl::Session { SharedWorld, PlayerSession }
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
- ~~**No gateway or multi-user session isolation**~~ — **M5 done**: `SessionManager` + per-nick `Arc<Mutex<Session>>` over one `SharedWorld`
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
- `DispatchStack` lives on [`WorldState`](src/world/world_state.rs) (not thread-local); [`SharedWorld`](src/world/world_state.rs) serializes mutations via `tokio::sync::Mutex` per command.
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

*Review date: July 2026. **532** tests. Milestones 1–3 complete; Milestone 4 largely complete; **M5 multi-user IRC complete**.*

### Executive summary

MUDL has a **coherent core** for a single-player REPL: one object graph, one move authority, MUDL-first content, and a unified event bus for room scripts and spawners. The architecture matches the stated principle — **content in MUDL, physics in Rust** — and five expansion packs prove extensibility without engine forks.

The main gaps are **transport DRY and IRC command parity**: `src/bin/repl.rs` is a 1.6k-line command router, `bootstrap.rs` is a 2.5k-line god module, and IRC exposes a player command subset (no `attack` / builder meta yet). World/player split, world-level locking, `SessionManager`, RBAC on actor `PermissionFlags`, and the IRC bot are in place.

### Strengths

| # | Area | Why it matters |
|---|------|----------------|
| 1 | **Single move authority** | `MoveManager` + `LocationRef` keep `location`, `contents`, and `body_slots` consistent; inventory verbs never bypass it. |
| 2 | **MUDL-first bootstrap** | Geography, creatures, behaviors, spawners, and expansions load from flat files — no Rust fork per adventure. |
| 3 | **Composable roles** | `MudlRoleProps` bridges parser → `ObjectFactory`; containers, wearables, portals, breakables stack cleanly. |
| 4 | **Unified event bus (M4)** | `execute_event` → subscribers (scheduler, spawners) → host `@trigger` scripts; depth/cycle guard and `EventOutcome::errors`. |
| 5 | **Hybrid scripting model** | Narrative scripts in `event_handlers`; AI tactics in `creature_behaviors` — bootstrap migrates legacy `@behavior` lines to triggers. |
| 6 | **Presentation tiers** | Player / builder / debug modes are consistent across commands; `resolve_object` is possession-first. |
| 7 | **Persistence abstraction** | `Persistence` trait + JSON blobs; `DirtyTracker` + incremental `persist_dirty`; optimistic `revision` / `updated_at` CAS on save with conflict retry. |
| 8 | **Integration tests** | Full paths: bootstrap → movement → combat → kill loot → conditions → persist (haunted forest, expansion packs). |
| 9 | **M5 concurrency foundation** | `SharedWorld` (`Arc<Mutex<WorldState>>`), per-world `DispatchStack`, transactional `save_objects_batch`. |

### Issues and technical debt (prioritized)

#### P0 — M5 blockers (resolved)

| Issue | Location | Impact | Recommendation |
|-------|----------|--------|----------------|
| ~~**World + player conflated**~~ | ~~`repl::Session` held graph + `player_id`~~ | **Done** — `world::WorldState` (graph, anatomy, dirty, dispatch) + `repl::PlayerSession` (actor, location cache). `Session` bundles `SharedWorld` + `PlayerSession`; IRC holds one `SharedWorld` and one `PlayerSession` per nick. | — |
| ~~**No concurrency control**~~ | ~~Single-threaded REPL; `DISPATCH_STACK` was `thread_local`~~ | **Done** — `DispatchStack` on `WorldState`; `SharedWorld` (`Arc<Mutex<WorldState>>`) with `lock()` / `lock_blocking()`; IRC uses per-nick `Arc<Mutex<Session>>` + `with_locked_async`; REPL uses `with_locked`. Batch saves release the world lock during SQLite I/O. Per-room mutex deferred. | — |
| ~~**RBAC stubbed**~~ | ~~`has_wizard_permission()` always `true`~~ | **Done on IRC** — `gateway/rbac.rs` checks `PermissionFlags` on actor; meta commands RBAC-gated (deferred to REPL). REPL still uses permissive defaults for local dev. | Rate-limit IRC; expand builder surface when ready. |
| ~~**Last-write-wins persistence**~~ | ~~`SqlitePersistence::save_object` per row, no version field~~ | **Done** — `Object.revision` + `updated_at`; CAS `UPDATE … WHERE revision = ?`; `PersistenceError::RevisionConflict`; `save_and_sync`, `save_object_with_retry`, `persist_dirty` batch retry. | — |

#### P1 — Quality / maintainability (pre- or early M5)

| Issue | Location | Impact | Recommendation |
|-------|----------|--------|----------------|
| **Fat frontend adapter** | `src/bin/repl.rs` (~1.6k lines) | IRC would duplicate routing logic | Introduce `CommandDispatcher` in `src/command/` (or `src/gateway/`) returning `CommandResult { lines, dirty }`; REPL and IRC bot call the same API. |
| **God-module bootstrap** | `world/bootstrap.rs` (~2.5k lines) | Hard to extend spawn phases or test in isolation | Split: `bootstrap/places.rs`, `bootstrap/creatures.rs`, `bootstrap/spawners.rs`, orchestrator only. |
| **`event_script` growth** | `world/event_script.rs` (~1.3k lines) | Every new action needs Rust | Cap M4 actions; plan M9 sandboxed runtime. Short term: register actions via enum + `register_action` table driven from MUDL metadata. |
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

### M5 as-built (IRC / multi-user)

```
repl.rs ──► Session               IRC bot ──┐
              SharedWorld + player            ├──► Gateway (auth, RBAC)
              Mutex per command               │         │
              DispatchStack on WorldState     │         ▼
              optimistic revision saves       └──► SharedWorld (Arc<Mutex<WorldState>>)
                        │                            │
                        ▼                            ▼
                   repl.db                    PlayerSession × N
                                              (actor_id, location, prefs)
```

| Delivered (M5) | Deferred to roadmap |
|----------------|---------------------|
| `SessionManager` login/logout + disconnect persist | **M6** Slack transport; **M9** per-room locking |
| `IrcBot` — TLS + IRCv3 caps, room channels, tells, OOC | **M7** builder meta over transports; **M8** combat parity |
| `dispatch_command` + per-nick `Arc<Mutex<Session>>` | **M6/M9** `CommandDispatcher` DRY |
| `SharedWorld` + `WorldState` / `PlayerSession` split | **M9** rate limiting |
| Per-world `DispatchStack` (depth/cycle guard) | **M7** undo/audit; **M10–M12** LLM apply |
| Transactional `save_objects_batch` + optimistic `revision` | **M7** GitHub webhooks; graph validator |
| Real RBAC (`PermissionFlags` on actor) over IRC | |
| Multi-player integration tests (`gateway::multi_user`, `load`, `edge_cases`, `m5_scenarios`) | |
| **532** tests (IRC + multi-user + load + edge cases) | |

**M5 slice (complete):** (1) ~~`WorldState` + `PlayerSession` split~~, (2) ~~`dispatch_command` + IRC bot~~, (3) ~~RBAC on actor permissions~~, (4) ~~world `Mutex` + transactional saves~~, (5) ~~IRC adapter + `SessionManager`~~.

### Recommended next steps

1. **M4 tail (1–2 PRs):** shared `behavior_line` parser; remove `npc_behaviors` legacy; fix persist fallback; graph validator (warn-only).
2. **M6 — Slack:** `SlackBot` adapter on `SessionManager`; workspace/channel config; playtest harness with mock transport (mirror `irc::` test pattern).
3. **Bridge to M7:** IRC `attack` / inventory parity; `CommandDispatcher` shared by REPL, IRC, and Slack; rate limiting stub.
4. **M7–M9:** wizard persistence and undo; gameplay modules (M8); sandboxed runtime and transport polish (M9).
5. **M10–M12:** LLM pipelines after builder tools and validation infrastructure are stable.

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
- ~~No SQLite transactions around multi-object moves~~ — batch `save_objects_batch` is transactional; per-move graph updates still in-memory under world lock
- Prototype inheritance resolver not in world state (P2)
- Fat `repl.rs` / `bootstrap.rs` (P1)

## High-Level Architecture (target)
```
┌─────────────────────────────────────────────────────────────┐
│                    Frontends / Input Layers                  │
│  • IRC Bot (primary, M5)                                     │
│  • Slack Bot (M6 — group playtesting)                        │
│  • CLI REPL / Interactive Prompt                             │
│  • File Loader (.mudl scripts)                               │
│  • GitHub Importer (raw files + webhooks, M7)                │
│  • Future: WebSocket client (M9), LLM authoring (M10–M12)    │
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
- **Planned (M9)**: sandboxed verb/event code for true self-modification; **M10–M12** apply validated LLM output through this runtime.

### 3. World State & Persistence
- [`WorldState`](src/world/world_state.rs): in-memory object graph, anatomy, `DirtyTracker`, and `DispatchStack`.
- [`SharedWorld`](src/world/world_state.rs): `Arc<Mutex<WorldState>>` — one handle per game world; REPL and IRC lock per command.
- SQLite for durability (JSON blobs per object row).
- Optimistic concurrency: `revision` + `updated_at` on each `Object`; CAS on save; `save_and_sync` / retry helpers keep in-memory revision aligned with the DB.
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
- **IRC Bot (M5)**: Command parsing, world interaction, multi-user play.
- **Slack Bot (M6)**: Group playtesting transport; same gateway and session model as IRC.
- **REPL**: Development and testing.
- **Loaders**: File + GitHub integration (webhooks expanded in M7).

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
- **IRC (M5)**: `src/irc/` — TLS via `rustls`/`tokio-rustls`, IRCv3 `CAP LS 302` negotiation, `IrcBot` + `SessionManager`.

## Repository Layout

```
mudl/
├── src/                    # Rust engine only
│   ├── object/             # Object model, roles, LocationRef, ObjectFactory
│   ├── mudl/               # MUDL parser, anatomy, role props, @include loader
│   ├── world/              # Bootstrap, MoveManager, WorldState, dispatch_guard, dirty, session helpers
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

**Startup**: `bootstrap_world()` ensures MUDL-defined content exists, then `Session::restore()` → `WorldState::restore()` hydrates all active objects from the DB; `PlayerSession::restore()` resolves `current_location` from the player's persisted `location` field.

**Roundtrip guarantee (M1)**: `milestone1_complex_scene_persist_reload_identical` builds a post-play graph (worn container, nested stack, two-handed wield, split ground piles), runs `persist_all` → `hydrate_world`, and asserts byte-identical `Object` equality for every node plus reference integrity across the graph.

**Incremental saves**: `DirtyTracker` marks touched IDs; REPL uses `Session::persist_changes` → `persist_dirty` (batch transactional save + revision retry on conflict). Remaining risk: `persist_inventory_dirty` full-graph fallback when dirty is empty — audit move/inventory paths before multi-user scale.

**Optimistic locking**: Each save expects the in-memory `revision` to match SQLite. On `RevisionConflict`, `persist_dirty` / `save_object_with_retry` reload the row, refresh `revision`, and retry (bounded). New inserts start at revision 1.

**Soft deletes**: Objects are never hard-deleted. `is_deleted` and `deleted_at` on `Object` mark removal; `list_objects(false)` hides them from normal play. Wizard commands `@delete <target>` and `@undelete <id>` toggle the flag. Deleted objects remain loadable by ID for recovery.

**Schema**: `objects(id, data, is_deleted, deleted_at, revision, updated_at)` and `counters(type_base, counter)`. Older DB files are migrated with `ALTER TABLE` on connect (`revision` / `updated_at` added when absent).

## Refactor Roadmap

### Completed (M1–M3)

1. ~~Unify wield through MoveManager~~
2. ~~REPL session model (`repl::Session`)~~
3. ~~Factory ordering pipeline~~
4. ~~Populate `items.mudl` + bootstrap spawn~~
5. ~~Creature vitals, equipment, combat, behaviors, spawners, loot (M3)~~
6. ~~Event bus foundation (M4 partial): `world::events`, `@trigger`, `execute_event`~~

### M5 — Multi-user IRC (complete)

1. ~~`WorldState` + `PlayerSession` split~~ — `world/world_state.rs`, `repl/player_session.rs`
2. ~~`SessionManager`~~ — login/logout, per-nick `Arc<Mutex<Session>>`, disconnect persist
3. ~~`IrcBot` + `dispatch_command`~~ — TLS/IRCv3, room channels, tells, OOC, nick normalization
4. ~~Concurrency hardening~~ — async world locks, load tests, edge-case reconnect/RBAC/conflict tests
5. ~~`SharedWorld` mutex + optimistic `revision`~~ — batch saves, conflict retry on logout

Test suites: `gateway::multi_user`, `gateway::session_manager`, `gateway::load`, `gateway::edge_cases`, `gateway::m5_scenarios`, `irc::` (`make test-m5`).

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

### M6 — Slack integration (planned)

| Priority | Task | Rationale |
|----------|------|-----------|
| **P0** | `SlackBot` + `SessionManager` binding | Reuse M5 multi-user model; workspace user → player session |
| **P0** | Channel/thread routing | OOC workspace channel vs per-room threads for in-character speech |
| **P1** | Mock transport + gateway tests | Mirror `irc::` / `gateway::m5_scenarios` pattern for CI |
| **P1** | `CommandDispatcher` shared by REPL, IRC, and Slack | DRY transport layer; shrink `repl.rs` |

### M7 — Wizard tools & persistence (planned)

| Priority | Task | Rationale |
|----------|------|-----------|
| **P0** | Builder meta over IRC/Slack (`@dig`, `@set`, …) | RBAC already checks `PermissionFlags` |
| **P0** | Undo / audit trail for wizard edits | Safe live modification; prerequisite for LLM apply |
| **P1** | GitHub webhooks + module hot-reload | File/GitHub authoring path from vision |
| **P1** | Graph validator on hydrate/bootstrap | Orphan refs, dangling `contents` |
| **P2** | `attack` and full inventory parity over transports | Match REPL player surface |

### M8 — Gameplay modules (planned)

| Module | Task | Rationale |
|--------|------|-----------|
| **Economy** | Currency, shops, buy/sell verbs | Data-driven via MUDL + thin engine hooks |
| **Combat polish** | `@formula`, status refinements, PvP rules | Reduce hard-coded combat math |
| **Magic** | Spells, mana, resistances, `@cast` | New `@trigger` actions + creature stats |
| **Crafting** | Recipes, workstations, `craft` verb | Extends harvest/resource spawner model |

### M9 — Polish & extensibility (planned)

| Priority | Task | Rationale |
|----------|------|-----------|
| **P0** | Sandboxed DSL interpreter | Replace `event_script` hardcoded actions; unlock self-modification |
| **P1** | Prototype inheritance resolver in world state | Runtime `@set prototype` consistency |
| **P1** | `object` → `display` decoupling | Core engine independent of presentation |
| **P2** | Rate limiting / flood protection | Production IRC/Slack hardening |
| **P2** | Per-room fine-grained locking | Scale if world-lock contention warrants |
| **P3** | WebSocket/web client | Third player-facing transport |
| **P3** | First-class exit objects | Keys, locks per direction beyond `exits` map |

### M10 — LLM world builder (planned)

- Prompt → MUDL with schema validation before apply
- Diff/review UI for builders (rooms, items, `@trigger` scripts)
- Git-friendly export of LLM-assisted edits

### M11 — LLM NPCs (planned)

- In-character dialogue generation bounded by `@behavior-template` and creature stats
- Session-scoped memory; no unbounded world mutation from NPC layer
- RBAC: NPC speech cannot bypass wizard permissions

### M12 — LLM JIT world generation (planned)

- Runtime procedural rooms, quests, and loot on demand
- Composes `@trigger`, spawners, and expansion-pack patterns
- Validated apply with rollback; integrates with M7 audit trail

## Future Directions

- **M6–M7:** Slack playtesting cohorts and full wizard tooling over all transports
- **M8:** Optional gameplay modules (economy, magic, crafting) as drop-in MUDL packs
- **M9:** Sandboxed runtime and transport polish — foundation for safe LLM apply
- **M10–M12:** Layered LLM integration — builder copilot, NPC dialogue, JIT procedural content
