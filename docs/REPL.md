# MUDL REPL Documentation

The MUDL REPL is the primary way to play and build today. It wraps `repl::Session` — object graph, movement, combat, events, and SQLite persistence — in a thin command-line adapter. The IRC bot ([IRC.md](IRC.md)) uses the same `SessionManager` + `PlayerSession` model for multi-user play over **IRCv3 + TLS** (port 6697 by default).

All data persists in SQLite (`repl.db` by default) between runs.

## Prerequisites

- Rust toolchain (`cargo`)
- Optional: copy [`.env.example`](../.env.example) for `DATABASE_URL` and logging

## Build & Run

```bash
cargo build --bin repl
cargo run --bin repl
# or: make run-repl
```

```text
Welcome to MUDL.
Type 'help' for commands.
>
```

Bootstrap and persistence log to stderr when `RUST_LOG=info` — not on the interactive prompt.

### Module loading

```bash
MUDL_MODULE=modules/default cargo run --bin repl
MUDL_UNIVERSE=path/to/universe.mudl cargo run --bin repl
MUDL_WORLD=default_world cargo run --bin repl
```

Default: `modules/default/universe.mudl` → `default_world` (naked human, starter map, five expansion packs pre-imported).

## Commands

Type `help` for the full list. Summary:

| Command | Description |
|---------|-------------|
| `look` / `l [target]` | In-character brief view |
| `examine` / `x [target]` | In-character detail (`self`, `self body`, `.parent`) |
| `@look` / `@examine [target]` | Builder structured view |
| `@dump [target]` | Full JSON debug dump |
| `create <type> <name...>` | Create object at current location |
| `@create <type> <name...> [key=value...]` | Wizard create with roles |
| `go <dir>` | Move between places |
| `inventory` / `i` | Slot-by-slot carry list |
| `get` / `take`, `drop`, `put`, `remove`, `wield`, `wear` | Inventory verbs |
| `read <object>` | Read signs, notes, mailboxes |
| `open` / `close`, `lock` / `unlock` | Containers, doors, windows |
| `break` / `smash <item>` | Break breakables |
| `harvest` / `gather <object>` | Harvest resource nodes |
| `attack <creature>` | Turn-based combat |
| `@set` / `@unset <target> <key> [value]` | Wizard property/state/verb edit |
| `@trigger …` | Attach event scripts (`@trigger help`) |
| `@dig`, `@link`, `@unlink` | Place building |
| `@damage` / `@heal` | Wizard vitals |
| `@addbehavior` / `@listbehaviors` | Creature AI templates |
| `@delete` / `@undelete` | Soft-delete objects |
| `@keyfor <container>` | Create matching key |
| `load` / `save <id>` | Cache ↔ database |
| `module reload` | Reload MUDL from disk |
| `module bundle <dir>` | Package module |
| `list` | Objects in session memory |
| `exit` / `quit` | Quit |

**Note:** `add_prop` and `add_verb` are replaced by `@set` / `@unset` (see [BUILDER.md](../BUILDER.md)).

## Output philosophy

| Tier | Commands | What you see |
|------|----------|--------------|
| **Player** | `look`, `take`, `go`, `attack`, … | Immersive prose — no IDs |
| **Player detail** | `examine` | Weight, capacity, gear, anatomy summary |
| **Builder** | `@examine`, `@set`, `@trigger`, … | Properties, state, status, anatomy |
| **Debug** | `@dump`, `RUST_LOG` | Full JSON and engine logs |

See [LANGUAGE.md](../LANGUAGE.md#player-facing-output) and [COMMANDS.md](../COMMANDS.md) for style rules.

**Target resolution:** Optional target defaults to current room. Use `here`, `self` / `me`, friendly names, or full object IDs. Possession is searched before room ground.

## Example session

### Explore

```text
> look
You are in a featureless void. This is the starting point for new players.

Obvious exits: north

> go north
You head north.

> l
North Passage
A narrow passage leading north from the void.

Obvious exits: north, south
```

### Items and inventory

```text
> create sword Rusty Sword
You forge a Rusty Sword, and it clatters to the ground in North Passage.
> take rusty sword
You pick up a Rusty Sword.
> examine self
You're a human carrying a Rusty Sword. You have a carry capacity of 1/10 and are carrying 1 of 100 weight.
> inventory
You are completely naked.
You are carrying:
  Rusty Sword — in your right hand
```

### Builder edit

```text
> @set sword description "A rusty old blade."
> @set sword hand_slot right
> @examine sword
name: Rusty Sword
type: item
...
```

### Install an expansion (from any room)

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/haunted_forest/haunted_forest.mudl
@create portal Haunted Forest door_direction=in door_destination=haunted-entry
@link in haunted-entry --return out
```

Then `go in`. Full pack guides: [expansions/README.md](../modules/default/worlds/default_world/expansions/README.md).

## How the REPL fits the engine

```
repl.rs (thin CLI)
    └── repl::Session
            ├── Object graph + DirtyTracker
            ├── MoveManager (inventory, go)
            ├── execute_event (@trigger bus)
            ├── creature combat / behaviors / conditions
            └── SqlitePersistence
```

- **ObjectFactory** — stable `type:slug-###` IDs, immediate save on create
- **Display** — `look` / `examine` / `@examine` / `@dump` via `DisplayMode`
- **Inventory** — `src/inventory/` delegates to `MoveManager`
- **Events** — room `go` orchestrates spawners, triggers, behaviors, condition ticks
- **Cache** — session holds active objects; most mutations auto-persist

Source: `src/bin/repl.rs`, `src/repl/session.rs`.

## Tips

- Delete `repl.db` (or your `DATABASE_URL` file) to start fresh.
- Use `@dump` or `RUST_LOG=info` when you need internal IDs.
- `@delete` hides objects from play; `@undelete <id>` restores them.
- Run `cargo test` or `make dev` — **532** tests cover bootstrap, combat, events, persistence, IRC, and multi-user scenarios.
- Run `make test-m5` for IRC and gateway multi-user tests only.
- Multi-user play over IRC: [IRC.md](IRC.md).