# Language Specification (MUDL)

**MUDL** — The domain-specific language for building programmable, self-modifying MUD/MOO worlds in this project.

**Status**: Draft / MVP skeleton. This is a living document that will evolve with the interpreter.

## Goals
- Accessible to non-programmers (builders) while powerful enough for complex behaviors.
- First-class support for MUD concepts: rooms, objects, verbs (actions), events, persistence.
- Safe for live execution (IRC, LLM-generated code).
- Extensible and self-modifying — the world can add new behaviors at runtime.
- Easy to serialize (files, GitHub) and generate (LLM).

## Core Concepts

### 1. Everything is an Object
- Rooms, items, players, NPCs, exits, even abstract systems.
- Objects have:
  - Unique ID
  - Name + aliases
  - Location (or null)
  - Prototype / parent (inheritance)
  - Owner + permissions
  - Properties
  - Verbs / behaviors
  - Event handlers

### 2. Properties
Key-value data attached to objects.

**Syntax sketch**:
```mudl
property "description" on room {
    value: "A cozy kitchen..."
    type: string
}

property "bag_of_holding" on container {
    capacity: infinite
    weightless: true
   
    on_add_item(item) {
        // behavior code
    }
}
```
### 3. Verbs / BehaviorsExecutable actions attached to objects.

```mudl
verb "bake bread" on kitchen {
    requires: ["flour", "yeast"]
    
    execute(player) {
        if (player.has("flour")) {
            say("You bake a fresh loaf!")
            create item "fresh bread" in this
        } else {
            say("Missing ingredients...")
        }
    }
}
```
### 4. Events & Hooks

Named triggers that run scripted lines when something happens in the world. Builders attach them with `@trigger` on **places** (`map.mudl` / expansion areas) and **objects** (`@prototype` / `@item` blocks).

**Built-in events** (Milestone 4):

| Event | Fires when |
|-------|------------|
| `on_enter` | A player enters the place |
| `on_leave` | A player leaves the place |
| `on_take` | A player picks up the object |
| `on_drop` | A player drops the object |
| `on_move` | The object changes location (take, drop, put, etc.) |
| `on_break` | A breakable object is smashed |
| `on_death` | A creature with handlers dies (NPCs/objects) |
| `on_kill` | Creature death — `@trigger` scripts and attached loot spawners fire via `execute_event` |
| `on_discovered` | Hidden creatures and objects revealed on `look` |
| `on_harvest` | Harvestable objects (`harvest <object>`) — resource spawners subscribe |
| `on_unlock` / `on_open` | Doors and containers (same as gate handlers) |

**Script actions**:

| Action | Example | Effect |
|--------|---------|--------|
| `narrate` / `say` / `message` | `narrate The air chills.` | Player-facing text |
| `emote` | `emote shudders.` | Item: `The <item> …`; place: atmospheric |
| `react` | `react flee` / `react attack` | Creature reactions (flee, attack, greet, warn) |
| `damage` / `heal` | `heal 5` / `damage host 12` | Adjust health on `actor` (default), `host`, or `target` |
| `mod-stat` / `mod-skill` | `mod-stat actor strength 2` | Permanent stat/skill bump |
| `set-property` | `set-property host player_discovered true` | Set a bool/int/string property |
| `grant-effect` | `grant-effect actor regeneration` | Apply a defined `@effect` to a creature |
| `teleport` | `teleport haunted-entry` | Move `actor`/`host`/`target` to a place `base_name` |
| `spawn` | `spawn mist-wisp` / `spawn item trail-rations` | Spawn creature template or item prototype in room |
| `when` / `if` | `when health below 30 then heal 15` | Conditional — runs nested action when true |
| `stop` | `stop` | Halt remaining handlers for this event |

**Conditionals** (`when … then …`):

| Condition | Example |
|-----------|---------|
| Health | `when health below 30 then heal 15` |
| Stat / skill | `when skill survival at_least 2 then narrate …` |
| Property | `when property player_discovered then narrate …` |
| Chance | `when chance 40 then spawn mist-wisp` (deterministic roll) |

Optional subject prefix on conditions and targeted actions: `actor`, `host`, `target`.

`on_kill` fires on the **victim** (killer as actor) and on the **killer** when the killer has handlers (victim as actor). `on_discovered` runs after perception reveals a hidden creature or object — via `@trigger` on the host (template `on_discovered=` lines are converted automatically at bootstrap). `on_harvest` fires when a player harvests a `harvestable=true` object; attached `@resource-spawner` blocks may drop renewable materials into the room.

```mudl
# Place trigger (legacy map block)
type: area
base_name: haunted-moon
name: Moonlit Glade
@trigger on_enter narrate Silver mist clings to the branches.
exits:
  south: haunted-entry

# Object trigger (@prototype / @item)
@prototype haunted-clay-pot
  breakable=true
  @trigger on_break emote shatters into pale dust.
@end

# Creature triggers (@npc / @spawn-template)
@npc path-watcher
  @trigger on_kill narrate The forest exhales as the watcher falls.
@end

@spawn-template pale-lurker
  @trigger on_discovered react attack
  @trigger on_discovered emote lunges from the shadows.
@end
```

Use **`@trigger`** for all creature scripts (say, emote, narrate, react). `@behavior-template` / `@use-behavior` supply AI tactics (`react`, `attack_damage`, `awareness_check`); inline `@behavior … react …` still works for react-only overrides. Legacy `@behavior on_enter say …` is migrated to triggers at bootstrap but prefer `@trigger` in new content.

**Runtime builder command** (REPL / wizard):

```
@trigger help
@trigger list [target]                    # default target: here (current room)
@trigger <target> <event> <script>        # add trigger
@trigger add <target> <event> <script>
@trigger remove <target> <event> [n]    # remove #n (default: last)
@trigger clear <target> [event]
@trigger set <target> <event> <n> <script>
@trigger test <target> <event>            # dry-run narrative preview
```

Targets: object/creature/place name, `here` / `.` (current room), `me` / `self` (player). Scripts are validated at attach time (unknown verbs and malformed `when … then …` are rejected). Changes persist to the live object immediately.
### 5. Built-in Primitives
* say(msg), tell(player, msg)
* move(thing, destination)
* create object ...
* get_property(obj, name), set_property(...)
* add_verb(obj, name, code)
* Reflection: list_properties(obj), list_verbs(obj)

## Fundamental vs Extensible
Fundamental (implemented in core engine):
* Object model
* Basic property/verb/event system
* Core types (string, int, bool, list, object ref)
* Sandboxed execution environment
* Persistence hooks

Extensible (defined in MUDL):
* New property behaviors
* Custom events
* Timers / scheduled actions
* LLM-generated content
* Self-modifying logic

## Syntax Philosophy
* Readable, English-like where possible.
* Support both declarative (for builders) and imperative (for complex logic) styles.
* Multi-line friendly for files/GitHub.
* Compact prefixes for live IRC input (e.g. !verb ...)

## Example Full Object

```mudl
room "Cozy Kitchen" {
    description: "Warm and inviting, with the smell of fresh bread."
    
    exit north to "Living Room"
    
    object "oven" {
        verb "bake" {
            // implementation
        }
    }
    
    on_enter {
        // welcome message
    }
}
```
## Safety & Sandboxing
* All user/LLM code runs in a restricted environment.
* No direct file/system access.
* Permission checks enforced by the Gateway.
* Resource limits (CPU, loops, memory).

## Universes, Worlds, and Entrypoints

Game content lives under `modules/<universe>/`. A universe holds one or more worlds:

```mudl
# universe.mudl
@universe default
  default_world=default_world
@end
@include-world default_world
```

Each world uses a flat set of `.mudl` files under `worlds/<name>/`, composed from `world.mudl`:

```mudl
# worlds/default_world/world.mudl
@world default_world
  starting_location=the-void
@end
@include map.mudl
@import expansions/haunted_forest.mudl
@include creatures.mudl
@include players.mudl
@include items.mudl
@include objects.mudl
```

- `@include` paths are relative to the **world** directory (built-in content shipped with the world).
- `@import` loads expansion packs from a **local path** or **URL** (fetched at load time). Resolution order for relative paths: directory of the importing file → world root → universe root. Supports `http://`, `https://`, and `file://` URLs.
- `@include-world <name>` loads `worlds/<name>/world.mudl` from the universe root.

### Expansion packs

Self-contained `.mudl` files bundle areas, items, and hooks for drop-in world extension. Tag them with `@expansion` metadata:

```mudl
@expansion haunted_forest
  name=Haunted Forest
  version=1
  integrates=forest-path,the-void,scene-chest
@end
```

Import from `world.mudl`:

```mudl
@import expansions/haunted_forest.mudl
@import https://example.com/mudl/expansions/haunted_forest.mudl
```

The host world keeps minimal map hooks (e.g. `forest-path` with `in: haunted-entry`); the expansion places items and defines puzzle areas.

Places may set `loop_to: <base_name>` so entering that room silently returns the player to another place (no movement message). Useful for maze wrong turns.
- Set `MUDL_MODULE=modules/default` (or `MUDL_UNIVERSE` to a specific file) to load a universe.
- Set `MUDL_WORLD=<name>` to select which world to bootstrap (defaults to the universe's `default_world`).

Fork `modules/default/` to add custom worlds — e.g. a feline campaign with `creature=cat` in `players.mudl`.

## Creatures and Anatomy

Creature anatomy is defined in `creatures.mudl` via `@creature` blocks. Player templates in `players.mudl` reference a creature:

```mudl
@creature human
  @slot left_hand capacity=1 type=grasp hands=1
  @slot right_hand capacity=1 type=grasp hands=1
  @slot head capacity=1 type=wear
  @slot torso capacity=1 type=wear
@end
```

```mudl
@player-template default
  creature=human
  gender=neutral
@end
```

**Creature vitals and stats** (Milestone 3):

```mudl
@creature human
  max_health=100
  base_max_weight=90
  @stat strength 10
  @stat dexterity 10
  @stat constitution 10
  @stat intelligence 10
  @skill combat 0
  @skill stealth 0
  @skill crafting 0
  @skill survival 0
  @slot left_hand capacity=1 type=grasp hands=1
@end

@effect weary
  mod_encumbrance=1.1
  mod_max_weight=-5
  mod_stat_dexterity=-2
  mod_skill_stealth=-1
@end
```

- **`@stat` / `@skill`** — free-form names; core stats are `strength`, `dexterity`, `constitution`, `intelligence`, `wisdom`, `charisma`. Core skills include `combat`, `stealth`, `crafting`, `survival`. Builders may define custom stats/skills on any `@creature`.
- **`max_health`** — template value scaled by constitution: each point above 10 adds 5 max health (constitution 12 → +10 health on a 100 template).
- **`base_max_weight`** plus effective `strength` sets carry capacity (equipment and effects stack on top).
- **`@effect`** defines reusable conditions; creatures track `active_effects` at runtime. `mod_stat_*` and `mod_skill_*` apply while active.
- **`@slot` effect=`** — optional slot-tagged body-plan conditions (future wound hooks).

**`examine self`** shows effective stats and skills (gear and active effects included), e.g. `You are Strength 12 (+2), Constitution 10. Your skills are Combat 1, Survival 1 (+1).`

**Skill progression** — combat awards experience on each hit; every 5 XP advances one skill rank (narrative line when a rank increases).

**Equipment modifiers** (wearable and wielded gear stack):

```mudl
@prototype chipped-blade
  hand_slot=right
  @mod-stat strength 2
@end

@prototype leather-vest
  is_wearable=true
  wear_slot=torso
  mod_max_health=5
  @mod-stat constitution 2
  @mod-skill survival 1
@end

@prototype boots-of-carrying
  is_wearable=true
  wear_slot=left_foot
  mod_max_weight=25
  mod_encumbrance=0.85
@end

@prototype iron-lantern
  hand_slot=right
  @grant-effect iron_lantern_aura
@end

@effect iron_lantern_aura
  mod_encumbrance=0.95
@end

@effect regeneration
  regen_on_enter=2
@end
```

- `@mod-stat` / `@mod-skill` — additive bonuses while equipped (worn or wielded in grasp slots).
- `mod_max_weight`, `mod_encumbrance`, `mod_max_health` — carry capacity, encumbrance feel, and health ceiling.
- `@grant-effect` — apply a defined `@effect` while the item is equipped (regeneration, auras, etc.).
- Modifiers from multiple worn items **stack**; granted effects compose with direct item bonuses.

**Behavior templates** (reusable, composable personalities):

```mudl
@behavior-template guard
  react=warn
  on_enter=say Halt! Who goes there?
@end

@behavior-template aggressive
  react=attack
  on_enter=say You should not have come here.
  attack_damage=12
@end

@behavior-template skittish
  react=flee
  on_enter=emote scrambles away from you.
@end

@behavior-template wanderer
  react=wander
  wander_interval=3
  on_enter=emote paces the area restlessly.
@end
```

- `react` — how the creature responds when a player enters: `ignore`/`passive`, `warn`/`guard`, `attack`/`aggressive`, `flee`/`skittish`, `wander`/`roam`.
- `on_enter` / `on_discovered` — optional scripted lines (`say`, `emote`) converted to `@trigger` handlers at bootstrap, fired alongside the react.
- `attack_damage` — damage dealt on `attack` react (default 8).
- `wander_interval` — emote every N player entries for `wander` react (default 3).

Creatures support **multiple simultaneous behaviors** — combine `@use-behavior` templates with `@trigger` scripts for unique personalities.

**NPCs and behaviors**:

```mudl
@npc path-watcher
  name=Path Watcher
  creature=human
  location=forest-path
  @use-behavior guard
  @trigger on_enter say The trees seem to lean closer when you pass.
@end
```

Supported trigger actions: `say`, `emote`, `narrate`, `react`. `on_enter` runs when a player enters the NPC's room.

Builders can attach templates at runtime: `@addbehavior <creature> <template>`, `@listbehaviors <creature>`.

**Combat and death** (Milestone 3):

```mudl
attack path watcher
```

Player command: `attack <creature>` — turn-based melee in the current room.

- **Damage** — derived from effective `strength` and `combat` skill (stats + equipment + effects), mitigated by target `constitution` and `dexterity`, with light per-exchange variance. Wielded gear with stat mods is named in the attack line. Successful hits award combat skill XP.
- **Critical hits** — surprise attacks (unaware target) always land as critical blows with bonus damage. Skilled fighters (`combat` 4+) can occasionally score a critical on aware targets.
- **Awareness** — templates with `awareness_check=true` (default for `react=attack`) run bilateral contests on room entry: player `stealth` vs creature `perception`, and player `survival`/wisdom vs creature ambush stealth. Unaware mobs skip attack/warn reactions; you may see `The pale lurker hasn't noticed you.` or `You spot the pale lurker before it sees you.`
- **Hidden lurkers** — creatures with `awareness_check=true` stay hidden from `look` until you discover them. `look` and `examine` run a perception check (`survival`, wisdom, dexterity vs ambush stealth). Success: `You notice a pale lurker here.` and any `on_discovered` behaviors fire.
- **Hidden objects** — items with `hidden_until_discovered=true` stay out of room listings until discovered. Optional `discovery_stealth=N` (default 8) sets the perception threshold. `@trigger on_discovered` on the object fires when revealed.
- **on_discovered** — builder hook when a hidden creature or object is revealed: `@trigger on_discovered emote ...`, `@trigger on_discovered react flee`, or template `on_discovered=emote ...` / `on_discovered_react=attack`. Supports `attack`, `flee`, `warn`, `greet`, and scripted lines.
- **Harvest** — `harvest <object>` on `harvestable=true` nodes fires `on_harvest`; `@resource-spawner` blocks with `trigger=on_harvest` drop weighted `@resource-template` items into the room (crafting pipeline).
- **Ambush** — if a lurking creature spots you first but you don't spot it, you may see `A pale lurker ambushes you!` and take surprise damage on its on-enter attack.
- **Surprise** — attacking an unaware creature adds bonus damage and grants the first strike (`You catch the pale lurker off guard and strike`). If you are unaware, the creature strikes first with bonus damage.
- **Initiative** — each exchange compares `dexterity`, optional `speed`, and `combat` skill; the faster combatant acts first (`The path watcher is quicker and strikes` when they win initiative).
- **Counter-attack** — aware NPCs with `react=attack` strike back after your blow (or first, if they win initiative), using `attack_damage` from their behavior template when set.
- **NPC death** — creature is removed; a **corpse** container (`is_corpse`) appears in the room holding all worn/wielded gear. `on_kill` loot spawners attached to the NPC fire.
- **Player death** — your corpse and gear remain where you fell; you respawn **naked** at `home_location` (set from `starting_location` at bootstrap) with full health.

Example kill loot on a fixed NPC:

```mudl
@loot-template watcher-rations
  prototype=trail-rations
  count=1
@end

@loot-spawner path-watcher-kill
  target=path-watcher
  trigger=on_kill
  chance=1.0
  max_active=2
  @entry watcher-rations weight=1
@end
```

Wizard vitals (testing): `@damage <creature> [amount]`, `@heal <creature> [amount]`.

**Creature spawners** (locations only spawn randomly when a spawner is attached):

```mudl
@spawn-template mist-wisp
  name=Mist Wisp
  creature=human
  @use-behavior wanderer
  @trigger on_enter emote drifts through the air.
@end

@spawner haunted-moon-phantoms
  location=haunted-moon
  trigger=on_enter
  chance=0.7
  max_active=1
  @entry mist-wisp weight=3
  @entry pale-lurker weight=1
@end
```

**Timed schedules** (`@schedule` — periodic host events on room-enter ticks):

```mudl
@schedule haunted-mist-weather
  target=haunted-mist
  interval=2
  event=on_weather
@end

type: area
base_name: haunted-mist
@trigger on_weather narrate The mist thickens, swallowing familiar landmarks.
```

- `interval` / `every` — fire every Nth `on_enter` tick for that room (shared scheduler).
- `event` — custom event name; matching `@trigger` scripts on the target run (no subscriber re-entry).
- `stop` / `cancel` in a trigger script halts remaining handlers for that dispatch.

**Resource spawners** (renewable harvest nodes for crafting materials):

```mudl
@resource-template forest-moss
  prototype=trail-rations
  count=1
@end

@resource-spawner moss-harvest
  target=haunted-moss-patch
  trigger=on_harvest
  chance=1.0
  max_active=2
  @entry forest-moss weight=1
@end
```

- `trigger=on_harvest` — fires when a player runs `harvest <object>` on a `harvestable=true` target.
- `trigger=on_enter` / `trigger=timer` — room-attached renewal (timer uses the central room enter tick).
- `@resource-template` references an item prototype; spawned items appear in the room.

- `trigger=on_enter` — roll on each player entry; `trigger=periodic` with `periodic_interval=N` — every Nth room enter tick (shared scheduler).
- `chance` — spawn attempt probability (0.0–1.0). `max_active` — cap concurrent spawned creatures per spawner.
- No spawner on a location → no random spawns (only explicit `@npc` or MUDL-placed creatures).

**Slot types** (MVP):
- `grasp` — hands; items with `hand_slot: left`, `right`, or `both` occupy these
- `wear` — clothing/armor/containers worn on the body
- `limb` — biological parts (descriptive; not used for inventory yet)
- `pocket` / `container` — reserved for clothing-provided capacity (future)

**Player properties** (set by engine from template):
- `creature` — name of the loaded creature definition (e.g. `human`)
- `gender` — for descriptions (`neutral`, `male`, `female`, etc.)
- `body_slots` — map of slot name → held/worn item ID

`@body-plan` and `body_plan=` are accepted as aliases during migration. Default players are **naked humans**: biological slots only, no pockets or clothing until equipped.

## Map and Locations

Locations are defined in `map.mudl`. Default locations use `type=area`:

```mudl
type: area
base_name: the-void
name: The Void
description: You are in a featureless void.

exits:
  north: north-passage
```

Exits are **builder-defined** — any name works (`around`, `path`, `door`, `window`, `in`, `out`). There is no built-in compass vocabulary; shorthand like `n` only works when you declare it.

```mudl
exits:
  west: the-void
  around: cottage-front
exit_aliases:
  path: around
  n: north
exit_returns:
  west: east
  around: rear
```

- **`exits`** — canonical exit name → destination `base_name`.
- **`exit_aliases`** — alternate player input → canonical exit name (`path` moves via the `around` exit).
- **`exit_returns`** — when leaving via an exit, the reciprocal exit name on the destination (used by `@link --return` and world validation).

**Movement**: `go around`, `around` (standalone when unambiguous), or `go path` when `path` is an alias. `look` lists obvious exits as `around (path), west`.

## Player-Facing Output

MUDL separates **what the world knows** from **what players read**. The engine tracks stable object IDs, types, and JSON state internally; frontends render MOO-style narrative text.

### Three display tiers

| Tier | Audience | Commands | Shows |
|------|----------|----------|-------|
| **Player** | Everyone playing | `look`, `take`, `create`, `go`, `inventory`, … | Immersive prose only — names, descriptions, exits, natural inventory |
| **Builder** | World authors | `examine`, `add_prop`, `add_verb`, `load`, `save`, … | Contextual detail — owners, properties, verbs, exit *names* (not raw IDs) |
| **Debug** | Engine developers | `@dump`, logs (`RUST_LOG`) | Full JSON, IDs, persistence paths, bootstrap diagnostics |

**Rules:**

- Player commands never print raw IDs, type prefixes, or struct dumps.
- Builder feedback uses in-world phrasing where possible (`You weave … into being`, `You inscribe … upon …`) while remaining informative.
- Technical details go to `tracing` logs, not the REPL prompt or future IRC channel.
- Future MUDL verbs may override default messages per object or command via properties / event hooks.

### Example session (player tier)

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
Admin
You are completely naked.
You are holding Rusty Sword in your right hand.
> inventory
You are completely naked.
You are carrying:
  Rusty Sword — in your right hand
```

Object IDs still exist internally (`Rusty Sword` → `sword:rusty-sword-001`); use `@dump` or `RUST_LOG=info` when you need them.

## Items and Inventory (REPL)

Items are objects with `location` set to a place or player. The REPL supports basic pickup:

- `create <type> <name...>` — everything after the type is the display name (spaces allowed). Quoted names work: `create sword "Rusty Sword"`.
- Object IDs use lowercase hyphenated slugs derived from the name (`Rusty Sword` → `sword:rusty-sword-001`). Display names keep original capitalization.
- `create` places new objects at your current location when one is set.
- `take` / `get` moves items from the ground in your current location into grasp slots. Items you already carry are ignored when resolving the target, so `take sword` picks up a ground sword even if you're holding another.
- Items may set `hand_slot` to `left`, `right`, or `both` (two-handed).

## Persistence

Every `Object` is stored as JSON in SQLite. State changes from `take`, `drop`, `go`, and `create` are saved immediately. Objects are never hard-deleted — wizard `@delete` sets `is_deleted` and `@undelete <id>` restores them.

## Future Extensions
* LLM-friendly generation (clear grammar + examples in prompts).
* Meta-programming (objects modifying the language/runtime).
* Visual / procedural helpers.
* Import/export formats.
