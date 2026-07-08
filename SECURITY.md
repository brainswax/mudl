# MUDL Security Review — Milestone 5 (Multi-user IRC)

*Review date: July 2026. Scope: M5 multi-user IRC transport, shared persistence, and command dispatch. **M5/M6 remediations applied July 2026** — see [Remediation status](#remediation-status-july-2026) below.*

## Scope & threat model

| Assumption | Notes |
|------------|-------|
| **Deployment** | Single-world SQLite backend; one IRC bot process; optional separate REPL process against the same `DATABASE_URL`. |
| **Adversary** | Untrusted IRC users on a public network; curious/cooperative players; abusive flooders; **not** assumed: OS root on the host (except where noted for DB file access). |
| **Trust boundary** | The IRC **server** authenticates nicks (NickServ/SASL/etc.); MUDL trusts the `from` field on inbound `PRIVMSG` lines. |
| **Out of scope** | Host hardening, TLS certificate pinning policy, Libera Chat operational security, LLM prompt injection (M10+). |

## Executive summary

M5 is **safe for controlled playtests** on a single IRC bot instance when operators deploy the July 2026 controls: **login tokens**, **rate limits**, **`WriterGuard` single-writer lock**, and optional **IRC `account-tag`** verification. The persistence layer uses **parameterized SQL**; in-process concurrency is **serialized on `SharedWorld`** with optimistic revision checks. **Player-supplied text is not evaluated as MUDL or Rust.**

**P0 findings from the original M5 review are resolved or mitigated in code.** Residual risk is primarily **operator configuration** and **REPL as a privileged bypass** (SEC-12).

| Original P0 | Status (July 2026) |
|-------------|-------------------|
| SEC-01 passwordless login | **Mitigated** — `LoginAuthPolicy` (`MUDL_LOGIN_TOKENS`, identity bindings); permissive only in `IRC_MOCK` / `SLACK_MOCK` / dev |
| SEC-23 split-brain | **Mitigated** — `WriterGuard` advisory lock (`MUDL_SINGLE_WRITER_ENABLED`, default on file DBs) |
| SEC-50 no rate limiting | **Resolved** — token buckets on dispatch + OOC + movement (`gateway/rate_limit.rs`) |
| SEC-60 cross-room look | **Resolved** — `irc_look_scope()` = `ResolveScope::RoomOnly` |

**Still not fully production-hardened** for unattended open Internet play:

1. **Login auth is operator-dependent** — tokens must be rotated; NickServ identify relay and `account-tag` checks assist network identity but do not replace MUDL tokens.
2. **REPL bypass** — separate process with full builder surface and permissive RBAC defaults (SEC-12).
3. **Builder meta deferred but RBAC-probed** — `@` commands leak tier requirements; execution blocked, not absent from the attack surface.
4. **MUDL `@trigger` script power** — trusted-builder model; no sandbox (SEC-32).

---

## Remediation status (July 2026)

| ID | Remediation | Module(s) | Tests |
|----|-------------|-----------|-------|
| SEC-01 | `LoginAuthPolicy`, `verify_login`, env `MUDL_LOGIN_*` | `gateway/login_auth.rs`, `irc/dispatch.rs` | `gateway::login_auth`, `irc::dispatch` login tests |
| SEC-03 | Nick validation/sanitization; optional `account-tag` + bindings; NickServ identify relay | `irc/nick.rs`, `irc/identity.rs`, `irc/message.rs`, `irc/nickserv.rs`, `irc/bot.rs` | `irc::nick`, `irc::identity`, `irc::nickserv`, `irc::bot` |
| SEC-23 | `WriterGuard` RAII lock per database file | `persistence/writer_lock.rs`, `bin/irc.rs`, `bin/repl.rs` | `persistence::writer_lock` |
| SEC-50 | `RateLimiter` — command / movement / OOC buckets | `gateway/rate_limit.rs`, `irc/dispatch.rs`, `irc/bot.rs`, `repl/session.rs` | `gateway::rate_limit`, `irc::dispatch`, `irc::bot` |
| SEC-60 | IRC look uses `RoomOnly` via `PlayerDispatchOptions` | `irc/visibility.rs`, `command/dispatcher.rs` | `irc::visibility` |
| — | Transport DRY: `CommandDispatcher`, `attack`/`drop` on IRC | `command/dispatcher.rs`, `irc/dispatch.rs` | `command::dispatcher`, `irc::dispatch` |
| — | `GameTransport` trait for shared delivery | `transport/mod.rs` | `transport` |

**590** library tests pass (`cargo test --lib`).

---

## Findings

### Authentication & session binding

| ID | Location | Issue | Impact | Recommendation | Priority |
|----|----------|-------|--------|----------------|----------|
| **SEC-01** | `gateway/login_auth.rs`, `irc/dispatch.rs`, `irc/nickserv.rs`, `slack/dispatch.rs` | ~~**Passwordless login**~~ — **Mitigated (July 2026):** `LoginAuthPolicy` requires tokens on live IRC/Slack (`MUDL_LOGIN_TOKENS`, `login_token` property, optional `MUDL_LOGIN_IDENTITY_BINDINGS` — IRC nicks or Slack `U…` ids, lowercase keys). Open login remains for `IRC_MOCK` / `SLACK_MOCK` / `MUDL_LOGIN_REQUIRE_AUTH=false`. | Residual: operators must deploy tokens; binding map optional; NickServ identify relay does not replace MUDL tokens. | Rotate tokens; use identity bindings (`U01234ABC=player:id` for Slack); enable `IRC_REQUIRE_ACCOUNT_TAG` + `nickserv identify` on public IRC networks. | **P1** (residual) |
| **SEC-02** | `gateway/session_manager.rs` `build_connection` | **One actor, one session** enforced (`is_actor_bound`); nick reuse blocked (`RegistryError::NickInUse`). | Mitigates duplicate-world presence for same player; does **not** stop SEC-01 initial bind. | Keep; extend with auth before bind. | — |
| **SEC-03** | `irc/message.rs`, `irc/nick.rs`, `irc/identity.rs`, `IrcBot` | **IRC nick from wire** — validated/sanitized at parse; optional `IRC_REQUIRE_ACCOUNT_TAG` + `MUDL_IRC_ACCOUNT_BINDINGS`; OOC sanitized (no embedded newlines/control chars). | Residual: MUDL does not run SASL — operators must enforce network-level nick ownership. | Deploy SASL/`+r` on IRC network + MUDL tokens; enable `IRC_REQUIRE_ACCOUNT_TAG` for public playtests. | **P1** (mitigated) |
| **SEC-04** | `bin/irc.rs` `run_mock_bot` (`IRC_MOCK=1`) | Stdin lines choose arbitrary nick + command with **no auth**. | Local dev only; if `IRC_MOCK` enabled on a shared host, full impersonation. | Refuse `IRC_MOCK` unless `RUST_ENV=development` or explicit opt-in; document in operator guide. | **P2** |
| **SEC-05** | `gateway/session_manager.rs` `reclaim_orphan_nick` | Orphan registry entries from `connect()` without `release` can be reclaimed on matching `login`. | Edge-case nick squatting after crashed test harness; low risk in production bot path. | Prefer `login` only in production; deprecate orphan `connect()` from external adapters. | **P3** |

### Authorization & privilege escalation

| ID | Location | Issue | Impact | Recommendation | Priority |
|----|----------|-------|--------|----------------|----------|
| **SEC-10** | `irc/dispatch.rs` `dispatch_meta` | Meta commands (`@set`, `@dig`, …) hit `authorize_meta_command` then return *"not enabled yet"* — **no mutation**, but error messages confirm tier (wizard vs builder). | Information leak about player object `PermissionFlags`; no direct escalation via IRC today. | When enabling meta (M7), re-check auth on **every** handler; audit log denials. | **P1** (on enable) |
| **SEC-11** | `gateway/rbac.rs`, `object/PermissionFlags` | Authorization reads **`permissions` on the player `Object`** in the world graph (JSON/SQLite). | Direct SQLite edit or REPL `@set` on permissions elevates IRC actor tier; compromised world file = full wizard. | Sign or checksum player rows (future); restrict REPL on production; separate builder DB role (M7). | **P1** |
| **SEC-12** | `bin/repl.rs` vs `gateway/rbac` | REPL is a **separate process** with full builder command surface; permissive local RBAC defaults. | Operator with REPL access bypasses IRC meta deferral entirely. | `WriterGuard` blocks concurrent REPL+IRC writers; no REPL on production world while IRC is live; unified service (M7). | **P1** (ops) |
| **SEC-13** | `irc/dispatch.rs` logged-out path | Only `login` / `help` accepted when logged out; other verbs rejected; `login` counted in command rate bucket. | Good default deny; brute-force login throttled with general command limit. | Keep; consider dedicated login-failure bucket if needed. | — |

### SQL injection & persistence

| ID | Location | Issue | Impact | Recommendation | Priority |
|----|----------|-------|--------|----------------|----------|
| **SEC-20** | `persistence/sqlite.rs` | All queries use **sqlx `?` binds** (`save_object`, `load_object`, `save_objects_batch`, counters). | **No SQL injection** from IRC command strings or `ObjectId` values under normal code paths. | Maintain bind-only pattern; forbid dynamic SQL in future admin tools. | — |
| **SEC-21** | `persistence/sqlite.rs` `load_object` | Object graph loaded via `serde_json::from_str` on `data` column. | **Untrusted JSON** if attacker has DB write access → arbitrary `permissions`, `event_handlers`, vitals. | File permissions on `repl.db`; optional schema validation on hydrate (M7 graph validator). | **P1** |
| **SEC-22** | `world/world_state.rs` `persist_changes` | Optimistic **`revision` CAS** + retry on `RevisionConflict`. | Mitigates lost updates for concurrent saves **within one process**; `edge_cases::logout_persists_despite_revision_conflict` covers logout path. | Keep; extend monitoring for conflict storms (M9). | — |
| **SEC-23** | `persistence/writer_lock.rs`, `bin/irc.rs`, `bin/repl.rs` | ~~**Two processes, one SQLite file**~~ — **Mitigated (July 2026):** `WriterGuard` acquires an exclusive advisory lock before DB open (default enabled for file URLs). | Residual: separate in-memory graphs if lock disabled (`MUDL_SINGLE_WRITER_ENABLED=0`); not a unified service process. | Keep lock enabled in production; run IRC **or** REPL, not both; unified service optional (M7). | **P1** (residual ops) |
| **SEC-24** | `IrcConfig::database_url` | Default `sqlite://mudl.db` — world state at rest **unencrypted**. | Host filesystem compromise exposes full world + player permissions. | Filesystem ACLs; encrypted volume; future SQLCipher if threat model requires. | **P2** |

### MUDL / code injection (player input)

| ID | Location | Issue | Impact | Recommendation | Priority |
|----|----------|-------|--------|----------------|----------|
| **SEC-30** | `command/parse.rs`, `command/dispatcher.rs` | Player input split into verb/args; **no eval** of user text as MUDL. | Player chat cannot upload scripts at parse time. | Keep; player verbs centralized in `CommandDispatcher`. | — |
| **SEC-31** | `irc/dispatch.rs` | IRC command surface is a **fixed match table**; unknown verbs fall through to movement resolver only. | No arbitrary verb execution from player strings. | Keep whitelist when extending dispatcher. | — |
| **SEC-32** | `world/event_script.rs` | `@trigger` scripts (loaded at bootstrap) execute **fixed Rust actions** (`set-property`, `teleport`, `spawn`, `damage`, …) with **no permission check** inside script executor. | **Supply-chain / builder trust**: malicious or buggy MUDL content can mutate world when triggered by normal play (`on_take`, `on_enter`, …). Not player-authored runtime code, but equivalent to **server-side include**. | Treat MUDL packs as trusted code; graph validator + builder audit (M7); sandbox runtime (M9). | **P1** |
| **SEC-33** | `inventory/mod.rs` `take_item` → `execute_event(ON_TAKE)` | Player `take` triggers host scripts with `actor_id` = player. | Intended gameplay; scripts can `damage`, `teleport`, `grant-effect` the actor. | Cap script power per event class; builder review for public modules. | **P2** |
| **SEC-34** | IRC meta deferral | `@import`, `module reload` **not reachable** over IRC (meta blocked). | Players cannot hot-load MUDL over IRC today. | Re-enable only with wizard auth + validation pipeline (M7/M10). | — |

### Concurrent modification & integrity

| ID | Location | Issue | Impact | Recommendation | Priority |
|----|----------|-------|--------|----------------|----------|
| **SEC-40** | `SharedWorld` mutex + `Session::with_locked_async` | Graph mutations for a command run **under one world lock** per session. | **TOCTOU-safe in single process** for take/move/go; `gateway::multi_user::concurrent_take_only_one_player_gets_sword` verifies exclusive take. | Keep; profile lock hold time under load (`gateway::load`). | — |
| **SEC-41** | `DirtyTracker` + `persist_changes` | Dirty set drained atomically under lock; snapshots taken before batch I/O. | Concurrent commands may interleave dirty marks; batch save retries on revision conflict. | Document semantics; alert on repeated conflict failures. | **P3** |
| **SEC-42** | `players_in_room_async` | Reads other sessions' locations under **per-session locks** (not world lock). | Brief stale audience for say/emote if player moved mid-command; social inconsistency, not item duping. | Accept or snapshot room roster under world lock for social commands. | **P3** |
| **SEC-43** | Logout persist failure | Failed `persist_connection_state` **restores session** in `logout` (session re-inserted). | Player may believe they quit while still bound; state not flushed. | Surface error to user; retry queue; forced disconnect policy. | **P2** |

### IRC transport, abuse & DoS

| ID | Location | Issue | Impact | Recommendation | Priority |
|----|----------|-------|--------|----------------|----------|
| **SEC-50** | `gateway/rate_limit.rs`, `irc/dispatch.rs`, `irc/bot.rs` | ~~**No rate limiting**~~ — **Resolved (July 2026):** per-nick token buckets for commands, movement, and OOC (`MUDL_RATE_LIMIT_*` env). | Residual: no per-IP or global cap; disabled when `MUDL_RATE_LIMIT_ENABLED=0` or `IRC_MOCK`. | Tune buckets for production; add connection cap (SEC-56) if needed. | — |
| **SEC-51** | `IrcBot::handle_world_ooc` | OOC broadcasts to **all connected** nicks + world channel; **OOC rate limit** at entry. | OOC spam still amplifies to every player within bucket allowance. | Tighten OOC bucket; cap message length (nick sanitization caps at 400 chars for OOC path). | **P2** |
| **SEC-52** | `repl/session.rs` `go_async` | Movement runs full **room entry pipeline**; **movement rate limit** via `Session::check_movement_rate_limit`. | Expensive `go` chains still possible within bucket (default 8/10s). | Tune `MUDL_RATE_LIMIT_MOVEMENT`; profile under load. | **P2** |
| **SEC-53** | `irc/bot.rs` `send_privmsg_lines` | Splits on `\n` before send. | Mitigates **IRC wire injection** via embedded newlines in game text. | Extend sanitization for control chars if clients misbehave. | **P3** |
| **SEC-54** | `irc/social.rs` `format_say` / `format_tell` | User `text` embedded in quotes **without escaping** (`"` in text). | Immersion break / mild social engineering, not protocol escape. | Escape quotes or use alternate delimiters. | **P3** |
| **SEC-55** | `irc/connect.rs` | TLS with **Mozilla root store**; `IRC_TLS=false` allows plaintext. | MITM on plaintext IRC exposes tokens/passwords on network. | Enforce TLS in production config; warn on plaintext startup. | **P1** |
| **SEC-56** | `SessionManager` | No cap on **connection count**. | Many simultaneous sessions increase mutex contention and memory. | Configurable `max_connections`; queue or reject. | **P2** |

### Information disclosure

| ID | Location | Issue | Impact | Recommendation | Priority |
|----|----------|-------|--------|----------------|----------|
| **SEC-60** | `irc/visibility.rs`, `command/dispatcher.rs` | ~~**General look scope**~~ — **Resolved (July 2026):** `irc_look_scope()` = `ResolveScope::RoomOnly`; passed via `PlayerDispatchOptions` to `CommandDispatcher::look`. | Cross-room intel via `look <name>` blocked on IRC; possession + room ground only. Builder `@look` / REPL builder mode still General. | Keep; audit new transports use `RoomOnly` for player look. | — |
| **SEC-61** | `irc/dispatch.rs` `dispatch_inventory` | Shows **only caller's** inventory. | Correct isolation. | Keep. | — |
| **SEC-62** | `irc/visibility.rs` `players_in_room_async` | Room audience for say/emote excludes other rooms. | Correct room boundary for IC speech (tested in `multi_user`). | Keep. | — |
| **SEC-63** | `help` / RBAC errors | Help text and denial messages reveal command set and tier names. | Low-sensitivity reconnaissance. | Accept or shorten messages for production. | **P3** |
| **SEC-64** | `tracing` / `bin/irc.rs` startup logs | Logs `database_url`, server, channel names at `info`. | Operator logs may leak paths. | Redact secrets; structured logging levels. | **P3** |

---

## Positive controls (M5 + M6 prep)

| Control | Evidence |
|---------|----------|
| Parameterized SQL | `persistence/sqlite.rs` — sqlx binds throughout |
| World-level mutation lock | `SharedWorld` + `with_locked_async` |
| Optimistic concurrency | `Object.revision` CAS + batch retry |
| Event dispatch bounds | `MAX_DISPATCH_DEPTH`, cycle detection (`dispatch_guard.rs`) |
| IRC meta execution blocked | `dispatch_meta` returns defer message after RBAC |
| Actor/player ID guard | `login` rejects non-`player:` IDs |
| Login token auth | `LoginAuthPolicy` + `verify_login` (SEC-01) |
| Rate limiting | `RateLimiter` on command/OOC/move (SEC-50) |
| Room-scoped IRC look | `irc_look_scope()` = `RoomOnly` (SEC-60) |
| Single-writer lock | `WriterGuard` advisory lock (SEC-23) |
| Nick sanitization | `irc/nick.rs` — validation, OOC control-char strip |
| Optional account-tag verify | `irc/identity.rs` — `IRC_REQUIRE_ACCOUNT_TAG`, bindings |
| Transport-neutral dispatch | `CommandDispatcher` — shared attack/drop/look/go |
| Concurrent take safety | `gateway::multi_user` XOR take test |
| TLS-by-default IRC config | `IrcConfig::default`, port 6697 |
| Co-located speech only | `players_in_room_async` + channel relay |
| IRC wire newline split | `send_privmsg_lines` splits before send (SEC-53) |

---

## Remediation roadmap (mapped to milestones)

| Priority | Finding IDs | Target milestone | Status |
|----------|-------------|------------------|--------|
| ~~**P0**~~ | ~~SEC-50, SEC-60~~ | Pre-M6 | **Done** — rate limits + `RoomOnly` look |
| ~~**P0**~~ | ~~SEC-23~~ | Pre-M6 | **Mitigated** — `WriterGuard`; ops policy |
| ~~**P0**~~ | ~~SEC-01 (open login)~~ | Pre-M6 | **Mitigated** — `LoginAuthPolicy` |
| **P1** | SEC-01 (residual), SEC-03 (residual) | **M6–M7** | Token rotation tooling; stricter `account-tag` on public nets |
| **P1** | SEC-10–SEC-12, SEC-21, SEC-32, SEC-55 | **M6–M7** | REPL RBAC align; wizard audit; graph validation |
| **P2** | SEC-04, SEC-24, SEC-33, SEC-43, SEC-51–SEC-52, SEC-56 | **M7–M9** | Ops hygiene, OOC/move tuning, connection limits |
| **P3** | SEC-05, SEC-41–SEC-42, SEC-53–SEC-54, SEC-63–SEC-64 | **M9** polish | |

---

## Operator checklist (July 2026)

For **controlled playtests** (not unattended public Internet):

1. **Single writer** — leave `MUDL_SINGLE_WRITER_ENABLED` at default (`true`); run IRC bot **or** REPL, not both against the same file DB.
2. **Login tokens** — set `MUDL_LOGIN_REQUIRE_AUTH=true` and `MUDL_LOGIN_TOKENS=player:id=secret,…`; optional `MUDL_LOGIN_IDENTITY_BINDINGS=nick=player:id` (IRC) or `U01234ABC=player:id` (Slack).
3. **Rate limits** — leave `MUDL_RATE_LIMIT_ENABLED` at default (`true`); tune `MUDL_RATE_LIMIT_COMMANDS`, `_MOVEMENT`, `_OOC` if needed.
4. **IRC identity** — for public networks, enable `IRC_REQUIRE_ACCOUNT_TAG=true` and/or `MUDL_IRC_ACCOUNT_BINDINGS`; require SASL/`+r` on the IRC network.
5. Keep **`IRC_MOCK` unset** on any shared machine.
6. Enforce **`IRC_TLS=true`**; restrict filesystem permissions on `repl.db` / `mudl.db`.
7. **No REPL on production world** while IRC is live — REPL bypasses IRC meta deferral (SEC-12) even with writer lock.

### Environment reference

| Variable | Purpose | Default (live IRC) |
|----------|---------|-------------------|
| `MUDL_LOGIN_REQUIRE_AUTH` | Require login token | `true` (false when `IRC_MOCK=1` or `SLACK_MOCK=1`) |
| `MUDL_LOGIN_TOKENS` | `player:id=token` map | unset → login denied when auth required |
| `MUDL_LOGIN_IDENTITY_BINDINGS` | `nick=player:id` or `U…=player:id` optional bind | unset |
| `MUDL_SINGLE_WRITER_ENABLED` | Advisory DB writer lock | `true` for file DBs |
| `MUDL_WRITER_MODE` | Lock metadata (`repl`/`irc`) | per binary |
| `MUDL_RATE_LIMIT_ENABLED` | Token-bucket throttling | `true` |
| `MUDL_RATE_LIMIT_COMMANDS` | burst/window_secs | `30/60` |
| `MUDL_RATE_LIMIT_MOVEMENT` | burst/window_secs | `8/10` |
| `MUDL_RATE_LIMIT_OOC` | burst/window_secs | `5/30` |
| `IRC_REQUIRE_ACCOUNT_TAG` | Reject unauthenticated PRIVMSG | `false` |
| `MUDL_IRC_ACCOUNT_BINDINGS` | `nick=AccountName` | unset |

---

*Next review: after M6 Slack transport and M7 wizard-tooling land — focus on Slack delivery attack surface, meta-command execution over transports, and REPL RBAC alignment.*