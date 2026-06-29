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

When it starts, you will see:

```
MUDL REPL starting...
Using database: repl.db
Default owner: player:admin-001
Type 'help' for commands.
Bootstrapping default world if needed...
Bootstrap complete. Starting at: room:the-void-001
>
```

The REPL creates (or opens) a file called `repl.db` in the current directory for SQLite storage.

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
| `create <type> <base_name>` | Create a new object using the ObjectFactory   | `create room cozy-kitchen`           |
| `list`                   | Show all objects currently in the session cache  | `list`                               |
| `look [target]` (`l`)    | Immersive player view (current room if no target) | `look`, `look here`, `look daisy`   |
| `examine [target]` (`x`) | Builder view with IDs, properties, and verbs     | `examine room:central-hub-001`       |
| `@dump [target]`         | Full JSON dump of an object (debug mode)         | `@dump room:the-void-001`            |
| `go <dir>`               | Move in a direction from the current room        | `go north`                           |
| `inventory` (`i`)        | Show hands, pockets, worn containers, and contents | `i`                                |
| `get` / `take <item>`    | Pick up an item from the room                    | `take coin`                          |
| `drop <item>`            | Drop a carried item into the room                | `drop coin`                          |
| `put <item> in <container>` | Stow a carried item in a container            | `put coin in wallet`                 |
| `remove <item> from <container>` | Take an item out of a container          | `remove coin from wallet`            |
| `wield <item>`           | Hold or wield an item in your hand(s)             | `wield sword`                        |
| `wear <item>`            | Wear a container or garment                      | `wear backpack`                      |
| `add_prop <id> <name> <value>` | Add (or overwrite) a string property on an object | `add_prop room:cozy-kitchen-001 description "A warm and inviting kitchen."` |
| `add_verb <id> <name> <code>` | Add a verb (with code) to an object            | `add_verb room:cozy-kitchen-001 bake "say('You bake some bread!')"` |
| `load <id>`              | Load an object from the database into the cache  | `load room:cozy-kitchen-001`         |
| `save <id>`              | Save an object from the cache to the database    | `save room:cozy-kitchen-001`         |
| `module reload`          | Reload MUDL module from disk                     | `module reload`                      |
| `module bundle <dir>`    | Package module to output directory               | `module bundle dist/default`         |
| `exit` or `quit`         | Exit the REPL                                    | `exit`                               |

**Notes on commands:**
- Object IDs follow the `type:base-name-counter` format (e.g. `room:cozy-kitchen-001`).
- Most commands that modify objects will automatically save changes to the database.
- The REPL keeps a small in-memory cache of objects you've recently created or loaded.

### Display Modes

The REPL uses three display modes from the presentation layer:

| Command | Mode | What you see |
|---------|------|--------------|
| `look` / `l` | Player | Name, description, obvious exits, visible contents — no internal IDs |
| `examine` / `x` | Builder | Owner, location, properties, verbs, exit targets, contents with IDs |
| `@dump` | Debug | Full JSON serialization of the object |

**Target resolution:** Commands accept an optional target. Omit the target to use your current location. Use `here` explicitly, `self` / `me` for your player, a friendly name (e.g. `Daisy`), an alias, or a full object ID.

### Anatomy and Inventory

Players spawn as **naked humans** from the active world's `creatures.mudl` (`@creature human`) — biological body slots only, no pockets or clothing by default.

- **In hands** — `left_hand` / `right_hand` grasp slots; two-handed items (`hand_slot: both`) occupy both
- **Worn** — items on `wear` slots (e.g. `torso` for a backpack via `wear_slot`)
- **Inside containers** — nested via each container's `contents` list

Example output:

```text
> look self
Admin
You are completely naked and empty-handed.

> take sword
You take the Rusty Sword.
> look self
Admin
You are holding Rusty Sword in your right hand.
```

Use `inventory` for a structured slot listing. Pockets will arrive later via clothing items.

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
You go north.
> l
North Passage
A narrow passage leading north from the void.

Obvious exits: north, south
```

### 3. Examine with builder details

```
> examine
North Passage [room:north-passage-001]
Owner: player:admin-001
Description: A narrow passage leading north from the void.
Exits: north -> room:central-hub-001, south -> room:the-void-001
Properties:
  description = A narrow passage leading north from the void.
  exits = {north: room:central-hub-001, south: room:the-void-001}
Verbs:
  (none)
```

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
Admin
You are completely naked and empty-handed.
> inventory
You are completely naked.
  empty-handed
```

### 6. Pick up and wield items

```
> create item sword
Created: sword (item:sword-001)
> add_prop item:sword-001 description "A rusty old blade."
Property added.
> add_prop item:sword-001 hand_slot right
Property added.
> take sword
You take the sword.
> look self
Admin
You are holding sword in your right hand.
```

### 7. Create and inspect an item

```
> create item daisy
Created: daisy (item:daisy-001)
> examine daisy
daisy [item:daisy-001]
Owner: player:admin-001
Properties:
  (none)
Verbs:
  (none)
```

### 8. Add a property and look at it

```
> add_prop item:daisy-001 description "A cheerful yellow flower."
Property added.
> look daisy
daisy
A cheerful yellow flower.
```

## How the REPL Uses the Core Components

- **ObjectFactory**: The REPL creates an `ObjectFactory` that wraps the persistence layer. When you run `create`, it calls `factory.create(type_name, base_name, owner)`. The factory:
  - Asks the persistence layer for the next counter value for that type + base name.
  - Generates a unique `ObjectId` in the `type:base-name-counter` format (using 3-digit hex).
  - Increments the counter in the database.
  - Builds a fresh `Object` with default values.
  - Saves the new object immediately.

- **Display Layer**: `look`, `examine`, and `@dump` route through the `Describable` trait with `DisplayMode::Player`, `DisplayMode::Builder`, and debug dump respectively. The REPL loads all known objects to resolve room contents and name-based targets.

- **Inventory Layer**: `get`, `drop`, `put`, `remove`, `wield`, and `wear` route through `src/core/inventory.rs`. Inventory state is stored as object properties (`pockets`, `left_hand`, `right_hand`, `worn`, `contents`, `carried_slot`) and serializes cleanly via the existing SQLite persistence.

- **Persistence Layer**: The REPL uses `SqlitePersistence` (an implementation of the `Persistence` trait). This handles:
  - Saving and loading full `Object` structs as JSON in SQLite.
  - Tracking per-type/base-name counters so IDs remain unique even after restarts.
  - All `save` and `load` commands go through this layer.

- **In-memory cache**: The REPL maintains a simple `HashMap` of recently used objects so you don't have to reload everything manually. Commands like `add_prop` and `add_verb` work on the cached copy and then persist the changes.

This design keeps the REPL thin while demonstrating how the real system will use the Object Model and persistence abstraction.

## Tips for Experimentation

- You can create different types: `create item silver-sword`, `create npc old-mage`, etc.
- Use friendly names with `look` and `examine`; use full IDs or `@dump` when you need precision.
- Changes are saved automatically in most cases, but you can explicitly use `save` if needed.
- The database file `repl.db` can be deleted to start fresh.

If you run into issues or want to extend the REPL, feel free to look at `src/bin/repl.rs` and the files under `src/core/`.