# Player and Wizard Commands

Command reference for the MUDL REPL (MVP). Player verbs use plain names; wizard/builder meta-commands use a leading `@` prefix.

## Inspection

### `look` / `l [target]`

**Purpose:** Quick, immersive snapshot of the world.

| Target | Shows |
|--------|--------|
| *(none)* | Current room: name, description, exits, ground items |
| `<object>` | Short name, description; containers also list direct contents (`Inside the purse: 20 coins`) |
| `self` | Player name/description plus brief gear summary |

**`look self` example:**
```
Admin
A weary adventurer.
You are holding: purse, 20 coins. Wearing: backpack.
```

Does **not** show: weight, capacity, hand slots, nested container contents, properties, or IDs.

Uses `DisplayFlags::BRIEF` internally.

### `examine` / `x [target]`

**Purpose:** Closer in-game inspection with physical stats.

Includes everything `look` shows, plus:

- Weight and capacity (`The purse weighs 2/10.`, `They weigh 20.` for stacks)
- On `examine self`: per-hand grasp detail, worn placement, total carried weight

Does **not** show: object IDs, raw properties, verb source, or JSON.

### `@examine [target]`

**Purpose:** Wizard/builder structured view for authoring and debugging.

Requires wizard permission (stubbed `true` in REPL).

Shows: short ID, owner, location, weight breakdown (`Weight: 20 (2 × 10)`, `Contents weight: 7/10`), properties, verbs, container contents summary.

### `@dump [target]`

**Purpose:** Full internal JSON representation of an object.

For deep debugging, diffing, and persistence inspection. Not player-facing.

## Inventory and movement

| Command | Purpose |
|---------|---------|
| `inventory` / `i` | Full slot-by-slot listing (hands, worn slots, nested container contents) |
| `get` / `take <item>` | Pick up from ground |
| `drop <item>` | Drop carried item |
| `put [count] <item> in <container>` | Stow items (partial transfer supported) |
| `remove <item> from <container>` | Take item out of a container |
| `wield <item>` | Hold/wield in grasp slots |
| `wear <item>` | Wear on body slot |
| `go <dir>` | Move between locations |

## Wizard meta-commands

| Command | Purpose |
|---------|---------|
| `@create <type> <name> [key=value...]` | Create with role options (`capacity=3`, `max_weight=10`, …) |
| `@delete <target>` | Soft-delete object |
| `@undelete <id>` | Restore soft-deleted object |

Meta-commands are parsed by `src/command/parse.rs` (`@` stripped, permission checked).

## Display layers

```
look          → Player + BRIEF   (short)
examine       → Player           (detailed stats)
@examine      → Builder          (IDs, properties, verbs)
@dump         → Debug / JSON     (full struct)
```

Implementation: `src/display/` (`Describable` on `Object`, `DisplayContext`, `DisplayFlags::BRIEF`).