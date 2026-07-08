# MUDL Slack Bot

**Milestone 6 (in progress)** — The Slack bot is the group-playtesting transport for MUDL. It shares one [`WorldState`](../src/world/world_state.rs) across all connected players via [`SessionManager`](../src/gateway/session_manager.rs), receives workspace events over HTTP, and delivers responses through [`GameTransport`](../src/transport/mod.rs).

Command dispatch via [`slack/dispatch.rs`](../src/slack/dispatch.rs) mirrors IRC — player verbs route through [`CommandDispatcher`](../src/command/dispatcher.rs).

## Session management

Each workspace member is bound to at most one player actor through the shared [`SessionManager`](../src/gateway/session_manager.rs):

| Layer | Key | Holds |
|-------|-----|-------|
| **Game session** | Normalized Slack user id (`U…` / `W…`, case-insensitive) | `Arc<Mutex<Session>>` over shared `WorldState` |
| **Delivery sidecar** | Same normalized id | DM conversation id (`D…`) in [`SlackSessionRegistry`](../src/slack/session.rs) |

**Login flow** (DM to the bot):

1. `dispatch_command` receives `user_id` + `reply_channel` from the Events API message.
2. Open mode: `login` matches player **display name** to the Slack user id string (mock/dev uses names like `alice`; production uses `U…` ids — use `login <player-id>` or a token).
3. Secured mode: `verify_login` checks `MUDL_LOGIN_TOKENS` and optional `MUDL_LOGIN_IDENTITY_BINDINGS` (keys are lowercase Slack user ids, e.g. `U01234ABC=player:hero-001`).
4. `SessionManager::login(user_id, player_id, …)` registers the connection; `SlackBot` records the DM channel for OOC relay and future delivery.

**Logout** (`quit` / `logout` / `exit`): `SessionManager::logout` persists player state, clears rate-limit buckets, and drops the registry entry; `SlackSessionRegistry` removes the DM mapping.

## Architecture

```
Slack workspace ──Events API POST──► axum server (/slack/events)
                                         │
                                         ▼
                                   SlackBot ──► SessionManager (Mutex)
                                         │              │
                                         │              ├── SharedWorld
                                         │              └── PlayerSession × N
                                         ▼
                         SlackWebTransport (GameTransport)
                    chat.postMessage / postEphemeral / conversations.*
```

- **Game commands** arrive as DMs to the bot (`login`, `look`, `go`, `take`, `attack`, …).
- **OOC chat** on `SLACK_WORLD_CHANNEL` broadcasts when logged in (rate-limited).
- **In-character speech** routes to per-room channels **or** threads (see below).

## Room routing (multi-channel / threaded play)

| Mode | Config | In-character `say` / `emote` | Movement `join` / `leave` |
|------|--------|------------------------------|----------------------------|
| **Named channels** (default) | `SLACK_ROOMS_CHANNEL` unset | Posts to `mudl-<room-slug>` channel | `conversations.join` / `leave` |
| **Threaded** | `SLACK_ROOMS_CHANNEL=C…` | Posts in thread `C…:thread:room-<slug>` | Thread entry/exit notices |

Set `SLACK_ROOM_CHANNEL_PREFIX` (default `mudl-`) for named-channel slugs.

## Multi-user group play

All logged-in players share one [`WorldState`](../src/world/world_state.rs) through [`SessionManager`](../src/gateway/session_manager.rs). Visibility and routing mirror IRC:

| Feature | Behavior |
|---------|----------|
| **Shared world** | Movement, inventory, and room state are visible to every connected session via the shared graph |
| **Room `look`** | [`ResolveScope::RoomOnly`](../src/slack/visibility.rs) — shows exits, items, and **other players** in your current room |
| **`say` / `emote`** | Delivered to co-located players (DM) and posted to the room channel or thread; does **not** cross room boundaries |
| **`tell`** | Private DM to one connected player (by display name or Slack user id); never posted to room channels |
| **Movement** | Joins/leaves room presence; notifies co-located players via DM and posts arrival/departure to the room channel |
| **OOC** | World channel broadcast + DM relay to other logged-in players |

### Channel routing

| Surface | Purpose |
|---------|---------|
| **DM to bot** | All game commands (`login`, `look`, `go`, `tell`, …) |
| **`SLACK_WORLD_CHANNEL`** | Out-of-character chat (no command prefix) |
| **Per-room channels** (`mudl-<slug>`) | In-character `say` / `emote` and movement notices (default) |
| **`SLACK_ROOMS_CHANNEL`** | Optional: all in-character speech and movement in per-room **threads** |

Private tells and room-local lines route through [`SlackSessionRegistry`](../src/slack/session.rs) so delivery uses each player's DM conversation id (`D…`), not raw `U…` ids.

## Output formatting

Game text is adapted in [`slack/format.rs`](../src/slack/format.rs) before Web API delivery:

| Output | Slack treatment |
|--------|-----------------|
| **Room `look`** | Block Kit sections — description, divider, *Obvious exits*, _You see…_ |
| **`say` / `emote`** | `*Speaker*` bold + quoted speech or _italic_ action |
| **`tell`** | `:envelope:` whisper styling in DMs |
| **OOC** | `` `[OOC]` `` code label + bold speaker name |
| **Help** | Bulleted command list in a single block message |
| **Notices** | Ephemeral `:warning:` for login/rate-limit hints |

[`SlackBot`](../src/slack/bot.rs) formats at delivery time; [`SlackWebTransport`](../src/slack/transport.rs) sends `blocks` when present (fallback `text` always set for notifications). Reactions are not used — room activity stays in channels/threads.

## GameTransport mapping

[`SlackWebTransport`](../src/slack/transport.rs) implements the shared [`GameTransport`](../src/transport/mod.rs) trait (same surface as IRC [`StreamTransport`](../src/irc/transport.rs)):

| Method | Slack Web API | Recipient format |
|--------|---------------|------------------|
| `send_direct` | `chat.postMessage` | `C…` / `D…` channel id, `C…:thread:TS`, or `U…` (opens DM) |
| `send_notice` | `chat.postEphemeral` | `C…:notice:U…` |
| `join` | `conversations.join` or thread entry notice | channel id or room slug (`mudl-void-001`) |
| `leave` | optional farewell + `conversations.leave` | channel id or room slug |

Helpers in [`slack/presence.rs`](../src/slack/presence.rs) and [`slack/channels.rs`](../src/slack/channels.rs) mirror IRC channel naming for future `slack/dispatch.rs` integration.

## Slack app setup

1. Create an app from [`slack-app-manifest.yaml`](../slack-app-manifest.yaml):
   - https://api.slack.com/apps → **Create New App** → **From an app manifest**
   - Pick a development workspace and paste the manifest.

2. **OAuth & Permissions** — install the app to the workspace and copy the **Bot User OAuth Token** (`xoxb-…`).

3. **Basic Information** — copy the **Signing Secret** and **App ID** (`A…`).

4. **Event Subscriptions** — enable events and set the **Request URL** to your public endpoint:
   ```
   https://YOUR_HOST/slack/events
   ```
   Slack sends a `url_verification` challenge; MUDL responds with the `challenge` field when the signing secret matches.

5. Create a workspace channel for OOC (e.g. `#mudl-ooc`), invite the bot, and copy the **channel ID** (`C…`) from channel details.

6. Configure `.env` (see [`.env.example`](../.env.example)):

```bash
SLACK_BOT_TOKEN=xoxb-your-bot-token
SLACK_SIGNING_SECRET=your-signing-secret
SLACK_APP_ID=A0123456789
SLACK_WORLD_CHANNEL=C0123456789
SLACK_BIND_ADDR=0.0.0.0:3000
SLACK_EVENTS_PATH=/slack/events
DATABASE_URL=sqlite://mudl.db
DEFAULT_PLAYER=player:admin-001

cargo run --bin slack
```

### Local development

Public HTTPS is required for Slack Event Subscriptions. Options:

- **ngrok** / **cloudflared** tunnel to `localhost:3000`
- **Slack mock mode** (no network):

```bash
SLACK_MOCK=1 cargo run --bin slack
```

Type lines as `user_id channel_id command`:

```text
U_ALICE D_DM look
U_ALICE C_WORLD brb dinner
```

## Environment variables

| Variable | Purpose | Default |
|----------|---------|---------|
| `SLACK_BOT_TOKEN` | Bot OAuth token (`xoxb-…`) | required for live mode |
| `SLACK_SIGNING_SECRET` | Events API signature verification | required for live mode |
| `SLACK_APP_ID` | Strip `<@APP>` mentions from commands | optional |
| `SLACK_WORLD_CHANNEL` | Channel ID for OOC | empty |
| `SLACK_ROOMS_CHANNEL` | Shared channel for per-room threads (optional) | unset → named channels |
| `SLACK_BIND_ADDR` | HTTP listen address | `0.0.0.0:3000` |
| `SLACK_EVENTS_PATH` | Events endpoint path | `/slack/events` |
| `SLACK_ROOM_CHANNEL_PREFIX` | Future per-room routing prefix | `mudl-` |
| `SLACK_MOCK` | Stdin mock mode (set any value) | unset |
| `DATABASE_URL` | Shared SQLite world | `sqlite://mudl.db` |
| `MUDL_SINGLE_WRITER_ENABLED` | Advisory DB lock (SEC-23) | `true` |

Login tokens and rate limits use the same `MUDL_LOGIN_*` and `MUDL_RATE_LIMIT_*` variables as the IRC bot.

| Variable | Slack usage |
|----------|-------------|
| `MUDL_LOGIN_REQUIRE_AUTH` | Require token before `SessionManager::login` (default `true`; `false` when `SLACK_MOCK=1`) |
| `MUDL_LOGIN_TOKENS` | `player:id=secret` — same as IRC |
| `MUDL_LOGIN_IDENTITY_BINDINGS` | `U01234ABC=player:hero-001` — binds a Slack member id to one actor (keys normalized to lowercase) |

## Security notes

- Every Events API request is verified with `X-Slack-Signature` / `X-Slack-Request-Timestamp` (5-minute replay window).
- Run only one live writer (Slack **or** IRC **or** REPL) against the same `DATABASE_URL`.
- Deploy `MUDL_LOGIN_TOKENS` before exposing the bot to a shared workspace.

## Tests

```bash
make test-m6
```

Covers payload parsing, signature verification, session login/logout, identity bindings, OOC relay, and `gateway::m6_scenarios` acceptance flows.

## Commands (DM the bot)

```text
login
look
go north
say Hello!
tell Bob psst
quit
```

`tell` accepts a connected player's **display name** or Slack user id. OOC goes to the world channel without a prefix.

## Next steps (M6)

- Container verbs (`put`, `open`, …) over Slack