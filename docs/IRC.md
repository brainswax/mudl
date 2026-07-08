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

```text
/msg mudl login
/msg mudl look
/msg mudl say Hello, void!
/msg mudl tell alice psst
```

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
| `help` | Command summary |
| `quit` | Persist and disconnect |

Builder/meta commands (`@dig`, `@set`, …) are RBAC-checked but deferred to the REPL for now.

## Channels

| Channel | Purpose |
|---------|---------|
| `#mudl` (configurable) | World / OOC chat — any text is broadcast as `[OOC] nick: message` |
| `#mudl-<room-slug>` | Per-room in-character speech and emotes (e.g. `#mudl-void-001` for `room:void-001`) |

The bot joins room channels as players enter places and parts when they leave. Players should join their current room channel in their IRC client to see channel traffic natively; the bot also relays room speech via private message to co-located players who have not joined.

## Concurrency

- [`SessionManager`](../src/gateway/session_manager.rs) sits behind a `tokio::sync::Mutex`.
- Each command acquires the manager lock, then [`Session::with_locked`](../src/repl/session.rs) holds the world mutex for the full command.
- Disconnect persists per-player actor state and flushes world dirty objects (optimistic revision saves).

## Tests

```bash
cargo test irc::
```

Tests cover message parsing, IRCv3 capability helpers, channel naming, visibility, dispatch, and multi-player bot integration. Mock transport is used — no live TLS connection in CI.