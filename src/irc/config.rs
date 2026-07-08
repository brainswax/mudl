//! IRC bot configuration from environment variables.

use crate::gateway::{LoginAuthPolicy, RateLimitConfig};

/// Runtime settings for the MUDL IRC gateway.
///
/// Defaults assume an **IRCv3-capable server over TLS** (port 6697).
#[derive(Debug, Clone, PartialEq)]
pub struct IrcConfig {
    /// IRC server hostname (used for TLS SNI and TCP connect).
    pub server: String,
    /// IRC server port (`6697` for TLS, `6667` for plaintext).
    pub port: u16,
    /// Connect with TLS (recommended; required by most public networks).
    pub tls: bool,
    /// Negotiate IRCv3 capabilities during registration (`CAP LS 302`).
    pub ircv3: bool,
    /// Bot nickname on the IRC network.
    pub bot_nick: String,
    /// `USER` realname gecos field.
    pub realname: String,
    /// Global out-of-character channel (e.g. `#mudl`).
    pub world_channel: String,
    /// Prefix for per-room channels (`#mudl-void-001` when prefix is `#mudl-`).
    pub room_channel_prefix: String,
    /// SQLite database URL shared with the REPL.
    pub database_url: String,
    /// Default player object used when bootstrapping an empty world.
    pub default_player: String,
    /// Login authentication policy (tokens, identity bindings — SEC-01).
    pub login_auth: LoginAuthPolicy,
    /// Anti-flood rate limits on command, movement, and OOC entry (SEC-50).
    pub rate_limits: RateLimitConfig,
}

impl Default for IrcConfig {
    fn default() -> Self {
        Self {
            server: "irc.libera.chat".to_string(),
            port: 6697,
            tls: true,
            ircv3: true,
            bot_nick: "mudl".to_string(),
            realname: "MUDL Bot".to_string(),
            world_channel: "#mudl".to_string(),
            room_channel_prefix: "#mudl-".to_string(),
            database_url: "sqlite://mudl.db".to_string(),
            default_player: "player:admin-001".to_string(),
            login_auth: LoginAuthPolicy::permissive(),
            rate_limits: RateLimitConfig::disabled(),
        }
    }
}

impl IrcConfig {
    /// Load configuration from environment variables with sensible defaults.
    pub fn from_env() -> Self {
        let mut config = Self::default();
        if let Ok(server) = std::env::var("IRC_SERVER") {
            config.server = server;
        }
        if let Ok(port) = std::env::var("IRC_PORT") {
            if let Ok(parsed) = port.parse() {
                config.port = parsed;
            }
        }
        if let Ok(raw) = std::env::var("IRC_TLS") {
            config.tls = parse_bool(&raw, true);
        }
        if let Ok(raw) = std::env::var("IRC_IRCV3") {
            config.ircv3 = parse_bool(&raw, true);
        }
        if let Ok(nick) = std::env::var("IRC_BOT_NICK") {
            config.bot_nick = nick;
        }
        if let Ok(realname) = std::env::var("IRC_REALNAME") {
            config.realname = realname;
        }
        if let Ok(channel) = std::env::var("IRC_WORLD_CHANNEL") {
            config.world_channel = normalize_channel_name(&channel);
        }
        if let Ok(prefix) = std::env::var("IRC_ROOM_CHANNEL_PREFIX") {
            config.room_channel_prefix = normalize_channel_prefix(&prefix);
        }
        if let Ok(db) = std::env::var("DATABASE_URL") {
            config.database_url = db;
        }
        if let Ok(player) = std::env::var("DEFAULT_PLAYER") {
            config.default_player = player;
        }
        config.login_auth = LoginAuthPolicy::from_env();
        config.rate_limits = RateLimitConfig::from_env();
        config
    }

    /// Human-readable connection summary for logs.
    pub fn connection_summary(&self) -> String {
        let security = if self.tls { "tls" } else { "plain" };
        let caps = if self.ircv3 { "ircv3" } else { "legacy" };
        format!(
            "{}:{} ({security}, {caps})",
            self.server, self.port
        )
    }
}

fn parse_bool(raw: &str, default: bool) -> bool {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}

fn normalize_channel_name(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with('#') {
        trimmed.to_string()
    } else {
        format!("#{trimmed}")
    }
}

fn normalize_channel_prefix(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with('#') {
        trimmed.to_string()
    } else {
        format!("#{trimmed}-")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_assume_tls_and_ircv3() {
        let config = IrcConfig::default();
        assert_eq!(config.port, 6697);
        assert!(config.tls);
        assert!(config.ircv3);
    }

    #[test]
    fn normalizes_channel_names() {
        assert_eq!(normalize_channel_name("mudl"), "#mudl");
        assert_eq!(normalize_channel_name("#play"), "#play");
        assert_eq!(normalize_channel_prefix("mudl"), "#mudl-");
    }

    #[test]
    fn parse_bool_env_values() {
        assert!(parse_bool("true", false));
        assert!(!parse_bool("0", true));
        assert!(parse_bool("garbage", true));
    }

    #[test]
    fn connection_summary_includes_tls_and_ircv3() {
        let config = IrcConfig::default();
        assert!(config.connection_summary().contains("tls"));
        assert!(config.connection_summary().contains("ircv3"));
    }
}