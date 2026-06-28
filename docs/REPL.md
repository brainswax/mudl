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
>
```

The REPL creates (or opens) a file called `repl.db` in the current directory for SQLite storage.

Type `help` at the prompt to see the list of commands at any time.

## Available Commands

| Command                  | Description                                      | Example                              |
|--------------------------|--------------------------------------------------|--------------------------------------|
| `help`                   | Show the list of available commands              | `help`                               |
| `create <type> <base_name>` | Create a new object using the ObjectFactory   | `create room cozy-kitchen`           |
| `list`                   | Show all objects currently in the session cache  | `list`                               |
| `look <id>`              | Display full details of an object                | `look room:cozy-kitchen-001`         |
| `add_prop <id> <name> <value>` | Add (or overwrite) a string property on an object | `add_prop room:cozy-kitchen-001 description "A warm and inviting kitchen."` |
| `add_verb <id> <name> <code>` | Add a verb (with code) to an object            | `add_verb room:cozy-kitchen-001 bake "say('You bake some bread!')"` |
| `load <id>`              | Load an object from the database into the cache  | `load room:cozy-kitchen-001`         |
| `save <id>`              | Save an object from the cache to the database    | `save room:cozy-kitchen-001`         |
| `exit` or `quit`         | Exit the REPL                                    | `exit`                               |

**Notes on commands:**
- Object IDs follow the `type:base-name-counter` format (e.g. `room:cozy-kitchen-001`).
- Most commands that modify objects will automatically save changes to the database.
- The REPL keeps a small in-memory cache of objects you've recently created or loaded.

## Usage Examples

### 1. Create a new room

```
> create room cozy-kitchen
Created: cozy-kitchen (room:cozy-kitchen-001)
```

### 2. Look at the new object

```
> look room:cozy-kitchen-001
=== room:cozy-kitchen-001 ===
Name: cozy-kitchen
Owner: player:admin-001
Permissions: OWNER
Properties:
Verbs:
```

### 3. Add a property

```
> add_prop room:cozy-kitchen-001 description "A warm and inviting kitchen with the smell of fresh bread."
Property added.
```

### 4. Add a verb

```
> add_verb room:cozy-kitchen-001 bake "say('You bake a fresh loaf of bread!')"
Verb added.
```

### 5. Inspect the updated object

```
> look room:cozy-kitchen-001
=== room:cozy-kitchen-001 ===
Name: cozy-kitchen
Owner: player:admin-001
Permissions: OWNER
Properties:
  description = String("A warm and inviting kitchen with the smell of fresh bread.") (perms: OWNER)
Verbs:
  bake: say('You bake a fresh loaf of bread!') (perms: OWNER)
```

### 6. List cached objects

```
> list
Cached objects:
  room:cozy-kitchen-001 - cozy-kitchen
```

### 7. Exit the REPL

```
> exit
Goodbye!
```

You can restart the REPL later and use `load room:cozy-kitchen-001` to bring the object back into memory.

## How the REPL Uses the Core Components

- **ObjectFactory**: The REPL creates an `ObjectFactory` that wraps the persistence layer. When you run `create`, it calls `factory.create(type_name, base_name, owner)`. The factory:
  - Asks the persistence layer for the next counter value for that type + base name.
  - Generates a unique `ObjectId` in the `type:base-name-counter` format (using 3-digit hex).
  - Increments the counter in the database.
  - Builds a fresh `Object` with default values.
  - Saves the new object immediately.

- **Persistence Layer**: The REPL uses `SqlitePersistence` (an implementation of the `Persistence` trait). This handles:
  - Saving and loading full `Object` structs as JSON in SQLite.
  - Tracking per-type/base-name counters so IDs remain unique even after restarts.
  - All `save` and `load` commands go through this layer.

- **In-memory cache**: The REPL maintains a simple `HashMap` of recently used objects so you don't have to reload everything manually. Commands like `add_prop` and `add_verb` work on the cached copy and then persist the changes.

This design keeps the REPL thin while demonstrating how the real system will use the Object Model and persistence abstraction.

## Tips for Experimentation

- You can create different types: `create item silver-sword`, `create npc old-mage`, etc.
- Use full IDs when referring to objects (copy them from `create` or `list` output).
- Changes are saved automatically in most cases, but you can explicitly use `save` if needed.
- The database file `repl.db` can be deleted to start fresh.

If you run into issues or want to extend the REPL, feel free to look at `src/bin/repl.rs` and the files under `src/core/`.
