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
You are holding a Rusty Sword and Wooden Sword and wearing a backpack.
```

Does **not** show: weight, capacity, hand slots, nested container contents, properties, or IDs.

Uses `DisplayFlags::BRIEF` internally.

### `examine` / `x [target]`

**Purpose:** Closer in-game inspection with physical stats.

Includes everything `look` shows, plus weight and capacity for items and containers.

**`examine self`** — concise equipment summary (MOO-style, no property dump):

```text
Admin (human)
Equipped: Rusty Sword (right hand), Wooden Sword (left hand), backpack (back)
Carrying: 12/100 weight.
```

**`examine self body`** or **`examine self.body`** — detailed anatomy only:

```text
You are human. Available slots: left hand, right hand, head, back, left arm, right arm, ...
```

**`examine human`** (creature name, no matching object) — same slot list with `Human anatomy.` heading.

**Parent / prototype inspection** (inherited defaults from the prototype object):

| Form | Example |
|------|---------|
| `examine <object>.parent` | `examine coins.parent` |
| `examine #parent` | Parent of self (creature body plan for players without a prototype object) |
| `examine <object> parent` | Same as dot form |

Shows inherited properties from the prototype (`weight`, `description`, role flags, etc.). Players without a `prototype` object fall back to their creature body plan (`examine #parent` → human anatomy).

Does **not** show: object IDs, raw properties, verb source, or JSON.

### `@examine [target]`

**Purpose:** Wizard/builder structured view for authoring and debugging.

Requires wizard permission (stubbed `true` in REPL).

Shows: short ID, owner, location, weight breakdown (`Weight: 20 (2 × 10)`, `Contents weight: 7/10`), properties, verbs, container contents summary.

**Parent / prototype:** `@examine coins parent` or `@examine coins.parent` — categorized view of the prototype object plus an `inherited:` section (local overrides marked).

**Body plans:** `@examine human` — slot definitions from `creatures.mudl`. `@examine self` (or any player) adds an `anatomy:` section listing each slot, type, capacity, and current occupant.

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