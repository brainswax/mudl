# Builder & Wizard Tools

Design for live object modification in Project MUDL. All builder/wizard mutation commands use the `@` prefix and route through a single `set` / `unset` interface.

## Three Data Layers

Every object exposes three conceptual layers in `@examine`:

| Layer | Mutable? | Examples | Storage |
|-------|----------|----------|---------|
| **Properties** | Yes (`@set`) | `weight`, `capacity`, `max_weight`, `is_container`, `description`, `exits` | `Object.properties` (config keys) |
| **State** | Yes (`@set` / `@unset`) | `owner`, `location`, `prototype`, `contents`, `body_slots`, `stack_count`, `carried_slot` | Top-level fields + designated state properties |
| **Status** | **No** (read-only) | `contents_weight`, `carried_weight`, `total_weight`, `weight` (stack total) | Computed at examine time |

**Verbs** are separate from properties: stored in `Object.verbs`, set via `@set <target> verb.<name> <code>`.

**Identity** (`id`, `type`) is immutable at runtime. `@examine` shows them in the header only.

### Property vs State Classification

**Object fields** (state, top-level struct):
- `name` — display name
- `owner` — object ID or resolvable name (`you`, `Admin`, `player:hero-001`)
- `location` — object ID, resolvable name, or `none` to clear
- `prototype` — object ID, resolvable short ID, or `none`
- `alias` — adds a single alias string (repeat `@set` to add more)

**State properties** (runtime, in `properties` map):
- `contents` — list of object refs `[coins, purse]`
- `body_slots` — map `{right_hand: sword}`
- `stack_count` — integer
- `carried_slot` — string

**Config properties** (everything else in `properties`):
- Physical: `weight`, `volume`, `capacity`, `max_weight`, `max_volume`
- Roles: `is_container`, `is_wearable`, `is_pocketable`, `stackable`
- Slots: `wear_slot`, `hand_slot`, `max_stack`
- Creature: `creature`, `gender`
- Content: `description`, `exits`

## Commands

All wizard commands require the `@` prefix. Non-`@` forms are player commands (`look`, `take`, …).

### `@set <target> <key> <value...>`

Create or update a property, state field, object field, or verb.

```
@set backpack weight 10
@set backpack capacity 20
@set backpack max_weight 100
@set backpack location Admin
@set backpack owner you
@set purse contents [coins]
@set hero body_slots {}
@set sword verb.sharpen say('You sharpen the blade.')
@set sword description "A rusty old blade."
@set room:void-001 exits {north: garden}
```

**Value parsing** (automatic type inference):
- `true` / `false` → boolean
- Integer literals → `Int`
- Quoted strings → `String` (quotes stripped)
- `[a, b]` → list (elements parsed recursively; object names resolved)
- `{key: value, ...}` → map
- Bare tokens → string, or object reference if resolvable

**Verb keys**: `verb.<name>` or `verb:<name>` (e.g. `verb.wave`).

### `@unset <target> <key>`

Remove a config property, state property, verb, or clear an object field.

```
@unset backpack weight
@unset sword verb.wave
@unset purse carried_slot
@unset item:coins-001 location
@unset hero prototype
```

**Cannot unset**: `id`, `type`, status fields, or required identity (`name`).

Clearing optional object fields:
- `@unset <target> location` → `location = None`
- `@unset <target> prototype` → `prototype = None`

### Existing wizard commands (unchanged)

| Command | Purpose |
|---------|---------|
| `@create <type> <name...> [key=value...]` | Create object with role options |
| `@examine [target]` | Categorized builder view |
| `@dump [target]` | Full JSON debug dump |
| `@delete <target>` | Soft-delete |
| `@undelete <id>` | Restore soft-deleted object |
| `load <id>` / `save <id>` | Session cache ↔ persistence |

### Removed commands

| Old | Replacement |
|-----|-------------|
| `add_prop <id> <name> <value>` | `@set <target> <name> <value>` |
| `add_verb <id> <name> <code>` | `@set <target> verb.<name> <code>` |

## `@examine` Output Format

```
name: backpack
type: container
id: backpack-001
properties:
  weight: 10
  capacity: 20
  max_weight: 100
  is_container: true
state:
  owner: you
  location: Admin
  contents: []
status:
  contents_weight: 0/100
  weight: 10
verbs: (none)
```

Rooms use `state.present` instead of `contents`. Players show `state.body_slots`, `anatomy:` (slot definitions from the creature body plan with occupancy), and `status.carried_weight`.

### Parent / prototype inspection

| Command | Shows |
|---------|--------|
| `examine <object>.parent` | Player-facing inherited properties from prototype |
| `examine #parent` | Parent of self; players without a prototype object show their creature body plan |
| `@examine <object> parent` | Builder view: `prototype of:`, `inherited:`, prototype `properties` / `state` / `verbs` |

Inherited keys match those copied at creation (`weight`, `volume`, role flags, `description`, etc.). Local overrides on the instance are marked `(overridden locally)`.

### Body plan inspection

Creature definitions live in MUDL (`@creature human` in `creatures.mudl`), loaded into `AnatomyRegistry`. Players store the plan name in the `creature` property (set from `@player-template` at spawn).

Player `examine self` stays concise (creature type, gear in prose, slot use, carry weight). Full slot lists are opt-in:

| Command | Shows |
|---------|--------|
| `examine self` | `You're a human carrying …` + `carry capacity of N/M` + `are carrying W/max weight.` |
| `examine self body` / `examine self.body` | `You are human. Available slots: …` |
| `examine human` | `Human anatomy. Available slots: …` (when no object named human) |
| `@examine human` / `@examine self body` | `type: body_plan` with per-slot `type`, `capacity`, `hands` |
| `@examine <player>` | Adds `anatomy:` section with slot occupancy |

## Permission Model

| Role | Capabilities |
|------|--------------|
| **Player** | `look`, `examine`, `take`, `drop`, movement, inventory |
| **Builder** | Player commands + `load`, `save`, `@examine`, `@dump` (read-only inspection) |
| **Wizard** | Builder + `@set`, `@unset`, `@create`, `@delete`, `@undelete`, `module reload` |

Current REPL stub: `has_wizard_permission()` returns `true` for the session player. Production will check `PermissionFlags::WIZARD` / `BUILDER` on the actor object.

`@set` / `@unset` require **wizard** level. `@examine` / `@dump` require **builder** or wizard.

## Safety

### Validation
- Keys normalized to lowercase.
- Status fields rejected on `@set` with a clear error.
- Numeric properties (`weight`, `capacity`, …) must parse as integers.
- Boolean role flags must be `true`/`false`.
- `contents` / `body_slots` values validated for resolvable object references.
- `id` and `type` are never writable.

### Auditing (planned)
- Log every `@set` / `@unset` with actor, target, key, old value, new value, timestamp.
- REPL: `tracing::info!` on successful mutations today.

### Undo (planned)
- Per-session undo stack (last N mutations).
- `@undo` replays inverse operation.
- Not implemented in MVP; design reserves the hook in the command layer.

## Implementation Notes

- **Parser**: `command::editor` — `parse_set_command`, `parse_unset_command`
- **Mutation**: `object::editor` — `set_field`, `unset_field`, `parse_value_literal`
- **Classification**: `object::fields` — shared key → layer mapping for examine + editor
- **Display**: `display::examine` — properties / state / status sections