# MUDL IRC Bot

The IRC bot is the multi-user transport for MUDL (M5). It shares one [`WorldState`](../src/world/world_state.rs) across all connected players via [`SessionManager`](../src/gateway/session_manager.rs), routes commands through [`IrcBot`](../src/irc/bot.rs), and enforces room-local visibility for in-character speech.

**Target environment:** an [IRCv3](https://ircv3.net)-capable server over **TLS** (default port **6697**). The bot negotiates IRCv3 capabilities during registration and uses `rustls` with the Mozilla root store for certificate verification.

## Architecture

```
IRC client ──PRIVMSG──► IrcBot ──► SessionManager (Mutex)
                         │              │
                         │              ├── SharedWorld (Mutex per command)
                         │              └── PlayerSession × N
                         ▼
              StreamTransport (TLS or plain TCP)
```

- **Commands** go to the bot nick as private messages.
- **In-character `say` / `emote`** reach co-located players and the matching room channel.
- **Private `tell`** delivers only to the target nick.
- **OOC chat** on the world channel (`#mudl` by default) broadcasts to all logged-in players.

## Quick Start

### Mock mode (local testing)

Mock mode skips the network entirely — useful for unit tests and local command rehearsal:

```bash
cargo build --bin irc
IRC_MOCK=1 cargo run --bin irc
```

Type lines as `nick command`, for example:

```text
alice login
bob login
alice say hello
alice tell bob secret
alice go north
alice quit
```

### Live IRCv3 server (TLS)

Configure `.env` (see [`.env.example`](../.env.example)):

```bash
IRC_SERVER=irc.libera.chat
IRC_PORT=6697
IRC_TLS=true
IRC_IRCV3=true
IRC_BOT_NICK=mudl
IRC_REALNAME=MUDL Bot
IRC_WORLD_CHANNEL=#mudl
DATABASE_URL=sqlite://mudl.db
DEFAULT_PLAYER=player:admin-001

cargo run --bin irc
```

On connect the bot:

1. Opens a **TLS** socket to `IRC_SERVER:IRC_PORT`
2. Sends `CAP LS 302`, `NICK`, `USER`
3. Requests IRCv3 capabilities (`server-time`, `message-tags`, `cap-notify`, …)
4. Sends `CAP END` and waits for `001` welcome
5. Joins the world channel

Players should also connect over TLS in their IRC client (port 6697 on Libera Chat, for example).

Send commands as private messages to the bot nick (`/msg mudl …` in most clients):

```text
/msg mudl login
/msg mudl look
/msg mudl say Hello, void!
/msg mudl tell alice psst
```

The bot also accepts `/msg mudl …` and `/query mudl …` pasted directly into mock mode input.

### Nick handling

- Session keys are **case-insensitive** (`Alice` and `alice` are the same player).
- Outgoing PRIVMSG and NOTICE targets use the canonical lowercase nick stored at login.
- In-character text uses the player's **object name** (e.g. `Alice says, "hi"`), not the IRC nick.
- OOC lines keep the sender's IRC nick as received (`[OOC] Alice: brb`).
- `tell` resolves targets case-insensitively; confirmation uses the resolved nick (`You tell bob, "…"`).

### Output formatting

- Multi-line responses (room descriptions, movement) are sent as **one IRC line per PRIVMSG** — no embedded newlines.
- `help` is delivered as separate lines for readability in IRC clients.
- In-character speech: `Alice says, "…"`; emotes: `Alice waves.`; tells: `Bob tells you, "…"`.

Join `#mudl` for out-of-character chat. Room channels (`#mudl-void-001`, etc.) receive in-character speech and emotes; the bot sends a NOTICE with the channel name when you enter a place.

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `IRC_SERVER` | `irc.libera.chat` | Server hostname (TLS SNI) |
| `IRC_PORT` | `6697` | Server port |
| `IRC_TLS` | `true` | Use TLS (`false` for plaintext dev only) |
| `IRC_IRCV3` | `true` | Negotiate IRCv3 capabilities at registration |
| `IRC_BOT_NICK` | `mudl` | Bot nickname |
| `IRC_REALNAME` | `MUDL Bot` | `USER` realname field |
| `IRC_WORLD_CHANNEL` | `#mudl` | Global OOC channel |
| `IRC_ROOM_CHANNEL_PREFIX` | `#mudl-` | Per-room channel prefix |
| `IRC_MOCK` | *(unset)* | Set to any value to enable stdin mock mode |

### IRCv3 capabilities requested

The bot requests these capabilities from the server (see [`IRCV3_CAPABILITIES`](../src/irc/capability.rs)):

- `cap-notify`, `server-time`, `message-tags`, `echo-message`
- `batch`, `labeled-response`, `account-tag`

Plaintext (`IRC_TLS=false`, port `6667`) is supported for local development but is **not recommended** for production.

## Login

| Command | Behavior |
|---------|----------|
| `login` | Bind IRC nick to a player whose **name** matches (case-insensitive) |
| `login player:hero-001` | Bind to an explicit player object id |

Players must log in before other commands work. `quit` saves state and disconnects.

## Commands

| Command | Description |
|---------|-------------|
| `look` (`l`) | Room or object description (private to you) |
| `go <dir>` | Move — also accepts standalone exit names (`north`, `n`, …) |
| `inventory` (`i`) | List carried items |
| `take <item>` | Pick up an item |
| `say <text>` | Speak to players in your room |
| `emote <text>` | Emote to players in your room |
| `tell <nick> <text>` | Private message to a connected player |
| `help` (`?`) | Command summary (one line per reply) |
| `quit` (`logout`, `exit`) | Persist and disconnect |
| `'` | Shorthand for `say` |
| `:` | Shorthand for `emote` |
| `whisper` | Alias for `tell` |

Shorthand movement: `north`, `n`, and other exit names work without `go` when unambiguous.

Builder/meta commands (`@dig`, `@set`, …) are RBAC-checked but deferred to the REPL for now.

## Channels

| Channel | Purpose |
|---------|---------|
| `#mudl` (configurable) | World / OOC chat — any text is broadcast as `[OOC] nick: message` |
| `#mudl-<room-slug>` | Per-room in-character speech and emotes (e.g. `#mudl-void-001` for `room:void-001`) |

The bot joins room channels as players enter places and parts when they leave. Players should join their current room channel in their IRC client to see channel traffic natively; the bot also relays room speech via private message to co-located players who have not joined.

## Concurrency

Lock order is always **manager (brief) → per-session → world**. There is no re-entrant world lock on the same task.

| Layer | Type | Scope |
|-------|------|--------|
| [`SessionManager`](../src/gateway/session_manager.rs) | `Arc<tokio::sync::Mutex<…>>` | Login, logout, registry — held only for lifecycle and nick lookup |
| Per-connection session | `Arc<tokio::sync::Mutex<Session>>` | One mutex per IRC nick; different players can run commands in parallel |
| [`SharedWorld`](../src/world/world_state.rs) | `Arc<tokio::sync::Mutex<WorldState>>` | Serializes in-memory graph mutations (movement, take, events) |

IRC handlers use [`Session::with_locked_async`](../src/repl/session.rs) (`world.lock().await`). The sync REPL keeps [`with_locked`](../src/repl/session.rs) (`lock_blocking` with spin + yield).

Persistence releases the world mutex before SQLite I/O so other connections can proceed. [`IrcBot::deliver`](../src/irc/bot.rs) flushes dirty objects via `SharedWorld::persist_changes` without holding the manager lock during disk writes.

### Performance tips

- Run load tests: `cargo test gateway::load`
- Mock mode for local dev: `IRC_MOCK=1 cargo run --bin irc` (no TLS)
- Contention shows up as parallel `look`/`say` waiting on the world lock — expected until per-room locking is added
- Avoid long-running builder work over IRC; meta commands are RBAC-checked but deferred to the REPL

## Tests

```bash
cargo test irc::
cargo test irc::input
cargo test irc::message
cargo test gateway::multi_user
cargo test gateway::load
cargo test gateway::edge_cases
```

Coverage includes:

- **IRC layer** — message parsing, IRCv3 caps, channel naming, visibility, dispatch, bot relay
- **Multi-user** (`gateway::multi_user`) — shared world movement, room-boundary `say`/`emote`, private `tell`, concurrent `go`/`take`, logout isolation, mixed-case nicks
- **Edge cases** (`gateway::edge_cases`) — disconnect/reconnect, IRC `QUIT`, double logout, login while connected, RBAC denials, revision-conflict retry on logout, orphan `connect()` reclaim

Mock transport is used — no live TLS connection in CI.