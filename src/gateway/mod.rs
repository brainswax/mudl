//! Multi-user session gateway: registry, lifecycle, RBAC, and persistence (M5).
//!
//! [`SessionManager`] is the **sole connection registry** for live transports (IRC today;
//! Slack and others in M6). It owns:
//!
//! - one [`SharedWorld`](crate::world::SharedWorld) graph hydrated from SQLite;
//! - a [`ConnectionRegistry`] mapping transport identity (IRC nick, future Slack user id)
//!   → player [`ObjectId`](crate::object::ObjectId);
//! - per-connection [`Session`](crate::repl::Session) handles (`Arc<Mutex<Session>>`).
//!
//! Transport adapters ([`IrcBot`](crate::irc::IrcBot), REPL) are thin: they parse input,
//! call `SessionManager` / `dispatch_command`, and deliver output. There is no parallel
//! `Gateway` type — use `SessionManager` for all multi-connection entry points.

mod login_auth;
mod persistence;
mod rate_limit;
mod rbac;
mod registry;
mod session_manager;

#[cfg(test)]
mod edge_cases;
#[cfg(test)]
mod load;
#[cfg(test)]
mod m5_scenarios;
#[cfg(test)]
mod m6_scenarios;
#[cfg(test)]
mod m6_multi_user;
#[cfg(test)]
mod multi_user;

pub use login_auth::{
    parse_login_args, resolve_player_by_token, resolve_player_for_login, verify_login,
    LoginAuthError, LoginAuthPolicy, LoginRequest, ParsedLoginArgs, LOGIN_TOKEN_PROPERTY,
};
pub use persistence::{hydrate_actor, persist_connection_state};
pub use rbac::{
    actor_has_tier, actor_tier, authorize_meta_command, authorize_plain_command,
    required_tier_for_meta_verb, required_tier_for_plain_command, tier_denied_message, ActorTier,
    AuthError,
};
pub use rate_limit::{
    rate_limit_kind_for_line, BucketSpec, RateLimitConfig, RateLimitContext, RateLimitDenied,
    RateLimitKind, RateLimiter,
};
pub use registry::{normalize_nick, ConnectionRegistry, RegistryError};
pub use session_manager::{LoginError, LogoutError, SessionManager};