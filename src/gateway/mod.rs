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

mod auto_login;
mod login_auth;
mod open_delivery;
mod persistence;
mod play_mode;
mod rate_limit;
mod rbac;
mod registry;
mod registration;
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

pub use auto_login::attempt_auto_login;
pub use login_auth::{
    parse_login_args, player_has_login_secret, resolve_player_by_token,
    resolve_player_for_auto_login, resolve_player_for_login, verify_identity_binding,
    verify_login, LoginAuthError, LoginAuthPolicy, LoginRequest, ParsedLoginArgs,
    LOGIN_TOKEN_PROPERTY,
};
pub use open_delivery::{
    actor_place_context, format_open_chat, format_open_context_post, is_open_channel_command,
    is_open_private_actor_line, open_channel_broadcast_body, transport_look_scope,
};
pub use persistence::{hydrate_actor, persist_connection_state};
pub use play_mode::PlayMode;
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
pub use crate::object::{
    display_name_from_login_name, display_name_from_player_id, find_player_by_login_name,
    normalize_player_login_name, player_id_for_login_name, player_id_login_slug, player_login_name,
    player_login_name_matches, LOGIN_NAME_PROPERTY,
};
pub use registration::{
    default_spawn_location, ensure_bootstrap_wizard, has_wizard, normalize_player_display_name,
    registrations_allowed, RegisterError, MAX_PLAYER_DISPLAY_NAME_LEN,
};
pub use session_manager::{LoginError, LogoutError, SessionManager};