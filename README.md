# MUDL

**A self-modifying MUD/MOO engine with a custom DSL — IRC-first, programmable worlds.**

MUDL is a Rust-powered text world engine for builders who want MOO-style live modification without giving up version control, persistence, or sane architecture. Define rooms, creatures, and player templates in flat `.mudl` files; explore and extend them through a REPL or IRC bot. New players spawn as a **naked human** — full anatomy slots, empty hands — ready for you to dress the world around them.

[![Rust](https://img.shields.io/badge/Rust-000000?style=flat&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Tokio](https://img.shields.io/badge/Tokio-000000?style=flat&logo=rust&logoColor=white)](https://tokio.rs/)
[![SQLite](https://img.shields.io/badge/SQLite-07405E?logo=sqlite&logoColor=white)](https://www.sqlite.org/)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPLv3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)

**Repository:** [github.com/brainswax/mudl](https://github.com/brainswax/mudl)

---

## Why MUDL?

If you have ever wanted a MUD where the **world is data you can edit live**, where **everything is an object** with prototype inheritance, and where **content lives in Git-friendly files** instead of opaque server state — this project is for you. MUDL targets three audiences:

- **Players** who want immersive IRC-friendly prose, turn-based combat, and discoverable secrets in optional adventure packs.
- **Builders** who want a readable language for rooms, creatures, `@trigger` scripts, and expansion packs — without recompiling the server for every change.
- **Engine contributors** interested in async Rust, DSL design, object graphs, and SQLite-backed persistence.

The long-term vision is IRC-first play, multi-modal authoring (REPL, files, GitHub), and safe runtime self-modification. **Milestone 5 (multi-user IRC)** is complete: shared world, per-nick sessions, room visibility, tells, and TLS/IRCv3 transport. Post-M5 work includes full builder commands over IRC, combat parity on IRC, and rate limiting.

---

## Features

| Area | What you get |
|------|----------------|
| **MUDL DSL** | Declarative `.mudl` files for universes, worlds, maps, creatures, behaviors, and expansion packs |
| **Object model** | Prototype-based inheritance — rooms, items, players, NPCs are all `Object`s with composable roles |
| **Anatomy & inventory** | Creature `@slot` definitions; take, drop, wield, wear, containers, stackables |
| **Combat & creatures** | Turn-based `attack`, awareness/ambush, corpses, respawn, `@behavior-template` AI |
| **Events & triggers** | `@trigger` scripts on places, items, and creatures; spawners, loot, harvest, schedules |
| **Conditions** | `@effect` buffs/debuffs with DoT/HoT, duration ticks, `grant-effect` / `cure-tag` in scripts |
| **Expansion packs** | Five drop-in adventures (haunted forest, swamp, spider den, beach resort, fey glade) |
| **Persistence** | SQLite with stable `type:base-name-###` IDs and full object JSON roundtrip |
| **Builder tools** | `@set` / `@unset`, `@dig` / `@link`, `@trigger`, `@examine`, place building |
| **IRC bot (M5)** | IRCv3 + TLS, `SessionManager`, room channels, tells, OOC world channel, mock + live modes |
| **Clean architecture** | Pure core engine; gateway RBAC (Player / Builder / Wizard) on `PermissionFlags` |
| **Tests** | **532** unit/integration tests — loader, combat, events, IRC, multi-user, load, and edge cases |

---

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (2021 edition toolchain)
- SQLite (bundled via `sqlx`)

### Build & Run

```bash
git clone git@github.com:brainswax/mudl.git
cd mudl

cp .env.example .env   # optional

cargo build --bin repl
cargo run --bin repl

# IRC bot (mock mode reads stdin as "nick command"):
IRC_MOCK=1 cargo run --bin irc

# Or:
make run-repl
make run-irc
make test-m5           # IRC + multi-user tests only
make dev               # fmt + check + clippy + test
```

On startup you should see:

```text
Welcome to MUDL.
Type 'help' for commands.
>
```

Bootstrap logs go to stderr when `RUST_LOG=info` — they stay off the prompt so play stays immersive.

The REPL loads `modules/default/universe.mudl`, bootstraps `default_world` if the database is empty, and places you in **The Void** as a naked human with no starting gear. The stock world already imports all five expansion packs; install any pack live with its Quick Install block (see [expansions/README.md](modules/default/worlds/default_world/expansions/README.md)).

### Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `DATABASE_URL` | `repl.db` *(or `sqlite://mudl.db` via `.env`)* | SQLite database path |
| `DEFAULT_PLAYER` | `player:admin-001` | Player object used for session and spawn |
| `MUDL_MODULE` | `modules/default` | Module directory containing `universe.mudl` |
| `MUDL_WORLD` | *(universe default)* | Override active world within the module |
| `RUST_LOG` | `info` | Tracing verbosity |
| `IRC_*` | see [docs/IRC.md](docs/IRC.md) | IRC bot (TLS, nick, channels); `IRC_MOCK=1` for stdin testing |

See [`.env.example`](.env.example) for a ready-to-copy template.

### Try It Out

```text
> look
The Void
You are in a featureless void. This is the starting point for new players.

Exits: north

> create item "shiny pebble"
You conjure a shiny pebble, and it settles onto the ground in The Void.

> take pebble
You pick up the shiny pebble.

> i
You are completely naked.
You are carrying:
  shiny pebble — in your left hand

> go north
You head north.

> look
North Passage
A narrow passage leading north from the void.

Exits: south, north
```

**Player commands:** `look` / `l`, `examine` / `x`, `take` / `get`, `drop`, `inventory` / `i`, `go`, `attack`, `harvest`, `read`, `open` / `close`, `wield`, `wear`, `create`.

**Builder commands:** `@set`, `@unset`, `@examine`, `@trigger`, `@dig`, `@link`, `module reload`. Type `help` in the REPL for the full list.

---

## Documentation map

| Doc | Audience | Contents |
|-----|----------|----------|
| [ARCHITECTURE.md](ARCHITECTURE.md) | Contributors | Milestones M1–M5, module layout, concurrency, roadmap |
| [SECURITY.md](SECURITY.md) | Operators & contributors | M5 security review, threat model, findings (SEC-*) |
| [docs/IRC.md](docs/IRC.md) | Players & operators | IRC bot setup, commands, channels, nick handling, testing |
| [LANGUAGE.md](LANGUAGE.md) | Builders | MUDL syntax: creatures, `@trigger`, combat, spawners, expansions |
| [COMMANDS.md](COMMANDS.md) | Players & builders | REPL + IRC command reference and output style |
| [BUILDER.md](BUILDER.md) | Builders | `@set` / `@unset`, properties vs state vs status |
| [OBJECT_MODEL.md](OBJECT_MODEL.md) | Contributors | `Object`, roles, IDs, display modes |
| [MODULES.md](MODULES.md) | Expansion authors | Pack README template and authoring rules |
| [docs/REPL.md](docs/REPL.md) | Developers | REPL setup, examples, persistence behavior |
| [modules/default/README.md](modules/default/README.md) | Builders | Default universe layout |
| [expansions/README.md](modules/default/worlds/default_world/expansions/README.md) | Players & builders | Official adventure packs |

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│              Frontends (IRC • REPL • Files • GitHub)         │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│         Gateway (`SessionManager` + RBAC on actor perms)      │
│         IRC bot • REPL • (future: WebSocket / GitHub)         │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                     Core Engine (Rust)                       │
│  Object Model • MUDL Loader • MoveManager • Display          │
│  Creatures & Combat • Events (@trigger) • SQLite           │
└─────────────────────────────────────────────────────────────┘
```

When a player enters a room, the engine runs subscribers (scheduler, spawners) then **host** `@trigger` scripts, then creature behaviors and condition ticks:

```mermaid
flowchart LR
  A[go dir] --> B[on_leave]
  B --> C[move player]
  C --> D[execute_event on_enter]
  D --> E[subscribers: scheduler + spawners]
  E --> F[host @trigger scripts]
  F --> G[creature behaviors]
  G --> H[look + regen + condition ticks]
```

**Key principles** ([ARCHITECTURE.md](ARCHITECTURE.md)):

- The **core engine is pure** — it knows nothing about IRC, auth, or transport.
- **World content** lives in MUDL; **physics** (movement rules, combat math) stays in Rust until a sandboxed runtime exists.
- **Host** = the object whose `@trigger` handlers run for an event (usually the room, item, or creature being acted on).

---

## Extending via MUDL

World content lives in **modules** — self-contained universe packs:

```
modules/default/
├── universe.mudl
└── worlds/default_world/
    ├── world.mudl         # @world entrypoint + @import expansions
    ├── map.mudl           # Areas (type: area) and exits
    ├── creatures.mudl     # @creature, @effect, @stat, @skill
    ├── behaviors.mudl     # @behavior-template personalities
    ├── npcs.mudl          # @npc, @spawner, @loot-spawner
    ├── players.mudl       # @player-template (creature=human)
    ├── items.mudl         # Scene items and prototypes
    ├── objects.mudl       # Shared prototypes
    └── expansions/        # Drop-in adventure packs
```

Fork the module, edit `.mudl` files, and run with `MUDL_MODULE=modules/my-universe`. Reload in the REPL with `module reload`.

Module layout: [modules/default/README.md](modules/default/README.md) · Expansion install: [expansions/README.md](modules/default/worlds/default_world/expansions/README.md) · Overview: [MODULES.md](MODULES.md)

---

## Current Status

### Implemented (Milestones 1–5)

- Object graph, `MoveManager`, inventory, SQLite persistence, REPL `Session`
- MUDL loader: universes, worlds, `@include` / `@import`, expansion packs
- Creatures: vitals, equipment, combat, death, behaviors, awareness, spawners, loot
- Events: `@trigger`, `execute_event`, scheduler, resource/loot spawners, `on_harvest`
- Conditions: `@effect` with DoT/HoT, `condition_ticks`, script `grant-effect` / `cure-tag`
- Five official expansion packs with self-contained READMEs
- **M5 multi-user IRC** — `SessionManager`, `IrcBot`, TLS/IRCv3, room visibility, tells, OOC, disconnect persist
- **532** unit and integration tests (incl. `gateway::multi_user`, `load`, `edge_cases`, `m5_scenarios`)

### Planned (post-M5)

- **IRC combat & builder parity** — `attack`, `@dig`, full meta-command surface over IRC
- **Rate limiting** — flood protection on IRC command dispatch
- **Sandboxed verb runtime** — replace hardcoded `event_script` actions with safe DSL execution
- **File + GitHub hot-reload** — webhooks and live module updates

Track active work on [GitHub Issues](https://github.com/brainswax/mudl/issues).

---

## Contributing

Contributions are welcome — Rust engine work, MUDL content, documentation, and tests.

1. Open an issue on [GitHub](https://github.com/brainswax/mudl/issues) for larger changes.
2. Fork, branch, and PR against `main`.
3. Run `make dev` or `cargo test` before submitting.
4. Prefer **modules over core patches** — great `.mudl` content and thin engine hooks beat bespoke Rust.

---

## License

Licensed under the [GNU Affero General Public License v3.0](LICENSE). Network server deployments must share corresponding source under the same license.