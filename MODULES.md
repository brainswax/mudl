# Adventure Modules

Official expansion packs for Project MUDL. Each module is a **self-contained folder** with `<name>.mudl` and `README.md` beside it.

## Folder layout

```
expansions/<folder>/
  <folder>.mudl    # @expansion metadata, areas, items, NPCs, spawners
  README.md        # Self-contained player/builder guide (required)
```

**Naming:** folder name and `.mudl` filename must match (`glimmerfen/glimmerfen.mudl`).

**Self-contained rules:**

- README must stand alone — no links to other packs, no “install X first”, no references to repo docs (`LANGUAGE.md`, `BUILDER.md`, etc.).
- All install steps fit in Quick Install (REPL/IRC only — no `cargo`, no editing server files).
- Teasers and puzzle notes must not spoil solutions or exact routes.

## `@expansion` block (in `.mudl`)

Every pack declares metadata at the top:

```mudl
@expansion <id>
  name=<Display Name>
  version=1
  integrates=<comma-separated host base_names for optional auto-placement>
@end
```

- **`<id>`** — stable pack identifier (document in README).
- **`integrates`** — host `base_name`s where the pack may place portal items or signs at load time. Builders using Quick Install do not need these rooms; document them in Detailed description for authors who want auto-hooks.

## Import URL

GitHub (canonical):

```mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/<folder>/<folder>.mudl
```

## README structure (required)

Use this exact section order and headings.

### 1. Title + theme teaser

`# <Display Name>` then one short paragraph: mood, genre, danger level hint. **No spoilers**, no puzzle answers.

### 2. Quick Install

Heading: `## Quick Install`

Exactly **three builder commands** plus entry — pasteable in IRC or the REPL from **any current room**:

```mudl
@import <full GitHub raw URL>
@create portal <Name> [prototype=<pack-prototype>] door_direction=<dir> door_destination=<entry-base>
@link <dir> <entry-base> --return <return-dir>
```

Then one line: `Then go <dir>.`

| Command | Purpose |
|---------|---------|
| `@import` | Loads the pack (areas, prototypes, NPCs, spawners) |
| `@create portal` | Places a portal object in the builder's current room |
| `@link` | Wires the exit; `--return` sets the reciprocal direction on the entry room |
| `go <dir>` | Enter the module (player command, not counted in the three) |

**Quick Install checklist:**

- [ ] Full `raw.githubusercontent.com` URL (not a short link)
- [ ] `door_destination` = entry area `base_name` from the `.mudl`
- [ ] `@link` direction and `--return` match the portal's `door_direction` and entry room's reciprocal exit
- [ ] Only use commands available via IRC/REPL (@import, @create, @link, @dig, go, etc.)
- [ ] Must not refer to local runtime environment commands (cargo, make, file edits, etc.)

### 3. Detailed description

Heading: `## Detailed description`

Cover everything a builder or curious player needs **without solving puzzles**. Suggested subsections (use bold labels, not extra `##` headings):

| Subsection | Include |
|------------|---------|
| **Module** | `@expansion` id, display name, entry `base_name`, install portal direction / return |
| **Areas** | Main route `base_name`s and how they connect; wrong-turn rooms with `loop_to`; finale room and `scatter_to` / `scatter_direction` |
| **Tone** | Danger level, combat, environmental damage |
| **Features** | NPCs, `@effect` names, harvestables, breakables, containers, schedules |
| **Hidden** | `hidden_until_discovered` objects — room or region hint only, not discovery method |
| **Puzzles** | What signage/markers teach (vocabulary themes); explicitly state that order and solutions are in-game |
| **Commands** | Player verbs worth knowing |

**Area table example** (adapt per pack):

```markdown
**Areas**

| base_name | Role |
|-----------|------|
| `fey-threshold` | Entry; exits north/east/south/west to route and wrong turns |
| `fey-dewglade` | Main route — dew theme |
| `fey-grace` | Finale; `out` scatters to host grounds |
| `fey-mist` | Wrong turn → `loop_to: fey-threshold` |
```

Do **not** list marker sequence, key order, or safe path as a solved walkthrough.

### 4. Extension ideas

Heading: `## Extension ideas`

Optional bullet list for builders: `@schedule`, `@spawn-template`, extra portals, `@trigger`, new harvest nodes, etc.

---

## README template (copy for new packs)

```markdown
# <Display Name>

<One paragraph theme teaser — no spoilers.>

## Quick Install

Stand in any room and paste:

\`\`\`mudl
@import https://raw.githubusercontent.com/brainswax/mudl/main/modules/default/worlds/default_world/expansions/<folder>/<folder>.mudl
@create portal <Name> door_direction=<dir> door_destination=<entry-base>
@link <dir> <entry-base> --return <return-dir>
\`\`\`

Then `go <dir>`.

## Detailed description

**Module:** `@expansion <id>` · entry `<entry-base>` · portal `<dir>` / return `<return-dir>`

**Areas**

| base_name | Role |
|-----------|------|
| `<entry-base>` | Entry |
| ... | ... |

**Tone:** ...

**Features:** ...

**Hidden:** ...

**Puzzles:** Signage and markers use a <theme> vocabulary; sequence is learned in play.

**Commands:** `look`, `examine`, `read`, `go`, ...

## Extension ideas

- ...
```

---

## Official packs

| Display name | Folder | `@expansion` id | Entry `base_name` |
|--------------|--------|-----------------|-------------------|
| Haunted Forest | `haunted_forest/` | `haunted_forest` | `haunted-entry` |
| Poisonous Swamp | `poisonous_swamp/` | `poisonous_swamp` | `swamp-entry` |
| Giant Spider Den | `giant_spider_den/` | `giant_spider_den` | `spider-entry` |
| Sandy Shoals Resort | `sandy_shoals/` | `beach_resort` | `beach-trail` |
| Glimmerfen | `glimmerfen/` | `fey_glade` | `fey-threshold` |

Each pack's full README lives at `modules/default/worlds/default_world/expansions/<folder>/README.md`.
