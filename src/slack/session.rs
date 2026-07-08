//! Per-Slack-user session metadata alongside [`SessionManager`](crate::gateway::SessionManager).
//!
//! [`SessionManager`] maps transport identity → player actor and owns the game
//! [`Session`](crate::repl::Session). This module tracks Slack-specific delivery
//! context (DM conversation id) keyed by normalized Slack user id (`U…` / `W…`).

use std::collections::HashMap;

use crate::gateway::{normalize_nick, LoginAuthPolicy};

/// Normalize a Slack member id for registry lookup (case-insensitive, trimmed).
///
/// Uses the same keying as [`SessionManager`](crate::gateway::SessionManager) and
/// [`MUDL_LOGIN_IDENTITY_BINDINGS`](crate::gateway::LoginAuthPolicy::identity_bindings)
/// so `U01234ABC` and `u01234abc` resolve to one connection.
pub fn normalize_slack_user_id(user_id: &str) -> String {
    normalize_nick(user_id)
}

/// Whether `user_id` looks like a Slack member id (`U…` / `W…`).
pub fn is_slack_member_id(user_id: &str) -> bool {
    let trimmed = user_id.trim();
    trimmed.len() > 1
        && (trimmed.starts_with('U') || trimmed.starts_with('W'))
        && trimmed[1..].chars().all(|c| c.is_ascii_alphanumeric())
}

/// Logged-out help tailored for Slack DMs (not IRC nicks).
pub fn slack_logged_out_help(policy: &LoginAuthPolicy) -> String {
    if policy.require_auth {
        "Send 'login <token>' or 'login <player-id> <token>' in this DM. \
         Your operator can bind your Slack user id via MUDL_LOGIN_IDENTITY_BINDINGS."
            .to_string()
    } else {
        "Send 'login' to bind this Slack account to a matching player name, \
         or 'login <player-id>'."
            .to_string()
    }
}

/// Slack delivery context for one connected workspace member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackSessionContext {
    /// Normalized Slack user id — registry key in [`SessionManager`].
    pub user_id: String,
    /// DM conversation id (`D…`) where command responses are posted.
    pub reply_channel: String,
}

/// Sidecar registry: normalized user id → DM channel for outbound delivery.
#[derive(Debug, Default, Clone)]
pub struct SlackSessionRegistry {
    reply_channels: HashMap<String, String>,
}

impl SlackSessionRegistry {
    pub fn record(&mut self, user_id: &str, reply_channel: &str) {
        let key = normalize_slack_user_id(user_id);
        let channel = reply_channel.trim();
        if key.is_empty() || channel.is_empty() {
            return;
        }
        self.reply_channels.insert(key, channel.to_string());
    }

    pub fn remove(&mut self, user_id: &str) {
        self.reply_channels.remove(&normalize_slack_user_id(user_id));
    }

    pub fn reply_channel(&self, user_id: &str) -> Option<&str> {
        self.reply_channels
            .get(&normalize_slack_user_id(user_id))
            .map(|s| s.as_str())
    }

    /// Best recipient for `send_direct`: stored DM id, else the user id (`U…` opens a DM).
    pub fn delivery_target(&self, user_id: &str) -> String {
        self.reply_channel(user_id)
            .map(|c| c.to_string())
            .unwrap_or_else(|| user_id.trim().to_string())
    }

    pub fn context(&self, user_id: &str) -> Option<SlackSessionContext> {
        let key = normalize_slack_user_id(user_id);
        self.reply_channels.get(&key).map(|reply_channel| SlackSessionContext {
            user_id: key,
            reply_channel: reply_channel.clone(),
        })
    }

    pub fn len(&self) -> usize {
        self.reply_channels.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::gateway::{verify_login, LoginRequest};
    use crate::object::{Object, ObjectId, PermissionFlags};

    fn player(id: &str, name: &str) -> Object {
        Object {
            id: ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new(id),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        }
    }

    #[test]
    fn normalize_slack_user_id_is_case_insensitive() {
        assert_eq!(normalize_slack_user_id("U01234ABC"), "u01234abc");
        assert_eq!(normalize_slack_user_id("  alice  "), "alice");
    }

    #[test]
    fn is_slack_member_id_detects_workspace_ids() {
        assert!(is_slack_member_id("U023BECGF2M"));
        assert!(is_slack_member_id("W01234567"));
        assert!(!is_slack_member_id("alice"));
        assert!(!is_slack_member_id("D_DM"));
    }

    #[test]
    fn registry_records_and_clears_reply_channel() {
        let mut registry = SlackSessionRegistry::default();
        registry.record("U_ALICE", "D_DM1");
        assert_eq!(registry.reply_channel("u_alice"), Some("D_DM1"));
        assert_eq!(registry.delivery_target("U_ALICE"), "D_DM1");
        registry.remove("U_ALICE");
        assert!(registry.reply_channel("U_ALICE").is_none());
        assert_eq!(registry.delivery_target("U_ALICE"), "U_ALICE");
    }

    #[test]
    fn slack_logged_out_help_mentions_dm_not_nick() {
        let open = LoginAuthPolicy::permissive();
        assert!(slack_logged_out_help(&open).contains("Slack account"));
        let secured = LoginAuthPolicy {
            require_auth: true,
            ..LoginAuthPolicy::permissive()
        };
        assert!(slack_logged_out_help(&secured).contains("IDENTITY_BINDINGS"));
    }

    #[test]
    fn verify_login_accepts_slack_user_id_binding() {
        let policy = LoginAuthPolicy {
            require_auth: true,
            identity_bindings: HashMap::from([(
                "u01234abc".to_string(),
                ObjectId::new("player:hero-001"),
            )]),
            env_tokens: HashMap::from([(
                "player:hero-001".to_string(),
                "sekrit".to_string(),
            )]),
            ..LoginAuthPolicy::permissive()
        };
        let hero = player("player:hero-001", "Alice");
        let ok = verify_login(
            &policy,
            LoginRequest {
                transport: "slack",
                identity: "U01234ABC",
                player_id: &ObjectId::new("player:hero-001"),
                token: Some("sekrit"),
                player: &hero,
            },
        );
        assert!(ok.is_ok());
    }
}