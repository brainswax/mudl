# MUDL Security Review — Milestone 5 (Multi-user IRC)

*Review date: July 2026. Scope: M5 multi-user IRC transport, shared persistence, and command dispatch. **Documentation only** — no remediations applied in this review.*

## Scope & threat model

| Assumption | Notes |
|------------|-------|
| **Deployment** | Single-world SQLite backend; one IRC bot process; optional separate REPL process against the same `DATABASE_URL`. |
| **Adversary** | Untrusted IRC users on a public network; curious/cooperative players; abusive flooders; **not** assumed: OS root on the host (except where noted for DB file access). |
| **Trust boundary** | The IRC **server** authenticates nicks (NickServ/SASL/etc.); MUDL trusts the `from` field on inbound `PRIVMSG` lines. |
| **Out of scope** | Host hardening, TLS certificate pinning policy, Libera Chat operational security, LLM prompt injection (M10+). |

## Executive summary

M5 is **safe for trusted playtest cohorts** on a single IRC bot instance with no parallel REPL writer. The persistence layer uses **parameterized SQL**; in-process concurrency is **serialized on `SharedWorld`** with optimistic revision checks. **Player-supplied text is not evaluated as MUDL or Rust.**

**Not production-hardened** for open Internet play today. Critical gaps:

1. **No authentication** at the MUDL layer — `login` / `login player:<id>` binds any known player object to an IRC nick.
2. **Cross-room information disclosure** — IRC `look <target>` uses `ResolveScope::General`, resolving objects world-wide.
3. **Split-brain risk** — REPL and IRC are separate processes sharing one SQLite file with independent in-memory graphs.
4. **No rate limiting** — command, OOC, and event-chain floods are unbounded.
5. **Builder meta deferred but RBAC-probed** — `@` commands leak tier requirements; execution blocked, not absent from the attack surface.

---

## Findings

### Authentication & session binding

| ID | Location | Issue | Impact | Recommendation | Priority |
|----|----------|-------|--------|----------------|----------|
| **SEC-01** | `gateway/login_auth.rs`, `irc/dispatch.rs` | ~~**Passwordless login**~~ — **Mitigated (July 2026):** `LoginAuthPolicy` requires tokens on live IRC (`MUDL_LOGIN_TOKENS`, `login_token` property, optional `MUDL_LOGIN_IDENTITY_BINDINGS`). Open login remains for `IRC_MOCK` / `MUDL_LOGIN_REQUIRE_AUTH=false`. | Residual: operators must deploy tokens; binding map optional; no NickServ/SASL integration yet. | Rotate tokens; use identity bindings for public playtests; add IRC `account-tag` verification (M6). | **P1** (residual) |
| **SEC-02** | `gateway/session_manager.rs` `build_connection` | **One actor, one session** enforced (`is_actor_bound`); nick reuse blocked (`RegistryError::NickInUse`). | Mitigates duplicate-world presence for same player; does **not** stop SEC-01 initial bind. | Keep; extend with auth before bind. | — |
| **SEC-03** | `irc/message.rs`, `irc/nick.rs`, `irc/identity.rs`, `IrcBot` | **IRC nick from wire** — validated/sanitized at parse; optional `IRC_REQUIRE_ACCOUNT_TAG` + `MUDL_IRC_ACCOUNT_BINDINGS`; OOC sanitized (no embedded newlines/control chars). | Residual: MUDL does not run SASL — operators must enforce network-level nick ownership. | Deploy SASL/`+r` on IRC network + MUDL tokens; enable `IRC_REQUIRE_ACCOUNT_TAG` for public playtests. | **P1** (mitigated) |
| **SEC-04** | `bin/irc.rs` `run_mock_bot` (`IRC_MOCK=1`) | Stdin lines choose arbitrary nick + command with **no auth**. | Local dev only; if `IRC_MOCK` enabled on a shared host, full impersonation. | Refuse `IRC_MOCK` unless `RUST_ENV=development` or explicit opt-in; document in operator guide. | **P2** |
| **SEC-05** | `gateway/session_manager.rs` `reclaim_orphan_nick` | Orphan registry entries from `connect()` without `release` can be reclaimed on matching `login`. | Edge-case nick squatting after crashed test harness; low risk in production bot path. | Prefer `login` only in production; deprecate orphan `connect()` from external adapters. | **P3** |

### Authorization & privilege escalation

| ID | Location | Issue | Impact | Recommendation | Priority |
|----|----------|-------|--------|----------------|----------|
| **SEC-10** | `irc/dispatch.rs` `dispatch_meta` | Meta commands (`@set`, `@dig`, …) hit `authorize_meta_command` then return *"not enabled yet"* — **no mutation**, but error messages confirm tier (wizard vs builder). | Information leak about player object `PermissionFlags`; no direct escalation via IRC today. | When enabling meta (M7), re-check auth on **every** handler; audit log denials. | **P1** (on enable) |
| **SEC-11** | `gateway/rbac.rs`, `object/PermissionFlags` | Authorization reads **`permissions` on the player `Object`** in the world graph (JSON/SQLite). | Direct SQLite edit or REPL `@set` on permissions elevates IRC actor tier; compromised world file = full wizard. | Sign or checksum player rows (future); restrict REPL on production; separate builder DB role (M7). | **P1** |
| **SEC-12** | `bin/repl.rs` vs `gateway/rbac` | REPL uses same `authorize_meta_command` on `Session` (not stubbed `has_wizard_permission` in current tree), but REPL is a **separate process** with full command surface. | Operator with REPL access bypasses IRC meta deferral entirely. | Single writer policy: no REPL on production world file while IRC is live; or shared `SessionManager` service (M7). | **P0** (ops) |
| **SEC-13** | `irc/dispatch.rs` logged-out path | Only `login` / `help` accepted when logged out; other verbs rejected. | Good default deny. | Keep; add rate limit on failed login attempts. | — |

### SQL injection & persistence

| ID | Location | Issue | Impact | Recommendation | Priority |
|----|----------|-------|--------|----------------|----------|
| **SEC-20** | `persistence/sqlite.rs` | All queries use **sqlx `?` binds** (`save_object`, `load_object`, `save_objects_batch`, counters). | **No SQL injection** from IRC command strings or `ObjectId` values under normal code paths. | Maintain bind-only pattern; forbid dynamic SQL in future admin tools. | — |
| **SEC-21** | `persistence/sqlite.rs` `load_object` | Object graph loaded via `serde_json::from_str` on `data` column. | **Untrusted JSON** if attacker has DB write access → arbitrary `permissions`, `event_handlers`, vitals. | File permissions on `repl.db`; optional schema validation on hydrate (M7 graph validator). | **P1** |
| **SEC-22** | `world/world_state.rs` `persist_changes` | Optimistic **`revision` CAS** + retry on `RevisionConflict`. | Mitigates lost updates for concurrent saves **within one process**; `edge_cases::logout_persists_despite_revision_conflict` covers logout path. | Keep; extend monitoring for conflict storms (M9). | — |
| **SEC-23** | Architecture (REPL + IRC) | **Two processes, one SQLite file**, separate `WorldState` heaps. | **Split-brain**: REPL changes invisible to IRC until reload; concurrent saves can conflict or overwrite; no distributed lock. | **Single live writer** policy; or one combined service process; document in operator guide. | **P0** (ops) |
| **SEC-24** | `IrcConfig::database_url` | Default `sqlite://mudl.db` — world state at rest **unencrypted**. | Host filesystem compromise exposes full world + player permissions. | Filesystem ACLs; encrypted volume; future SQLCipher if threat model requires. | **P2** |

### MUDL / code injection (player input)

| ID | Location | Issue | Impact | Recommendation | Priority |
|----|----------|-------|--------|----------------|----------|
| **SEC-30** | `command/parse.rs` | Player input split into verb/args; **no eval** of user text as MUDL. | Player chat cannot upload scripts at parse time. | Keep; centralize in `CommandDispatcher` (M6). | — |
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
| **SEC-50** | `IrcBot::handle_input` / `dispatch_command` | **No rate limiting** per nick, IP, or global. | Flood of `go`, `look`, OOC, or `say` can CPU-spin event chains and SQLite writes. | Token bucket on dispatcher entry (M9); OOC throttle (M6 if Slack exposed). | **P0** |
| **SEC-51** | `IrcBot::handle_world_ooc` | OOC broadcasts to **all connected** nicks + world channel; requires login only. | OOC spam amplifies to every player. | Per-nick OOC cooldown; cap message length. | **P1** |
| **SEC-52** | `repl/session.rs` `go_async` | Movement runs full **room entry pipeline** (spawners, `@trigger`, behaviors, conditions). | Repeated `go` / exit aliases = expensive graph + event work per command. | Rate limit movement; cheap `look` path already lighter. | **P1** |
| **SEC-53** | `irc/bot.rs` `send_privmsg_lines` | Splits on `\n` before send. | Mitigates **IRC wire injection** via embedded newlines in game text. | Extend sanitization for control chars if clients misbehave. | **P3** |
| **SEC-54** | `irc/social.rs` `format_say` / `format_tell` | User `text` embedded in quotes **without escaping** (`"` in text). | Immersion break / mild social engineering, not protocol escape. | Escape quotes or use alternate delimiters. | **P3** |
| **SEC-55** | `irc/connect.rs` | TLS with **Mozilla root store**; `IRC_TLS=false` allows plaintext. | MITM on plaintext IRC exposes tokens/passwords on network. | Enforce TLS in production config; warn on plaintext startup. | **P1** |
| **SEC-56** | `SessionManager` | No cap on **connection count**. | Many simultaneous sessions increase mutex contention and memory. | Configurable `max_connections`; queue or reject. | **P2** |

### Information disclosure

| ID | Location | Issue | Impact | Recommendation | Priority |
|----|----------|-------|--------|----------------|----------|
| **SEC-60** | `irc/dispatch.rs` `dispatch_look` | Named targets use `ResolveScope::General` + `ensure_object` (loads from DB into graph). | **`look sword` / `look player:hero-002` can resolve objects anywhere** in the world if name/ID is known. | Use `ResolveScope::RoomOnly` or `PossessionOrRoom` for IRC; builder `@look` stays General. | **P0** |
| **SEC-61** | `irc/dispatch.rs` `dispatch_inventory` | Shows **only caller's** inventory. | Correct isolation. | Keep. | — |
| **SEC-62** | `irc/visibility.rs` `players_in_room_async` | Room audience for say/emote excludes other rooms. | Correct room boundary for IC speech (tested in `multi_user`). | Keep. | — |
| **SEC-63** | `help` / RBAC errors | Help text and denial messages reveal command set and tier names. | Low-sensitivity reconnaissance. | Accept or shorten messages for production. | **P3** |
| **SEC-64** | `tracing` / `bin/irc.rs` startup logs | Logs `database_url`, server, channel names at `info`. | Operator logs may leak paths. | Redact secrets; structured logging levels. | **P3** |

---

## Positive controls (M5)

| Control | Evidence |
|---------|----------|
| Parameterized SQL | `persistence/sqlite.rs` — sqlx binds throughout |
| World-level mutation lock | `SharedWorld` + `with_locked_async` |
| Optimistic concurrency | `Object.revision` CAS + batch retry |
| Event dispatch bounds | `MAX_DISPATCH_DEPTH`, cycle detection (`dispatch_guard.rs`) |
| IRC meta execution blocked | `dispatch_meta` returns defer message after RBAC |
| Actor/player ID guard | `login` rejects non-`player:` IDs |
| Concurrent take safety | `gateway::multi_user` XOR take test |
| TLS-by-default IRC config | `IrcConfig::default`, port 6697 |
| Co-located speech only | `players_in_room_async` + channel relay |

---

## Remediation roadmap (mapped to milestones)

| Priority | Finding IDs | Target milestone |
|----------|-------------|------------------|
| **P0** | SEC-23, SEC-50, SEC-60 | **Pre-M6 / M6** — single-writer ops, rate limits, IRC look scope |
| **P1** | SEC-01 (residual), SEC-03 | **M6** — NickServ/account-tag; token rotation tooling |
| **P1** | SEC-03, SEC-10–SEC-12, SEC-21, SEC-32, SEC-51–SEC-52, SEC-55 | **M6–M7** — transport hardening, wizard audit, validation |
| **P2** | SEC-04, SEC-24, SEC-33, SEC-43, SEC-56 | **M7–M9** — ops hygiene, script caps, connection limits |
| **P3** | SEC-05, SEC-41–SEC-43, SEC-53–SEC-54, SEC-63–SEC-64 | **M9** polish |

---

## Operator checklist (interim)

Until P0 items are addressed:

1. Run **only one live writer** (IRC bot **or** REPL, not both) against `DATABASE_URL`.
2. Require **registered IRC nicks** (network-level) for playtests.
3. Use an **allowlist** of player object IDs or pre-provisioned accounts — treat `login player:admin-001` as a secret.
4. Keep **`IRC_MOCK` unset** on any shared machine.
5. Enforce **`IRC_TLS=true`**; restrict filesystem permissions on `repl.db` / `mudl.db`.
6. Do not expose the bot to **open Internet** without rate limiting (SEC-50).

---

*Next review: after M6 Slack transport and M7 wizard-tooling land — focus on auth binding, `CommandDispatcher` attack surface, and meta-command execution over transports.*