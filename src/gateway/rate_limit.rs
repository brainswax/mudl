//! Per-identity token-bucket rate limiting for transport entry points (SEC-50).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::registry::normalize_nick;

/// Kind of action subject to independent rate limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RateLimitKind {
    /// General commands: look, say, take, login, help, meta, etc.
    Command,
    /// Movement: `go`, exit aliases, and `Session::go_async`.
    Movement,
    /// Out-of-character chat on world / Slack channels.
    Ooc,
}

/// Token-bucket parameters for one limit class.
#[derive(Debug, Clone, PartialEq)]
pub struct BucketSpec {
    /// Maximum burst (bucket capacity).
    pub burst: f64,
    /// Seconds to fully refill from empty to burst.
    pub window_secs: f64,
}

impl BucketSpec {
    pub fn new(burst: u32, window_secs: f64) -> Self {
        Self {
            burst: burst as f64,
            window_secs: window_secs.max(0.001),
        }
    }

    fn refill_per_sec(&self) -> f64 {
        self.burst / self.window_secs
    }
}

/// Configurable limits for command, movement, and OOC floods.
#[derive(Debug, Clone, PartialEq)]
pub struct RateLimitConfig {
    pub enabled: bool,
    pub commands: BucketSpec,
    pub movement: BucketSpec,
    pub ooc: BucketSpec,
}

impl RateLimitConfig {
    /// No throttling — used by unit tests and local REPL.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            commands: BucketSpec::new(30, 60.0),
            movement: BucketSpec::new(8, 10.0),
            ooc: BucketSpec::new(5, 30.0),
        }
    }

    /// Production-oriented defaults when rate limiting is enabled.
    pub fn production_defaults() -> Self {
        Self {
            enabled: true,
            commands: BucketSpec::new(30, 60.0),
            movement: BucketSpec::new(8, 10.0),
            ooc: BucketSpec::new(5, 30.0),
        }
    }

    /// Load from environment (IRC / future Slack transports).
    ///
    /// `MUDL_RATE_LIMIT_ENABLED` — default `true` when unset (set `0` to disable).
    /// `MUDL_RATE_LIMIT_COMMANDS` — `burst/window_secs` (default `30/60`).
    /// `MUDL_RATE_LIMIT_MOVEMENT` — default `8/10`.
    /// `MUDL_RATE_LIMIT_OOC` — default `5/30`.
    pub fn from_env() -> Self {
        let enabled = match std::env::var("MUDL_RATE_LIMIT_ENABLED") {
            Ok(raw) => parse_bool_env(&raw, true),
            Err(_) => true,
        };
        let mut config = Self::production_defaults();
        config.enabled = enabled;
        if let Ok(raw) = std::env::var("MUDL_RATE_LIMIT_COMMANDS") {
            config.commands = parse_bucket_spec(&raw, 30, 60.0);
        }
        if let Ok(raw) = std::env::var("MUDL_RATE_LIMIT_MOVEMENT") {
            config.movement = parse_bucket_spec(&raw, 8, 10.0);
        }
        if let Ok(raw) = std::env::var("MUDL_RATE_LIMIT_OOC") {
            config.ooc = parse_bucket_spec(&raw, 5, 30.0);
        }
        config
    }

    pub fn spec(&self, kind: RateLimitKind) -> &BucketSpec {
        match kind {
            RateLimitKind::Command => &self.commands,
            RateLimitKind::Movement => &self.movement,
            RateLimitKind::Ooc => &self.ooc,
        }
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self::disabled()
    }
}

/// Handle shared between [`SessionManager`](super::SessionManager) and per-connection [`Session`](crate::repl::Session).
#[derive(Debug, Clone)]
pub struct RateLimitContext {
    pub limiter: Arc<Mutex<RateLimiter>>,
    pub identity: String,
}

impl RateLimitContext {
    pub fn check(&self, kind: RateLimitKind) -> Result<(), RateLimitDenied> {
        let mut guard = self
            .limiter
            .lock()
            .map_err(|_| RateLimitDenied { kind })?;
        guard.check(&self.identity, kind)
    }
}

/// Per-transport-identity token buckets (IRC nick, Slack user id, etc.).
#[derive(Debug)]
pub struct RateLimiter {
    config: RateLimitConfig,
    identities: HashMap<String, IdentityBuckets>,
}

#[derive(Debug)]
struct IdentityBuckets {
    command: TokenBucket,
    movement: TokenBucket,
    ooc: TokenBucket,
}

#[derive(Debug)]
struct TokenBucket {
    capacity: f64,
    tokens: f64,
    refill_per_sec: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn from_spec(spec: &BucketSpec) -> Self {
        Self {
            capacity: spec.burst,
            tokens: spec.burst,
            refill_per_sec: spec.refill_per_sec(),
            last_refill: Instant::now(),
        }
    }

    fn try_consume(&mut self) -> bool {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        if elapsed > 0.0 {
            self.tokens = (self.tokens + elapsed * self.refill_per_sec).min(self.capacity);
            self.last_refill = now;
        }
    }
}

impl IdentityBuckets {
    fn from_config(config: &RateLimitConfig) -> Self {
        Self {
            command: TokenBucket::from_spec(&config.commands),
            movement: TokenBucket::from_spec(&config.movement),
            ooc: TokenBucket::from_spec(&config.ooc),
        }
    }

    fn bucket_mut(&mut self, kind: RateLimitKind) -> &mut TokenBucket {
        match kind {
            RateLimitKind::Command => &mut self.command,
            RateLimitKind::Movement => &mut self.movement,
            RateLimitKind::Ooc => &mut self.ooc,
        }
    }
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            identities: HashMap::new(),
        }
    }

    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }

    /// Attempt to consume one token for `identity` and `kind`.
    pub fn check(&mut self, identity: &str, kind: RateLimitKind) -> Result<(), RateLimitDenied> {
        if !self.config.enabled {
            return Ok(());
        }
        let key = normalize_nick(identity);
        let buckets = self
            .identities
            .entry(key)
            .or_insert_with(|| IdentityBuckets::from_config(&self.config));
        if buckets.bucket_mut(kind).try_consume() {
            Ok(())
        } else {
            Err(RateLimitDenied { kind })
        }
    }

    /// Drop per-identity state when a transport connection ends.
    pub fn forget_identity(&mut self, identity: &str) {
        self.identities.remove(&normalize_nick(identity));
    }

    pub fn denial_message(&self, kind: RateLimitKind) -> String {
        match kind {
            RateLimitKind::Command => {
                "You are sending commands too quickly. Please wait a moment.".to_string()
            }
            RateLimitKind::Movement => {
                "You're moving too quickly. Slow down.".to_string()
            }
            RateLimitKind::Ooc => {
                "Please wait before sending another out-of-character message.".to_string()
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitDenied {
    pub kind: RateLimitKind,
}

impl std::fmt::Display for RateLimitDenied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "rate limit exceeded ({:?})", self.kind)
    }
}

impl std::error::Error for RateLimitDenied {}

/// Classify a parsed command line for rate-limit bucket selection.
pub fn rate_limit_kind_for_line(line: &crate::command::CommandLine) -> RateLimitKind {
    if line.is_meta {
        return RateLimitKind::Command;
    }
    match line.verb.as_str() {
        "go" => RateLimitKind::Movement,
        "help" | "?" | "login" | "quit" | "logout" | "exit" | "look" | "l" | "inventory" | "i"
        | "say" | "'" | "emote" | ":" | "tell" | "whisper" | "take" | "get" | "drop" | "open"
        | "close" | "attack" => {
            RateLimitKind::Command
        }
        _ => RateLimitKind::Movement,
    }
}

fn parse_bool_env(raw: &str, default: bool) -> bool {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}

fn parse_bucket_spec(raw: &str, default_burst: u32, default_window: f64) -> BucketSpec {
    let trimmed = raw.trim();
    if let Some((burst, window)) = trimmed.split_once('/') {
        let burst = burst.trim().parse().unwrap_or(default_burst);
        let window = window.trim().parse().unwrap_or(default_window);
        BucketSpec::new(burst, window)
    } else if let Ok(burst) = trimmed.parse() {
        BucketSpec::new(burst, default_window)
    } else {
        BucketSpec::new(default_burst, default_window)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::parse_command_line;

    fn tight_config() -> RateLimitConfig {
        RateLimitConfig {
            enabled: true,
            commands: BucketSpec::new(2, 60.0),
            movement: BucketSpec::new(1, 10.0),
            ooc: BucketSpec::new(1, 30.0),
        }
    }

    #[test]
    fn disabled_config_allows_unlimited() {
        let mut limiter = RateLimiter::new(RateLimitConfig::disabled());
        for _ in 0..100 {
            assert!(limiter.check("alice", RateLimitKind::Command).is_ok());
        }
    }

    #[test]
    fn command_bucket_enforces_burst() {
        let mut limiter = RateLimiter::new(tight_config());
        assert!(limiter.check("alice", RateLimitKind::Command).is_ok());
        assert!(limiter.check("alice", RateLimitKind::Command).is_ok());
        assert!(limiter.check("alice", RateLimitKind::Command).is_err());
    }

    #[test]
    fn movement_and_command_buckets_are_independent() {
        let mut limiter = RateLimiter::new(tight_config());
        assert!(limiter.check("alice", RateLimitKind::Movement).is_ok());
        assert!(limiter.check("alice", RateLimitKind::Movement).is_err());
        assert!(limiter.check("alice", RateLimitKind::Command).is_ok());
    }

    #[test]
    fn identities_are_isolated() {
        let mut limiter = RateLimiter::new(tight_config());
        assert!(limiter.check("alice", RateLimitKind::Command).is_ok());
        assert!(limiter.check("alice", RateLimitKind::Command).is_ok());
        assert!(limiter.check("bob", RateLimitKind::Command).is_ok());
    }

    #[test]
    fn forget_identity_clears_state() {
        let mut limiter = RateLimiter::new(tight_config());
        assert!(limiter.check("alice", RateLimitKind::Command).is_ok());
        assert!(limiter.check("alice", RateLimitKind::Command).is_ok());
        assert!(limiter.check("alice", RateLimitKind::Command).is_err());
        limiter.forget_identity("alice");
        assert!(limiter.check("alice", RateLimitKind::Command).is_ok());
    }

    #[test]
    fn rate_limit_kind_for_line_classifies_movement_aliases() {
        assert_eq!(
            rate_limit_kind_for_line(&parse_command_line("north")),
            RateLimitKind::Movement
        );
        assert_eq!(
            rate_limit_kind_for_line(&parse_command_line("go north")),
            RateLimitKind::Movement
        );
        assert_eq!(
            rate_limit_kind_for_line(&parse_command_line("look")),
            RateLimitKind::Command
        );
        assert_eq!(
            rate_limit_kind_for_line(&parse_command_line("@examine self")),
            RateLimitKind::Command
        );
    }

    #[test]
    fn parse_bucket_spec_accepts_burst_and_window() {
        let spec = parse_bucket_spec("12/5", 1, 1.0);
        assert_eq!(spec.burst, 12.0);
        assert_eq!(spec.window_secs, 5.0);
    }

    #[test]
    fn denial_messages_are_player_facing() {
        let limiter = RateLimiter::new(RateLimitConfig::production_defaults());
        assert!(limiter.denial_message(RateLimitKind::Ooc).contains("out-of-character"));
    }
}