# MUDL IRC Bot

**Milestone 5 (complete)** — The IRC bot is the multi-user transport for MUDL. It shares one [`WorldState`](../src/world/world_state.rs) across all connected players via [`SessionManager`](../src/gateway/session_manager.rs), routes commands through [`IrcBot`](../src/irc/bot.rs), and enforces room-local visibility for in-character speech. Command reference and output style: [COMMANDS.md](../COMMANDS.md#irc-bot-m5).

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
5. If `IRC_NICKSERV_PASSWORD` is set, sends NickServ `REGISTER` (when `IRC_NICKSERV_EMAIL` is set) and/or `IDENTIFY` for the **bot nick**
6. Joins the world channel

Players should also connect over TLS in their IRC client (port 6697 on Libera Chat, for example).

Send commands as private messages to the bot nick (`/msg mudl …` in most clients):

```text
/msg mudl login
/msg mudl look
/msg mudl say Hello, void!
/msg mudl tell alice psst
```

The bot also accepts `/msg mudl …` and `/query mudl …` pasted directly into mock mode input.

### Nick handling and trust boundary

MUDL trusts the IRC **server** to authenticate nicks (SASL, NickServ `+r`, etc.). The bot reads the message prefix and optional IRCv3 `account-tag`; it does not perform SASL itself.

| Layer | Behavior |
|-------|----------|
| **Wire parse** | Nicks are validated against IRC rules (length ≤ 30, no control chars). Invalid prefixes are ignored. |
| **Session key** | Canonical **lowercase** nick (`Alice` → `alice`). |
| **OOC display** | Sanitized wire nick (control chars stripped, single-line body, 400-char cap). |
| **In-character** | Player **object name**, not IRC nick (`Alice says, "hi"`). |
| **`tell`** | Case-insensitive nick resolve; confirmation uses canonical nick. |

**Operator requirements for production:**

1. **TLS** to the IRC network (`IRC_TLS=true`, port 6697).
2. **Registered nicks** or **SASL** on the IRC network so others cannot impersonate players.
3. **MUDL login tokens** (`MUDL_LOGIN_REQUIRE_AUTH=true`) — binds game session to credentials you issue.
4. **Optional** `MUDL_LOGIN_IDENTITY_BINDINGS` — lock IRC nick → player id at login.
5. **Optional** `IRC_REQUIRE_ACCOUNT_TAG=true` — reject PRIVMSG without IRCv3 `account-tag` (requires identified/SASL account on the network).
6. **Optional** `MUDL_IRC_ACCOUNT_BINDINGS=alice=AccountName` — require a specific SASL/account name per nick.

```bash
# Strict public playtest example (Libera Chat)
IRC_IRCV3=true
MUDL_LOGIN_REQUIRE_AUTH=true
MUDL_LOGIN_TOKENS=player:hero-001=rotate-this-secret
MUDL_LOGIN_IDENTITY_BINDINGS=alice=player:hero-001
IRC_REQUIRE_ACCOUNT_TAG=true
```

On networks with `account-tag`, identified users receive `@account=YourAccount` on each message. Unidentified clients send `account=*`; MUDL rejects those when `IRC_REQUIRE_ACCOUNT_TAG=true`.

## NickServ (register & identify)

Most public IRC networks (Libera Chat, etc.) require a **registered, identified nick** before others can trust who you are. MUDL does not run SASL in the client; it relies on the network’s NickServ and IRCv3 `account-tag` for identity verification.

### Why it matters for MUDL

| Step | What happens |
|------|----------------|
| **Register** | Claims your IRC nick on the network (one-time, from your IRC client). |
| **Identify** | Proves you own that nick (`+r` / `account-tag` on your messages). |
| **MUDL login** | Binds your IRC nick to a game player (`login` + token when auth is on). |
| **Account binding** | Optional `MUDL_IRC_ACCOUNT_BINDINGS` locks nick → NickServ account name. |

When `IRC_REQUIRE_ACCOUNT_TAG=true`, players must **identify to NickServ before any MUDL command** (including `login`). The bot rejects unidentified PRIVMSG with a notice to identify first.

### Bot operator setup

Configure the **bot’s own** NickServ credentials in `.env` so the bot nick is registered and identified after connect:

```bash
IRC_NICKSERV_SERVICE=NickServ          # default; change if your network uses a different service nick
IRC_NICKSERV_PASSWORD=bot-secret       # bot account password — IDENTIFY after welcome
# IRC_NICKSERV_EMAIL=bot@example.com  # optional: one-time REGISTER for the bot nick, then remove
```

| Variable | Default | Description |
|----------|---------|-------------|
| `IRC_NICKSERV_SERVICE` | `NickServ` | NickServ service nick to PRIVMSG |
| `IRC_NICKSERV_PASSWORD` | *(unset)* | Bot password; triggers auto-`IDENTIFY` after `001` welcome |
| `IRC_NICKSERV_EMAIL` | *(unset)* | With password, sends `REGISTER` once before `IDENTIFY` (first-time bot setup) |

**First-time bot registration on Libera Chat:**

1. Start the bot with `IRC_NICKSERV_EMAIL` and `IRC_NICKSERV_PASSWORD` set (pick a strong password and a valid email).
2. Confirm NickServ accepts registration (check bot logs or `/msg NickServ INFO` from an operator client).
3. Remove `IRC_NICKSERV_EMAIL` from `.env` — only `IRC_NICKSERV_PASSWORD` is needed on subsequent starts.

### Player registration (IRC client)

Nick **registration** must be done from the player’s **own IRC connection** (NickServ ties registration to the nick you are currently using). The bot cannot register a player nick on your behalf.

In your IRC client (replace placeholders):

```text
/msg NickServ REGISTER YourPassword your.email@example.com
```

Libera Chat also documents this at [https://libera.chat/guides/registration](https://libera.chat/guides/registration). Save the email NickServ sends — it contains a verification command.

If you message the bot `nickserv register …`, MUDL replies with the same client-side instruction (it does not relay `REGISTER`).

### Player identification

**Option A — via the MUDL bot** (password is relayed to NickServ but **never echoed** back to you):

```text
/msg mudl nickserv identify YourPassword
```

Shorthand (logged out only):

```text
/msg mudl identify YourPassword
```

The bot sends `IDENTIFY <your-nick> <password>` to NickServ. When NickServ confirms, you receive a NOTICE and can proceed with `login`.

**Option B — directly in your IRC client** (recommended if you prefer not to send the password through the bot):

```text
/msg NickServ IDENTIFY YourPassword
```

On some networks, if you are using a different nick temporarily:

```text
/msg NickServ IDENTIFY YourNick YourPassword
```

### Recommended player flow (strict networks)

```text
# 1. Register once (IRC client only)
/msg NickServ REGISTER my-secret my.email@example.com

# 2. Identify (client or bot)
/msg NickServ IDENTIFY my-secret
# or: /msg mudl nickserv identify my-secret

# 3. Log in to MUDL
/msg mudl login player:hero-001 my-mudl-token
```

### Linking NickServ account to MUDL auth

For public playtests, combine network identity with MUDL tokens:

```bash
IRC_REQUIRE_ACCOUNT_TAG=true
MUDL_IRC_ACCOUNT_BINDINGS=alice=AliceAccountName
MUDL_LOGIN_IDENTITY_BINDINGS=alice=player:hero-001
MUDL_LOGIN_TOKENS=player:hero-001=rotate-this-secret
```

- `MUDL_IRC_ACCOUNT_BINDINGS` — IRC nick → **NickServ account name** (from `account-tag`).
- `MUDL_LOGIN_IDENTITY_BINDINGS` — IRC nick → **player object id** at MUDL login.

After `nickserv identify`, your next PRIVMSG should carry `@account=AliceAccountName` (when the server supports `account-tag`). MUDL then allows commands and enforces the binding map.

### NickServ commands via the bot

Available **before** MUDL login (private message to the bot):

| Command | Description |
|---------|-------------|
| `nickserv help` | NickServ setup summary (alias: `ns help`) |
| `nickserv identify <password>` | Relay `IDENTIFY` to NickServ for your nick |
| `identify <password>` | Shorthand for `nickserv identify` |

Passwords are never repeated in bot replies. Use `nickserv help` for full syntax.

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
| `IRC_REQUIRE_ACCOUNT_TAG` | `false` | Reject PRIVMSG without IRCv3 `account-tag` (identified/SASL account) |
| `MUDL_IRC_ACCOUNT_BINDINGS` | *(unset)* | `nick=AccountName` — optional per-nick SASL account lock |
| `IRC_NICKSERV_SERVICE` | `NickServ` | NickServ service nick for bot startup and player relay |
| `IRC_NICKSERV_PASSWORD` | *(unset)* | Bot NickServ password — auto-`IDENTIFY` after welcome |
| `IRC_NICKSERV_EMAIL` | *(unset)* | Optional email for one-time bot `REGISTER` |

### IRCv3 capabilities requested

The bot requests these capabilities from the server (see [`IRCV3_CAPABILITIES`](../src/irc/capability.rs)):

- `cap-notify`, `server-time`, `message-tags`, `echo-message`
- `batch`, `labeled-response`, `account-tag`

Plaintext (`IRC_TLS=false`, port `6667`) is supported for local development but is **not recommended** for production.

## Login

Live IRC requires authenticated login (SEC-01). Mock mode (`IRC_MOCK=1`) and unit tests use open login unless `MUDL_LOGIN_REQUIRE_AUTH=true`.

| Command | When auth **off** (dev) | When auth **on** (production) |
|---------|------------------------|-------------------------------|
| `login` | Bind nick to player whose **name** matches | Denied — token required |
| `login player:hero-001` | Bind to explicit player id | Denied without token |
| `login <token>` | — | Resolve player by token and bind nick |
| `login player:hero-001 <token>` | — | Bind explicit id after token check |

### Configuring credentials

| Source | Purpose |
|--------|---------|
| `MUDL_LOGIN_TOKENS` | `player:hero-001=secret,player:hero-002=other` — operator-managed secrets |
| `login_token` property | Per-player token on the object (`@set hero login_token secret`) |
| `MUDL_LOGIN_IDENTITY_BINDINGS` | `alice=player:hero-001` — optional IRC nick → player lock |

Environment variables (see [`.env.example`](../.env.example)):

```bash
MUDL_LOGIN_REQUIRE_AUTH=true
MUDL_LOGIN_TOKENS=player:hero-001=change-me
MUDL_LOGIN_IDENTITY_BINDINGS=alice=player:hero-001
```

Failed logins return `Invalid login credentials.` without revealing whether the player id or token was wrong.

On networks with `IRC_REQUIRE_ACCOUNT_TAG=true`, identify to NickServ **before** `login` (see [NickServ](#nickserv-register--identify)).

Players must log in before other commands work. `quit` saves state and disconnects.

## Commands

| Command | Description |
|---------|-------------|
| `nickserv identify <password>` | Identify your IRC nick via NickServ (before login; password not echoed) |
| `nickserv help` | NickServ registration and identification help |
| `identify <password>` | Shorthand for `nickserv identify` (logged out) |
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
| [`SessionManager`](../src/gateway/session_manager.rs) | `Arc<tokio::sync::Mutex<…>>` | **Sole connection registry** — login, logout, nick→actor map, per-nick session handles |
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

Run the full M5 suite:

```bash
make test-m5
# or:
cargo test gateway:: && cargo test irc::
```

Individual modules:

```bash
cargo test irc::
cargo test gateway::multi_user
cargo test gateway::session_manager
cargo test gateway::load
cargo test gateway::edge_cases
cargo test gateway::m5_scenarios
```

Coverage includes:

- **IRC layer** (`irc::`) — message parsing, IRCv3 caps, channel naming, visibility, dispatch, bot relay, NickServ identify relay, input shorthands
- **Session manager** (`gateway::session_manager`) — login/logout lifecycle, nick registry, disconnect persist
- **Multi-user** (`gateway::multi_user`) — shared world movement, room-boundary `say`/`emote`, private `tell`, concurrent `go`/`take`, logout isolation, mixed-case nicks
- **Load** (`gateway::load`) — parallel command stress, deadlock avoidance, latency under contention
- **Edge cases** (`gateway::edge_cases`) — disconnect/reconnect, IRC `QUIT`, double logout, login while connected, RBAC denials, revision-conflict retry on logout, orphan `connect()` reclaim
- **Acceptance** (`gateway::m5_scenarios`) — explicit player login, shorthand movement/`say`/`emote`, whisper alias, OOC login gate, room channel JOIN/PART on `go`, per-actor inventory isolation on `take`

Mock transport is used — no live TLS connection in CI. The full project suite is **532** tests (`make dev` or `cargo test`).