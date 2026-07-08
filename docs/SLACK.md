# MUDL Slack Bot — Setup & Operator Guide

**Milestone 6** — Group-playtesting transport for MUDL. Multiple workspace members share one [`WorldState`](../src/world/world_state.rs) through [`SessionManager`](../src/gateway/session_manager.rs). Commands arrive over the Slack **Events API**; responses go out via the Web API ([`SlackWebTransport`](../src/slack/transport.rs)).

Command reference and output style: [COMMANDS.md](../COMMANDS.md) (Slack uses the same player verbs as IRC).

---

## Quick start

### 1. Local mock (no Slack account)

Rehearse the full group-play loop without credentials or HTTPS:

```bash
cargo build --bin slack
SLACK_MOCK=1 cargo run --bin slack
```

Type each line as `user_id channel_id command`:

```text
U_ALICE D_ALICE login player:hero-001
U_BOB D_BOB login player:hero-002
U_ALICE D_ALICE look
U_ALICE D_ALICE say hello
U_ALICE D_ALICE tell Bob psst
U_ALICE C_WORLD brb dinner
U_ALICE D_ALICE quit
```

Or run the automated smoke script:

```bash
./scripts/slack-mock-flow.sh
# equivalent: make test-slack-flow
```

### 2. Live workspace (group deployment)

Prerequisites: Rust toolchain, a Slack workspace where you can install apps, and a **public HTTPS** URL for Event Subscriptions.

```bash
cp .env.example .env
# fill SLACK_* values (see below)
cargo run --bin slack
```

Players DM the **MUDL** bot for game commands; OOC goes in your world channel; in-character speech posts to per-room channels or threads.

---

## Group deployment checklist

Use this when onboarding a playtest group:

| Step | Action |
|------|--------|
| 1 | Create the Slack app from [`slack-app-manifest.yaml`](../slack-app-manifest.yaml) |
| 2 | Install the app to the workspace → copy **Bot User OAuth Token** (`xoxb-…`) |
| 3 | Copy **Signing Secret** and **App ID** from **Basic Information** |
| 4 | Start MUDL behind HTTPS (tunnel or host) on port `3000` (default) |
| 5 | Enable **Event Subscriptions** → set Request URL → verify `url_verification` |
| 6 | Create `#mudl-ooc` (or similar) → invite the bot → copy **channel ID** (`C…`) |
| 7 | Set `.env` (`SLACK_BOT_TOKEN`, `SLACK_SIGNING_SECRET`, `SLACK_WORLD_CHANNEL`, …) |
| 8 | Deploy **login tokens** before opening to the whole workspace (SEC-01) |
| 9 | Run **one** writer only — do not run Slack + IRC + REPL on the same `DATABASE_URL` |
| 10 | Smoke-test: DM `login`, `look`, `say`, `tell`; post OOC in the world channel |

**Recommended for public playtests:**

```bash
MUDL_LOGIN_REQUIRE_AUTH=true
MUDL_LOGIN_TOKENS=player:hero-001=rotate-me,player:hero-002=other-secret
MUDL_LOGIN_IDENTITY_BINDINGS=U01234ABC=player:hero-001,U05678DEF=player:hero-002
```

Binding keys are **lowercase** Slack user ids. Issue tokens out-of-band; players send `login <token>` or `login player:hero-001 <token>` in a DM.

---

## Create the Slack app

### Option A — App manifest (recommended)

1. Open [Create New Slack App](https://api.slack.com/apps?new_app=1) → **From an app manifest**.
2. Pick your development or playtest workspace.
3. Paste the contents of [`slack-app-manifest.yaml`](../slack-app-manifest.yaml).
4. Review scopes and **Create**.

The manifest pre-configures:

- Bot scopes: `chat:write`, `channels:join`, `channels:read`, `im:history`, `im:write`, `users:read`, …
- Bot events: `message.im`, `message.channels`, `message.groups`, `app_mention`
- Placeholder Event Subscriptions URL (you will update this after the bot is reachable)

### Option B — Manual creation

If you cannot use a manifest, create a blank app and match the manifest:

**OAuth & Permissions → Bot Token Scopes:** same list as in `slack-app-manifest.yaml`.

**Event Subscriptions → Subscribe to bot events:** `message.im`, `message.channels`, `message.groups`, `app_mention`.

---

## Bot token and credentials

After creating the app:

### Install to workspace

1. **Settings → Install App** → **Install to Workspace** → Allow.
2. Copy **Bot User OAuth Token** → `SLACK_BOT_TOKEN` (`xoxb-…`).

### Signing secret

1. **Settings → Basic Information** → **App Credentials**.
2. Copy **Signing Secret** → `SLACK_SIGNING_SECRET`.

MUDL verifies every Events API POST with `X-Slack-Signature` / `X-Slack-Request-Timestamp` (5-minute replay window). Requests with a bad signature receive `401`.

### App ID (optional but recommended)

Copy **App ID** (`A…`) → `SLACK_APP_ID`. Strips `<@APP>` mentions when players @-mention the bot in DMs.

---

## Event Subscriptions

Slack delivers DMs and channel messages to your HTTP endpoint. **HTTPS is required** for live mode.

### Start the bot locally

```bash
SLACK_BIND_ADDR=0.0.0.0:3000
SLACK_EVENTS_PATH=/slack/events
cargo run --bin slack
```

Without `SLACK_BOT_TOKEN` + `SLACK_SIGNING_SECRET`, live mode refuses to start. Use `SLACK_MOCK=1` for offline testing.

### Expose HTTPS

Pick one tunnel (examples):

**ngrok:**

```bash
ngrok http 3000
# copy https://….ngrok-free.app
```

**cloudflared:**

```bash
cloudflared tunnel --url http://localhost:3000
```

### Configure in Slack

1. **Features → Event Subscriptions** → Enable Events.
2. **Request URL:** `https://YOUR_PUBLIC_HOST/slack/events`  
   (must match `SLACK_EVENTS_PATH`, default `/slack/events`).
3. Slack sends a `url_verification` challenge. MUDL responds with the `challenge` JSON field when the signing secret matches. Status should show **Verified**.
4. Under **Subscribe to bot events**, confirm: `message.im`, `message.channels`, `message.groups`, `app_mention`.
5. **Save Changes**.

### Reinstall after scope changes

If you add OAuth scopes later, **reinstall the app** to the workspace and update `SLACK_BOT_TOKEN` if Slack rotates it.

---

## Channels for group play

| Channel | Purpose | Required |
|---------|---------|----------|
| **DM to MUDL bot** | All game commands (`login`, `look`, `go`, `tell`, …) | Yes |
| **World channel** (`#mudl-ooc`) | Out-of-character chat — plain text, no prefix | Recommended |
| **Per-room channels** (`mudl-void-001`, …) | In-character `say` / `emote`; bot auto-joins | Default mode |
| **Shared rooms channel** | All IC speech in **threads** (set `SLACK_ROOMS_CHANNEL`) | Optional |

### World (OOC) channel

1. Create e.g. `#mudl-ooc`.
2. `/invite @MUDL` (or add the app via channel Integrations).
3. Open channel details → copy **Channel ID** (`C0123456789`) → `SLACK_WORLD_CHANNEL`.

Logged-in players post OOC without a command prefix. Others receive a relay in their bot DM.

### In-character routing

**Named channels (default)** — `SLACK_ROOMS_CHANNEL` unset:

- Speech posts to `mudl-<room-slug>` (prefix: `SLACK_ROOM_CHANNEL_PREFIX`, default `mudl-`).
- Bot calls `conversations.join` when players enter a room.

**Threaded mode** — set `SLACK_ROOMS_CHANNEL=C…`:

- Create one channel (e.g. `#mudl-rooms`), invite the bot.
- IC speech and movement notices go to threads `room-<slug>` inside that channel.

---

## Environment variables

Copy [`.env.example`](../.env.example) and set at minimum:

```bash
# Required for live mode
SLACK_BOT_TOKEN=xoxb-…
SLACK_SIGNING_SECRET=…

# Strongly recommended
SLACK_APP_ID=A…
SLACK_WORLD_CHANNEL=C…

# Server
SLACK_BIND_ADDR=0.0.0.0:3000
SLACK_EVENTS_PATH=/slack/events

# Shared world (same as IRC / REPL)
DATABASE_URL=sqlite://mudl.db
DEFAULT_PLAYER=player:admin-001
MUDL_MODULE=modules/default

# Optional threaded IC mode
# SLACK_ROOMS_CHANNEL=C…

# Local testing without Slack
# SLACK_MOCK=1
```

| Variable | Purpose | Default |
|----------|---------|---------|
| `SLACK_BOT_TOKEN` | Bot OAuth token (`xoxb-…`) | required (live) |
| `SLACK_SIGNING_SECRET` | Events API HMAC verification | required (live) |
| `SLACK_APP_ID` | Strip `<@APP>` from commands | optional |
| `SLACK_WORLD_CHANNEL` | OOC channel id (`C…`) | empty |
| `SLACK_ROOMS_CHANNEL` | Shared channel for per-room threads | unset |
| `SLACK_ROOM_CHANNEL_PREFIX` | Named channel slug prefix | `mudl-` |
| `SLACK_BIND_ADDR` | HTTP listen address | `0.0.0.0:3000` |
| `SLACK_EVENTS_PATH` | Events endpoint path | `/slack/events` |
| `SLACK_MOCK` | Stdin mock mode | unset |
| `DATABASE_URL` | SQLite world file | `sqlite://mudl.db` |
| `MUDL_SINGLE_WRITER_ENABLED` | Advisory DB lock (SEC-23) | `true` |

Login and rate limits (shared with IRC):

| Variable | Slack usage |
|----------|-------------|
| `MUDL_LOGIN_REQUIRE_AUTH` | `true` in live mode; `false` when `SLACK_MOCK=1` |
| `MUDL_LOGIN_TOKENS` | `player:id=secret` |
| `MUDL_LOGIN_IDENTITY_BINDINGS` | `U01234ABC=player:hero-001` (lowercase keys) |
| `MUDL_RATE_LIMIT_*` | Anti-flood on commands, movement, OOC |

---

## Run the bot

```bash
# Development
make run-slack          # needs .env or exports

# Production-style
RUST_LOG=info cargo run --bin slack

# Verify automated tests
make test-m6
make test-slack-flow
```

On startup the bot:

1. Acquires the single-writer lock on `DATABASE_URL` (SEC-23)
2. Bootstraps the active universe if the database is empty
3. Opens [`SessionManager`](../src/gateway/session_manager.rs) with rate-limit policy
4. Listens for Events API POSTs on `SLACK_BIND_ADDR` + `SLACK_EVENTS_PATH`

---

## Player quick start

After the operator shares login credentials:

1. Open a **DM** with the MUDL app.
2. Send `login` (open dev) or `login <token>` / `login player:hero-001 <token>` (secured).
3. Explore:

```text
look
go north
say Hello!
tell Alice meet at north
```

4. Join `#mudl-ooc` for OOC (no command prefix while logged in).
5. `quit` to save and disconnect.

`tell` accepts a connected player's **display name** or Slack user id. In-character speech never crosses room boundaries.

---

## How messages flow

```
Slack workspace ──Events API POST──► axum (/slack/events)
                                         │
                                         ▼
                                   SlackBot ──► SessionManager
                                         │
                         ┌───────────────┼───────────────┐
                         ▼               ▼               ▼
                    DM commands    world channel OOC   room channels / threads
                         │               │               │
                         └───────────────┴───────────────┘
                                         ▼
                              SlackWebTransport (Web API)
```

| Input | Routing |
|-------|---------|
| DM to bot | `login`, `look`, `go`, `take`, `attack`, `say`, `tell`, `quit` |
| World channel | OOC broadcast + DM relay |
| Room channel / thread | IC `say` / `emote` + movement notices (bot posts; players use DMs for commands) |

Multi-user rules mirror IRC: shared world graph, room-scoped `look`, private tells via DM, co-located audience for `say` / `emote`. See [Multi-user group play](#multi-user-group-play) below.

---

## Multi-user group play

| Feature | Behavior |
|---------|----------|
| **Shared world** | One `WorldState`; all sessions see the same rooms and objects |
| **Room `look`** | Current room only — exits, items, other players |
| **`say` / `emote`** | Co-located DMs + room channel/thread; no cross-room bleed |
| **`tell`** | Private DM only |
| **Movement** | Room join/leave + arrival/departure notices |
| **OOC** | World channel + DM relay to logged-in players |

Private delivery uses [`SlackSessionRegistry`](../src/slack/session.rs) to map each `U…` id to its `D…` conversation.

---

## Session & authentication

- Registry key: normalized Slack user id (`U…` / `W…`, case-insensitive).
- Open login (`SLACK_MOCK` / dev): `login` binds by player name or `login player:<id>`.
- Secured login: `MUDL_LOGIN_TOKENS` + optional `MUDL_LOGIN_IDENTITY_BINDINGS`.
- Logout: `quit` / `logout` / `exit` — persists player state and clears session.

---

## Output formatting

Game text is adapted in [`slack/format.rs`](../src/slack/format.rs): Block Kit for room `look`, mrkdwn for speech/OOC/tells, ephemeral notices for errors. Details in source; players see formatted Slack messages automatically.

---

## Security notes

- Verify Events API signatures (enabled by default when `SLACK_SIGNING_SECRET` is set).
- Deploy login tokens before inviting a large workspace.
- Run only **one** live writer per database (Slack **or** IRC **or** REPL).
- See [SECURITY.md](../SECURITY.md) for SEC-01, SEC-23, SEC-50.

---

## Testing

```bash
make test-m6              # unit + integration (slack::, gateway::m6_*)
./scripts/slack-mock-flow.sh   # login → look → say → tell → move → OOC → quit
```

Covers signature verification, session lifecycle, identity bindings, multi-user visibility, and DM routing.

---

## Troubleshooting

| Symptom | Check |
|---------|--------|
| Event URL never verifies | Bot running? Tunnel HTTPS? `SLACK_SIGNING_SECRET` matches app? Path is `/slack/events`? |
| `401` on events | Signing secret mismatch or clock skew (>5 min) |
| Bot ignores DMs | `message.im` subscribed? App reinstalled after scope change? |
| OOC ignored | `SLACK_WORLD_CHANNEL` set? Bot invited to channel? Player ran `login`? |
| `login` denied | `MUDL_LOGIN_REQUIRE_AUTH=true` without token? Use `login <token>` |
| No room speech | Named mode: bot needs `channels:join`; threaded mode: check `SLACK_ROOMS_CHANNEL` |
| Database locked | Another REPL/IRC/Slack process holds `DATABASE_URL` — stop other writers |
| Live mode refuses start | Set both `SLACK_BOT_TOKEN` and `SLACK_SIGNING_SECRET`, or use `SLACK_MOCK=1` |

---

## Architecture reference

Implementation map (for contributors):

| Module | Role |
|--------|------|
| [`slack/server.rs`](../src/slack/server.rs) | axum Events API handler |
| [`slack/bot.rs`](../src/slack/bot.rs) | Event routing, delivery |
| [`slack/dispatch.rs`](../src/slack/dispatch.rs) | Command → `CommandDispatcher` |
| [`slack/session.rs`](../src/slack/session.rs) | DM channel sidecar per user |
| [`slack/transport.rs`](../src/slack/transport.rs) | Web API (`postMessage`, `join`, …) |
| [`slack/format.rs`](../src/slack/format.rs) | mrkdwn / Block Kit |
| [`bin/slack.rs`](../src/bin/slack.rs) | Binary entrypoint |

Room routing modes:

| Mode | Config | IC speech | Movement |
|------|--------|-----------|----------|
| Named channels | `SLACK_ROOMS_CHANNEL` unset | `mudl-<slug>` channel | `conversations.join` / `leave` |
| Threaded | `SLACK_ROOMS_CHANNEL=C…` | thread `room-<slug>` | thread entry/exit notices |

---

## Next steps (M6+)

- Container verbs (`put`, `open`, …) over Slack
- Socket Mode option for firewalled hosts (today: HTTP Events API only)