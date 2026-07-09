//! Slack bot configuration from environment variables.

use crate::gateway::{LoginAuthPolicy, OpenMovementNotices, PlayMode, RateLimitConfig};

/// Runtime settings for the MUDL Slack gateway.
#[derive(Debug, Clone, PartialEq)]
pub struct SlackConfig {
    /// Bot user OAuth token (`xoxb-…`) for Web API calls.
    pub bot_token: String,
    /// Signing secret for Events API request verification.
    pub signing_secret: String,
    /// Slack app-level ID (`A…`) — used to strip `<@APP>` mentions from commands.
    pub app_id: Option<String>,
    /// Workspace channel ID (`C…`) for out-of-character chat.
    pub world_channel: String,
    /// Optional shared channel (`C…`) for per-room **threads** (multi-channel threaded play).
    /// When set, in-character speech posts as threads here; when unset, each room uses a
    /// dedicated channel slug (`mudl-void-001`) via `conversations.join`.
    pub rooms_channel: Option<String>,
    /// Prefix for per-room channel slugs when `rooms_channel` is unset.
    pub room_channel_prefix: String,
    /// Bind address for the Events API HTTP server (e.g. `0.0.0.0:3000`).
    pub bind_addr: String,
    /// HTTP path for Slack event subscriptions.
    pub events_path: String,
    /// SQLite database URL shared with the REPL and IRC bot.
    pub database_url: String,
    /// Default player object used when bootstrapping an empty world.
    pub default_player: String,
    /// Login authentication policy (tokens, identity bindings — SEC-01).
    pub login_auth: LoginAuthPolicy,
    /// Anti-flood rate limits on command, movement, and OOC entry (SEC-50).
    pub rate_limits: RateLimitConfig,
    /// Story vs open-world visibility and channel routing.
    pub play_mode: PlayMode,
    /// Generic arrival/departure lines on movement in open mode (`MUDL_OPEN_MOVEMENT_NOTICES`).
    pub open_movement_notices: OpenMovementNotices,
}

impl Default for SlackConfig {
    fn default() -> Self {
        Self {
            bot_token: String::new(),
            signing_secret: String::new(),
            app_id: None,
            world_channel: String::new(),
            rooms_channel: None,
            room_channel_prefix: "mudl-".to_string(),
            bind_addr: "0.0.0.0:3000".to_string(),
            events_path: "/slack/events".to_string(),
            database_url: "sqlite://mudl.db".to_string(),
            default_player: "player:admin-001".to_string(),
            login_auth: LoginAuthPolicy::permissive(),
            rate_limits: RateLimitConfig::disabled(),
            play_mode: PlayMode::Story,
            open_movement_notices: OpenMovementNotices::Off,
        }
    }
}

impl SlackConfig {
    /// Load configuration from environment variables with sensible defaults.
    pub fn from_env() -> Self {
        let mut config = Self::default();
        if let Ok(token) = std::env::var("SLACK_BOT_TOKEN") {
            config.bot_token = token;
        }
        if let Ok(secret) = std::env::var("SLACK_SIGNING_SECRET") {
            config.signing_secret = secret;
        }
        if let Ok(app_id) = std::env::var("SLACK_APP_ID") {
            let trimmed = app_id.trim();
            if !trimmed.is_empty() {
                config.app_id = Some(trimmed.to_string());
            }
        }
        if let Ok(channel) = std::env::var("SLACK_WORLD_CHANNEL") {
            config.world_channel = channel.trim().to_string();
        }
        if let Ok(channel) = std::env::var("SLACK_ROOMS_CHANNEL") {
            let trimmed = channel.trim();
            if !trimmed.is_empty() {
                config.rooms_channel = Some(trimmed.to_string());
            }
        }
        if let Ok(prefix) = std::env::var("SLACK_ROOM_CHANNEL_PREFIX") {
            config.room_channel_prefix = prefix.trim().to_string();
        }
        if let Ok(addr) = std::env::var("SLACK_BIND_ADDR") {
            config.bind_addr = addr;
        }
        if let Ok(path) = std::env::var("SLACK_EVENTS_PATH") {
            let trimmed = path.trim();
            config.events_path = if trimmed.starts_with('/') {
                trimmed.to_string()
            } else {
                format!("/{trimmed}")
            };
        }
        if let Ok(db) = std::env::var("DATABASE_URL") {
            config.database_url = db;
        }
        if let Ok(player) = std::env::var("DEFAULT_PLAYER") {
            config.default_player = player;
        }
        config.login_auth = LoginAuthPolicy::from_env();
        config.rate_limits = RateLimitConfig::from_env();
        config.play_mode = PlayMode::from_env();
        config.open_movement_notices = OpenMovementNotices::from_env();
        config
    }

    /// Whether live Slack credentials are present.
    pub fn has_live_credentials(&self) -> bool {
        !self.bot_token.is_empty() && !self.signing_secret.is_empty()
    }

    /// Human-readable summary for logs.
    pub fn connection_summary(&self) -> String {
        format!(
            "events {} on {}, world_channel={}",
            self.events_path, self.bind_addr, self.world_channel
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_include_events_path() {
        let config = SlackConfig::default();
        assert_eq!(config.events_path, "/slack/events");
        assert_eq!(config.bind_addr, "0.0.0.0:3000");
    }

    #[test]
    fn has_live_credentials_requires_token_and_secret() {
        let mut config = SlackConfig::default();
        assert!(!config.has_live_credentials());
        config.bot_token = "xoxb-test".to_string();
        assert!(!config.has_live_credentials());
        config.signing_secret = "secret".to_string();
        assert!(config.has_live_credentials());
    }
}