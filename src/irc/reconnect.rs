//! IRC reconnect policy and exponential backoff.

use std::time::Duration;

/// Settings for automatic IRC transport reconnect after disconnect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrcReconnectConfig {
    /// When true, retry connect/read failures with backoff instead of exiting.
    pub enabled: bool,
    /// First reconnect delay in seconds (doubled on each attempt until capped).
    pub initial_secs: u64,
    /// Maximum delay between reconnect attempts in seconds.
    pub max_secs: u64,
    /// Max reconnect attempts after a registered session ends (`0` = unlimited).
    pub max_attempts: u32,
}

impl Default for IrcReconnectConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            initial_secs: 5,
            max_secs: 300,
            max_attempts: 0,
        }
    }
}

impl IrcReconnectConfig {
    pub fn from_env() -> Self {
        let mut config = Self::default();
        if let Ok(raw) = std::env::var("IRC_RECONNECT") {
            config.enabled = parse_bool(&raw, true);
        }
        config.initial_secs =
            parse_u64_env(std::env::var("IRC_RECONNECT_INITIAL_SECS").ok().as_deref(), 5);
        config.max_secs =
            parse_u64_env(std::env::var("IRC_RECONNECT_MAX_SECS").ok().as_deref(), 300);
        if let Ok(raw) = std::env::var("IRC_RECONNECT_MAX_ATTEMPTS") {
            if let Ok(parsed) = raw.trim().parse::<u32>() {
                config.max_attempts = parsed;
            }
        }
        if config.max_secs < config.initial_secs {
            config.max_secs = config.initial_secs;
        }
        config
    }

    pub fn should_retry(&self, attempts_after_failure: u32) -> bool {
        if !self.enabled {
            return false;
        }
        if self.max_attempts == 0 {
            return true;
        }
        attempts_after_failure < self.max_attempts
    }
}

/// Exponential backoff for reconnect delays. Reset after a successful registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExponentialBackoff {
    initial: Duration,
    max: Duration,
    attempt: u32,
}

impl ExponentialBackoff {
    pub fn new(config: &IrcReconnectConfig) -> Self {
        Self {
            initial: Duration::from_secs(config.initial_secs),
            max: Duration::from_secs(config.max_secs),
            attempt: 0,
        }
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// Next delay before a reconnect attempt (also increments the attempt counter).
    pub fn next_delay(&mut self) -> Duration {
        let multiplier = 1u32 << self.attempt.min(31);
        let delay = self.initial.saturating_mul(multiplier);
        self.attempt = self.attempt.saturating_add(1);
        delay.min(self.max)
    }
}

fn parse_u64_env(raw: Option<&str>, default: u64) -> u64 {
    raw.and_then(|s| s.trim().parse().ok())
        .filter(|&v| v > 0)
        .unwrap_or(default)
}

fn parse_bool(raw: &str, default: bool) -> bool {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_reconnect_is_enabled_with_sane_backoff() {
        let config = IrcReconnectConfig::default();
        assert!(config.enabled);
        assert_eq!(config.initial_secs, 5);
        assert_eq!(config.max_secs, 300);
        assert_eq!(config.max_attempts, 0);
    }

    #[test]
    fn backoff_doubles_until_max() {
        let config = IrcReconnectConfig {
            initial_secs: 2,
            max_secs: 10,
            ..IrcReconnectConfig::default()
        };
        let mut backoff = ExponentialBackoff::new(&config);
        assert_eq!(backoff.next_delay(), Duration::from_secs(2));
        assert_eq!(backoff.next_delay(), Duration::from_secs(4));
        assert_eq!(backoff.next_delay(), Duration::from_secs(8));
        assert_eq!(backoff.next_delay(), Duration::from_secs(10));
        assert_eq!(backoff.next_delay(), Duration::from_secs(10));
    }

    #[test]
    fn backoff_resets_after_success() {
        let config = IrcReconnectConfig::default();
        let mut backoff = ExponentialBackoff::new(&config);
        assert_eq!(backoff.next_delay(), Duration::from_secs(5));
        backoff.reset();
        assert_eq!(backoff.next_delay(), Duration::from_secs(5));
    }

    #[test]
    fn max_attempts_zero_means_unlimited() {
        let config = IrcReconnectConfig {
            max_attempts: 0,
            ..IrcReconnectConfig::default()
        };
        assert!(config.should_retry(999));
    }

    #[test]
    fn max_attempts_limits_retries() {
        let config = IrcReconnectConfig {
            max_attempts: 3,
            ..IrcReconnectConfig::default()
        };
        assert!(config.should_retry(0));
        assert!(config.should_retry(2));
        assert!(!config.should_retry(3));
    }
}