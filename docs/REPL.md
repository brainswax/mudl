# MUDL REPL Documentation

The MUDL REPL is a simple command-line tool for experimenting with the core Object Model, ObjectFactory, and persistence layer. It provides an interactive way to create, inspect, modify, save, and load objects without needing the full IRC frontend.

This is intended as a local development and testing environment. All data is stored in a SQLite database (`repl.db` by default) so your objects persist between runs.

## Prerequisites

- Rust toolchain installed (including `cargo`)
- Basic familiarity with the command line

## Building the REPL

From the project root, build the REPL binary:

```bash
cargo build --bin repl
```

This compiles the project and produces the executable at `target/debug/repl`.

## Running the REPL

Start the REPL with:

```bash
cargo run --bin repl
```

Or run the built binary directly:

```bash
./target/debug/repl
```

When it starts, you will see a minimal welcome:

```text
Welcome to MUDL.
Type 'help' for commands.
>
```

Bootstrap, database paths, module loading, and object counts are logged via `tracing` (set `RUST_LOG=info` to see them on stderr). They are intentionally hidden from the interactive prompt so play stays immersive.

The REPL creates (or opens) a SQLite database (`repl.db` by default, or `DATABASE_URL` from `.env`) for persistence.

By default it loads `modules/default/universe.mudl` and bootstraps the `default_world` (naked human, starter rooms). Override with:

```bash
MUDL_MODULE=modules/default cargo run --bin repl
# or
MUDL_UNIVERSE=path/to/universe.mudl cargo run --bin repl
# select a world within the universe
MUDL_WORLD=default_world cargo run --bin repl
```

Type `help` at the prompt to see the list of commands at any time.

## Available Commands

| Command                  | Description                                      | Example                              |
|--------------------------|--------------------------------------------------|--------------------------------------|
| `help`                   | Show the list of available commands              | `help`                               |
| `create <type> <name...>` | Create a new object at your current location  | `create sword Rusty Sword`           |
| `list`                   | Builder: names in session working memory         | `list`                               |
| `look [target]` (`l`)    | Immersive player view (current room if no target) | `look`, `look here`, `look daisy`   |
| `examine [target]` (`x`) | In-game detail; `self` shows equipment + weight  | `examine self`, `examine self body`, `examine coins.parent` |
| `@examine [target] [parent]` | Wizard: properties, anatomy, prototype chain | `@examine self`, `@examine coins parent` |
| `@dump [target]`         | Full JSON dump of an object (debug mode)         | `@dump room:the-void-001`            |
| `@create <type> <name...> [key=value...]` | Wizard create with role options | `@create container "Leather Bag" capacity=8 max_weight=40` |
| `go <dir>`               | Move in a direction from the current room        | `go north`                           |
| `inventory` (`i`)        | Show hands, pockets, worn containers, and contents | `i`                                |
| `get` / `take <item>`    | Pick up an item from the ground (not held items) | `take sword`                         |
| `drop <item>`            | Drop a carried item into the room                | `drop coin`                          |
| `put [count] <item> in <container>` | Stow carried items (optional stack count) | `put 10 coins in purse`              |
| `remove <item> from <container>` | Take an item out of a container          | `remove coin from wallet`            |
| `wield <item>`           | Hold or wield an item in your hand(s)             | `wield sword`                        |
| `wear <item>`            | Wear a container or garment                      | `wear backpack`                      |
| `add_prop <id> <name> <value>` | Add (or overwrite) a string property on an object | `add_prop room:cozy-kitchen-001 description "A warm and inviting kitchen."` |
| `add_verb <id> <name> <code>` | Add a verb (with code) to an object            | `add_verb room:cozy-kitchen-001 bake "say('You bake some bread!')"` |
| `load <id>`              | Load an object from the database into the cache  | `load room:cozy-kitchen-001`         |
| `save <id>`              | Save an object from the cache to the database    | `save room:cozy-kitchen-001`         |
| `module reload`          | Reload MUDL module from disk                     | `module reload`                      |
| `module bundle <dir>`    | Package module to output directory               | `module bundle dist/default`         |
| `@delete <target>`       | Wizard: soft-delete an object (kept in DB)       | `@delete boots`                      |
| `@undelete <id>`         | Wizard: restore a soft-deleted object            | `@undelete item:boots-001`           |
| `exit` or `quit`         | Exit the REPL                                    | `exit`                               |

**Notes on commands:**
- Object IDs follow the `type:base-name-counter` format internally (e.g. `room:cozy-kitchen-001`). Player commands never print them.
- Most commands that modify objects will automatically save changes to the database.
- The REPL keeps a small in-memory cache of objects you've recently created or loaded.

### Output Philosophy

MUDL aims for **MOO-like immersion**: player commands speak in narrative prose; builder commands stay contextual but avoid raw struct dumps; only debug commands expose engine internals.

| Command | Mode | What you see |
|---------|------|--------------|
| `look` / `l`, `take`, `create`, `go`, `inventory`, … | **Player** | Immersive text — names, descriptions, exits, natural inventory. No IDs. |
| `examine` / `x` | **Player** | Weight, capacity, carried gear, body plan summary; `examine <obj>.parent` for prototype properties |
| `@examine`, `load`, `save`, `list`, `@set`, … | **Builder** | Structured fields — owner, properties, anatomy slots, inherited prototype values |
| `@dump` | **Debug** | Full JSON serialization of the object |

Technical bootstrap and persistence events log to stderr when `RUST_LOG=info` (or higher). See [LANGUAGE.md](../LANGUAGE.md#player-facing-output) for the full output model and future MUDL customization hooks.

**Target resolution:** Commands accept an optional target. Omit the target to use your current location. Use `here` explicitly, `self` / `me` for your player, a friendly name (e.g. `Daisy`), an alias, or a full object ID.

### Anatomy and Inventory

Players spawn as **naked humans** from the active world's `creatures.mudl` (`@creature human`) — biological body slots only, no pockets or clothing by default.

- **In hands** — `left_hand` / `right_hand` grasp slots; two-handed items (`hand_slot: both`) occupy both
- **Worn** — items on `wear` slots (e.g. `torso` for a backpack via `wear_slot`)
- **Inside containers** — nested via each container's `contents` list
- **On the ground** — items with `location` set to your current area/room appear in `look` as `You see: …`

`create <type> <name...>` places the new object at your current location (area, room, or any navigable place). Multi-word names are supported; IDs are lowercase slugs capped at 16 characters (`Rusty Sword` → `sword:rusty-sword-001`). Quote names if needed: `create sword "Rusty Sword"`.

Options are separate from the name: `create container purse capacity=3 max_weight=10` creates an object named **purse** with ID `item:purse-001`, not `purse capacity=3 max_weight=10`.

`take` / `get` only search the ground in your current location — items already in your hands are ignored. One ground match takes silently; multiple ground matches prompt "Which X do you mean?".

`@create` supports role-aware types: `container`, `wearable`, `stackable`, plus `key=value` options (`capacity`, `max_weight`, `max_volume`, `count`, `prototype`). Example:

```text
> @create container "Leather Bag" capacity=8 max_weight=40
You forge a Leather Bag, and it clatters to the ground in The Void.
> @create stackable "Gold Coin" count=25
You forge a Gold Coin, and it clatters to the ground in The Void.
> put 10 coins in purse
You put 10 coins in your purse.
> put coins in purse
You put 10 coins in your purse. 10 won't fit.
> look purse
purse
Inside the purse: 10 coins
```

Example output:

```text
> create sword Rusty Sword
You forge a Rusty Sword, and it clatters to the ground in The Void.
> look
The Void
You are in a featureless void.
You see: Rusty Sword
> take rusty sword
You pick up the Rusty Sword.
> look self
You are holding a Rusty Sword.
> inventory
You are completely naked.
You are carrying:
  Rusty Sword — in your right hand
```

`look self` summarizes what you carry in natural language. `inventory` lists each item with its body slot. Pockets will arrive later via clothing items.

## Usage Examples

### 1. Look around (player view)

After bootstrap, your player starts in The Void:

```
> look
The Void
You are in a featureless void. This is the starting point for new players.

Obvious exits: north
```

### 2. Move and look again

```
> go north
You head north.
> l
North Passage
A narrow passage leading north from the void.

Obvious exits: north, south
```

### 3. Examine with builder details

```
> examine
North Passage
Owner: you
A narrow passage leading north from the void.
Exits: north to Central Hub, south to The Void
Present: Admin
Properties:
  description: A narrow passage leading north from the void.
  exits: {north: North Passage, south: The Void}
Verbs:
  (none)
```

Builder `examine` resolves exit targets and owners to display names — no raw IDs unless you `@dump`.

### 4. Debug dump

```
> @dump room:the-void-001
{
  "id": "room:the-void-001",
  "name": "The Void",
  ...
}
```

### 5. Look at yourself and check inventory

```
> look self
You aren't holding or wearing anything.
> inventory
You are completely naked.
Your hands are empty.
```

### 6. Pick up and wield items

```
> create item sword
You conjure a sword, and it settles onto the ground in The Void.
> add_prop item:sword-001 description "A rusty old blade."
You inscribe "description" upon sword.
> add_prop item:sword-001 hand_slot right
You inscribe "hand_slot" upon sword.
> take sword
You pick up the sword.
> look self
You are holding a sword.
```

### 7. Create and inspect an item

```
> create item daisy
You conjure a daisy, and it settles onto the ground in The Void.
> examine daisy
daisy
Owner: you
Properties:
  (none)
Verbs:
  (none)
```

### 8. Add a property and look at it

```
> add_prop item:daisy-001 description "A cheerful yellow flower."
You inscribe "description" upon daisy.
> look daisy
daisy
A cheerful yellow flower.
```

## How the REPL Uses the Core Components

- **ObjectFactory**: The REPL creates an `ObjectFactory` that wraps the persistence layer. When you run `create`, `create_at_location()` calls the factory then sets `location` to your current place when one is set. The factory:
  - Asks the persistence layer for the next counter value for that type + base name.
  - Generates a unique `ObjectId` in the `type:base-name-counter` format (using 3-digit hex).
  - Increments the counter in the database.
  - Builds a fresh `Object` with default values.
  - Saves the new object immediately.

- **Display Layer**: `look`, `examine`, and `@dump` route through the `Describable` trait with `DisplayMode::Player`, `DisplayMode::Builder`, and debug dump respectively. Command feedback (`create`, `go`, `add_prop`, …) uses `display::narrative` for MOO-style messages. The REPL loads all known objects to resolve room contents and name-based targets.

- **Inventory Layer**: `get`/`take`, `drop`, `put`, `remove`, `wield`, and `wear` route through `src/inventory/` via `take_from_location()` in `src/command/`. Inventory state is stored as object properties (`body_slots`, `contents`, `carried_slot`) and serializes cleanly via the existing SQLite persistence.

- **Persistence Layer**: The REPL uses `SqlitePersistence` (an implementation of the `Persistence` trait). This handles:
  - Saving and loading full `Object` structs as JSON in SQLite.
  - Tracking per-type/base-name counters so IDs remain unique even after restarts.
  - All `save` and `load` commands go through this layer.

- **In-memory cache**: The REPL maintains a simple `HashMap` of recently used objects so you don't have to reload everything manually. Commands like `add_prop` and `add_verb` work on the cached copy and then persist the changes.

This design keeps the REPL thin while demonstrating how the real system will use the Object Model and persistence abstraction.

## Tips for Experimentation

- You can create different types: `create item silver-sword`, `create npc old-mage`, etc.
- Use friendly names with `look` and `examine`; use `@dump` or `RUST_LOG=info` when you need internal IDs and persistence detail.
- Changes are saved automatically in most cases, but you can explicitly use `save` if needed.
- The database file `repl.db` can be deleted to start fresh.
- All objects and state changes (inventory, location, created items) persist in SQLite. On restart the REPL hydrates the full world from the database and restores your location from the player object.
- Soft-deleted objects (`@delete`) are hidden from `look` but remain in the DB; use `@undelete <id>` to restore them.

If you run into issues or want to extend the REPL, feel free to look at `src/bin/repl.rs` and the files under `src/core/`.