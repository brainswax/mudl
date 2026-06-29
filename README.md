# MUDL

**A self-modifying MUD/MOO engine with a custom DSL — IRC-first, programmable worlds.**

MUDL is a Rust-powered text world engine for builders who want MOO-style live modification without giving up version control, persistence, or sane architecture. Define rooms, creatures, and player templates in flat `.mudl` files; explore and extend them through a REPL today and IRC tomorrow. New players spawn as a **naked human** — full anatomy slots, empty hands — ready for you to dress the world around them.

[![Rust](https://img.shields.io/badge/Rust-000000?style=flat&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Tokio](https://img.shields.io/badge/Tokio-000000?style=flat&logo=rust&logoColor=white)](https://tokio.rs/)
[![SQLite](https://img.shields.io/badge/SQLite-07405E?logo=sqlite&logoColor=white)](https://www.sqlite.org/)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPLv3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)

**Repository:** [github.com/brainswax/mudl](https://github.com/brainswax/mudl)

---

## Why MUDL?

If you have ever wanted a MUD where the **world is data you can edit live**, where **everything is an object** with prototype inheritance, and where **content lives in Git-friendly files** instead of opaque server state — this project is for you. MUDL targets three audiences:

- **MUD veterans** who miss programmable worlds but want modern persistence and tooling.
- **Rust developers** interested in async engines, DSL design, and object graphs backed by SQLite.
- **World builders** who want a readable language for rooms, creatures, anatomy, and verbs — without recompiling the server for every change.

The long-term vision is IRC-first play, multi-modal authoring (REPL, files, GitHub), and safe runtime self-modification. The REPL and MUDL loader are working now; IRC and the gateway layer are next.

---

## Features

| Area | What you get |
|------|----------------|
| **MUDL DSL** | Declarative `.mudl` files for universes, worlds, maps, creatures, and player templates |
| **Object model** | Prototype-based inheritance — rooms, items, players, NPCs, and abstract systems are all `Object`s |
| **Anatomy & inventory** | Creature `@slot` definitions (grasp, wear, limb); take, drop, wield, wear, and container commands |
| **Live extension** | Add properties and verbs at runtime via the REPL (`add_prop`, `add_verb`) |
| **Persistence** | SQLite with stable `type:base-name-###` IDs and full object serialization |
| **Flat modules** | Git-friendly layout under `modules/default/` — no deep nesting required |
| **Naked human baseline** | Default player uses the `human` creature template with empty anatomy slots |
| **Clean architecture** | Pure core engine; gateway RBAC (Player / Builder / Wizard) planned as a thin enforcement layer |
| **Multi-modal input** | REPL today; IRC bot, file hot-reload, and GitHub integration on the roadmap |

---

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (2021 edition toolchain)
- SQLite (bundled via `sqlx`)

### Build & Run

```bash
# Clone and enter the repo
git clone git@github.com:brainswax/mudl.git
cd mudl

# Optional: copy environment defaults
cp .env.example .env

# Build and start the REPL
cargo build --bin repl
cargo run --bin repl

# Or use the Makefile
make run-repl
```

On startup you should see something like:

```text
MUDL REPL starting...
Using database: repl.db
Default owner: player:admin-001
Type 'help' for commands.
Loading module: modules/default
Loaded universe 'default' / world 'default_world' (6 sources, human creature)
Bootstrapping world if needed...
Restored 5 object(s) from database.
Current location: area:the-void-001
>
```

The REPL loads `modules/default/universe.mudl`, bootstraps `default_world` if the database is empty, and places you in **The Void** as a naked human with no starting gear.

### Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `DATABASE_URL` | `repl.db` *(or `sqlite://mudl.db` via `.env`)* | SQLite database path |
| `DEFAULT_PLAYER` | `player:admin-001` | Player object used for session and spawn |
| `MUDL_MODULE` | `modules/default` | Module directory containing `universe.mudl` |
| `MUDL_WORLD` | *(universe default)* | Override active world within the module |
| `RUST_LOG` | `info` | Tracing verbosity |

See [`.env.example`](.env.example) for a ready-to-copy template.

### Try It Out

Player-facing commands work out of the box:

```text
> look
The Void
You are in a featureless void. This is the starting point for new players.

Exits: north

> create item "shiny pebble"
Created: shiny pebble (item:shiny-pebble-00a) at area:the-void-001

> take pebble
You pick up the shiny pebble.

> i
You are completely naked.
You are carrying:
  shiny pebble (in left_hand)

> go north
You go north.

> look
North Passage
A narrow passage leading north from the void.

Exits: south, north
```

**Common commands:** `look` / `l`, `examine` / `x`, `take` / `get`, `drop`, `inventory` / `i`, `go <dir>`, `wield`, `wear`, `create <type> <name...>`.

**Builder commands:** `add_prop`, `add_verb`, `load`, `save`, `@dump`, `module reload`, `module bundle`.

Full command reference: [docs/REPL.md](docs/REPL.md).

### Development

```bash
make help      # list Makefile targets
make dev       # fmt + check + clippy + test
cargo test     # 38 unit tests covering loader, inventory, persistence
```

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│              Frontends (IRC • REPL • Files • GitHub)         │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│              API Gateway + RBAC (planned)                    │
│         Player / Builder / Wizard permission tiers           │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                     Core Engine (Rust + Tokio)               │
│  Object Model • MUDL Loader • Commands • Inventory           │
│  Display Layer • Events (planned) • SQLite Persistence       │
└─────────────────────────────────────────────────────────────┘
```

**Key principles** (full detail in [ARCHITECTURE.md](ARCHITECTURE.md)):

- The **core engine is pure** — it knows nothing about IRC, auth, or transport.
- **All world mutations** will flow through a gateway layer for RBAC enforcement.
- **The world is data + behavior** defined primarily in MUDL files and runtime verbs.
- **Prototype inheritance** and **runtime modification** are first-class design goals.

**Deep dives:**

- Object model, IDs, permissions, inheritance — [OBJECT_MODEL.md](OBJECT_MODEL.md)
- MUDL language goals and syntax — [LANGUAGE.md](LANGUAGE.md)
- REPL commands and display modes — [docs/REPL.md](docs/REPL.md)

---

## Extending via MUDL

World content lives in **modules** — self-contained universe packs. The official baseline uses a **flat file layout** (no subfolders inside a world):

```
modules/default/
├── universe.mudl              # @universe + default world pointer
└── worlds/
    └── default_world/
        ├── world.mudl         # @world entrypoint + @include list
        ├── map.mudl           # Rooms (type: area) and exits
        ├── creatures.mudl     # @creature anatomy (@slot definitions)
        ├── players.mudl       # @player-template (creature=human)
        ├── items.mudl         # Item prototypes (placeholder)
        └── objects.mudl       # Shared prototypes (placeholder)
```

### Naked Human Baseline

The default player template spawns with the `human` creature — ten anatomy slots, nothing equipped:

```mudl
# creatures.mudl
@creature human
  @slot left_hand  capacity=1 type=grasp hands=1
  @slot right_hand capacity=1 type=grasp hands=1
  @slot head       capacity=1 type=wear
  @slot torso      capacity=1 type=wear
  @slot left_arm   capacity=1 type=limb
  @slot right_arm  capacity=1 type=limb
  @slot left_leg   capacity=1 type=limb
  @slot right_leg  capacity=1 type=limb
  @slot left_foot  capacity=1 type=wear
  @slot right_foot capacity=1 type=wear
@end
```

```mudl
# players.mudl
@player-template default
  creature=human
  gender=neutral
@end
```

Running `i` with no gear reports: *"You are completely naked and empty-handed."*

### Example Map Fragment

```mudl
# map.mudl
type: area
base_name: the-void
name: The Void
description: You are in a featureless void. This is the starting point for new players.

exits:
  north: north-passage
```

### Fork Your Own Universe

```bash
# Copy the baseline module
cp -r modules/default modules/my-universe

# Edit the .mudl files (add worlds, creatures, rooms, etc.)
# Then run with your module:
MUDL_MODULE=modules/my-universe cargo run --bin repl

# Or point at an example pack:
MUDL_MODULE=examples/my-universe MUDL_WORLD=my_world cargo run --bin repl
```

Add a new creature in `creatures.mudl`, reference it from a `@player-template`, and reload with `module reload` in the REPL. Verbs and event handlers can be added in MUDL (as the interpreter matures) or live via `add_verb`.

Module layout reference: [modules/default/README.md](modules/default/README.md) · Example packs: [examples/README.md](examples/README.md)

---

## Current Status & Roadmap

### Implemented

- **Core object model** — `Object`, `Property`, `Verb`, prototype resolution, `PermissionFlags`
- **ObjectFactory** — stable ID generation (`type:slug-###`), soft-delete support
- **SQLite persistence** — object round-trips, per-type counters, session restore
- **MUDL loader** — parses `universe.mudl`, `@world`, `@include`, map definitions, `@creature`, `@player-template`
- **World bootstrap** — creates rooms, exits, and player from flat module files
- **Anatomy registry** — grasp / wear / limb slots drive inventory placement
- **Player commands** — `look`, `examine`, `go`, `take`, `drop`, `put`, `remove`, `wield`, `wear`, `create`
- **Display layer** — player, builder, and debug modes
- **REPL** — interactive session with module reload and bundle export
- **Tests** — loader, inventory, persistence, and anatomy coverage

### Planned / In Progress

- **Executable verbs & events** — runtime that makes the world truly self-modifying
- **Item & object prototypes** — populate `items.mudl` and `objects.mudl` in the loader
- **IRC bot frontend** — play and build from any IRC client
- **API gateway / RBAC** — enforce Player / Builder / Wizard tiers on all mutations
- **File + GitHub loaders** — hot-reload module changes from disk or webhooks
- **Rich verb DSL** — safe sandboxed execution for live and LLM-generated code

Track active work on [GitHub Issues](https://github.com/brainswax/mudl/issues) and open branches.

---

## Contributing

Contributions are welcome — Rust engine work, MUDL language design, example universes, documentation, and tests.

1. **Open an issue** on [GitHub](https://github.com/brainswax/mudl/issues) to discuss larger changes.
2. **Fork, branch, and PR** against `main`.
3. **Run checks** before submitting: `make dev` or `cargo test`.
4. **Prefer modules over core patches** — the best extensions are often great `.mudl` content and thin engine hooks, not bespoke core logic.

**Guidelines:**

- Keep the engine pure; put transport and auth in frontend/gateway layers.
- Match existing code style (`cargo fmt`, `cargo clippy`).
- Add tests for behavior changes.
- Update docs when adding user-facing commands or MUDL syntax.

---

## License

Licensed under the [GNU Affero General Public License v3.0](LICENSE).

Network server deployments must share corresponding source under the same license. See [LICENSE](LICENSE) for the full text.