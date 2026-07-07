# Player and Wizard Commands

Command reference for the MUDL REPL. Type `help` at the prompt for the canonical list.

Related docs: [BUILDER.md](BUILDER.md) (`@set` / `@unset`), [LANGUAGE.md](LANGUAGE.md) (MUDL syntax), [docs/REPL.md](docs/REPL.md) (REPL setup and examples).

## In-character vs out-of-character

| Layer | Commands | Voice | IRC notes |
|-------|----------|-------|-----------|
| **In-character** | `look`, `examine`, `take`, `attack`, `go`, … | Short natural English sentences | No leading object name; no player name on `look self` |
| **Out-of-character** | `@look`, `@examine`, `@dump`, `@set`, `@trigger`, … | Structured builder fields / JSON | Technical data for builders |

Player verbs have no `@` prefix. Wizard/builder meta-commands use `@` and require builder or wizard permission (stubbed `true` in the REPL today).

### In-character style guidelines

- **Voice:** Second person (`You …`) or immersive third (`The backpack contains …`).
- **No leading names:** Item look/examine never starts with the object name. `look self` never leads with the player name.
- **Room contents:** `You see an anvil and a boulder here.` — not `You see: anvil; boulder`.
- **Articles:** Use `a` / `an` when introducing items. Stack counts stay bare (`20 coins`).
- **Stats on examine only:** Weight, slot occupancy, and capacity appear on `examine`, not `look`.

## Inspection

### `look` / `l [target]`

Quick, immersive snapshot. Omitted target = current room.

| Target | Shows |
|--------|--------|
| *(none)* | Room description, exits, visible ground items |
| `<object>` | Description or container contents |
| `self` | One-sentence gear summary |

Does **not** show weight, properties, or IDs.

### `examine` / `x [target]`

Closer inspection with physical stats. `examine self` adds creature type, gear prose, slot use, and carry weight. `examine self body` lists anatomy slots. `examine <obj>.parent` shows inherited prototype properties.

### `@look` / `@examine [target]`

Builder structured view: properties, state, status, anatomy, prototype chain. See [BUILDER.md](BUILDER.md) for output format.

### `@dump [target]`

Full internal JSON. Debug only.

## Movement

| Command | Purpose |
|---------|---------|
| `go <dir>` | Move via an exit (`north`, `around`, aliases from `exit_aliases`) |
| `n` / `s` / `e` / `w` / … | Shorthand when declared in map `exit_aliases` |

Room entry runs `on_enter` triggers, spawners, creature behaviors, equipment regen, and condition ticks (see [ARCHITECTURE.md](ARCHITECTURE.md)).

## Inventory

| Command | Purpose |
|---------|---------|
| `inventory` / `i` | Full slot-by-slot listing |
| `get` / `take <item>` | Pick up from ground |
| `drop <item>` | Drop carried item |
| `put [count] <item> in <container>` | Stow items (partial stack transfer) |
| `remove <item> from <container>` | Take from container |
| `wield <item>` | Hold in grasp slots |
| `wear <item>` | Wear on body slot |

## World interaction

| Command | Purpose |
|---------|---------|
| `create <type> <name...>` | Create object at current location |
| `read <object>` | Read text on signs, notes, mailboxes |
| `open` / `close <target>` | Open or close containers, doors, windows |
| `lock` / `unlock <target> [with <key>]` | Lock or unlock (auto-finds matching key) |
| `break` / `smash <item>` | Smash breakable objects (`on_break` fires) |
| `harvest` / `gather <object>` | Harvest `harvestable=true` nodes (`on_harvest` + resource spawners) |

## Combat

| Command | Purpose |
|---------|---------|
| `attack <creature>` | Turn-based melee in the current room |

Combat uses effective stats, equipment, awareness, initiative, crits, counter-attacks, corpses, and `on_kill` triggers. Player death respawns naked at `home_location`. Wizard testing: `@damage <creature> [amount]`, `@heal <creature> [amount]`.

## Wizard meta-commands

### Object editing

| Command | Purpose |
|---------|---------|
| `@set <target> <key> <value...>` | Set property, state, verb, or object field |
| `@unset <target> <key>` | Remove property, verb, or clear field |
| `@create <type> <name...> [key=value...]` | Create with role options |
| `@delete <target>` | Soft-delete |
| `@undelete <id>` | Restore soft-deleted object |
| `load <id>` / `save <id>` | Session cache ↔ persistence |

Full `@set` / `@unset` reference: [BUILDER.md](BUILDER.md). Replaces legacy `add_prop` / `add_verb`.

### Places and triggers

| Command | Purpose |
|---------|---------|
| `@dig <dir> <name...>` | Create and link a new place |
| `@link <dir> <target> [--return <dir>]` | Wire an exit from here |
| `@unlink <dir>` | Remove an exit |
| `@trigger …` | Attach, list, test event scripts (`@trigger help`) |
| `@import <url-or-path>` | Load expansion pack MUDL at runtime |

### Creatures

| Command | Purpose |
|---------|---------|
| `@addbehavior <creature> <template>` | Attach behavior template |
| `@listbehaviors <creature>` | List attached templates |
| `@damage` / `@heal` | Apply damage or healing |
| `@keyfor <container> [name]` | Create a key for a lockable |

### Module

| Command | Purpose |
|---------|---------|
| `module reload` | Reload MUDL from disk |
| `module bundle <dir>` | Package module for distribution |
| `list` | Names in session working memory |

## Display layers

```
look          → Player + BRIEF   (short)
examine       → Player           (detailed stats)
@examine      → Builder          (IDs, properties, verbs)
@dump         → Debug / JSON     (full struct)
```

Implementation: `src/display/` (`DisplayContext`, `DisplayFlags::BRIEF`).