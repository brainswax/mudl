//! IRC channel naming for world and per-room visibility.

use crate::gateway::PlayMode;
use crate::object::ObjectId;

use super::config::IrcConfig;

/// Classify an IRC target string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelTarget {
    World,
    Room(String),
    Private(String),
    Bot,
}

/// Build the per-room channel name for a place object.
pub fn room_channel_name(prefix: &str, room_id: &ObjectId) -> String {
    let slug = room_id
        .as_str()
        .split_once(':')
        .map(|(_, rest)| rest)
        .unwrap_or(room_id.as_str());
    format!("{prefix}{slug}")
}

/// Classify an IRC message target relative to the bot configuration.
pub fn classify_target(target: &str, config: &IrcConfig) -> ChannelTarget {
    let normalized = target.trim();
    if normalized.eq_ignore_ascii_case(&config.bot_nick) {
        return ChannelTarget::Bot;
    }
    if normalized.eq_ignore_ascii_case(&config.world_channel) {
        return ChannelTarget::World;
    }
    if let Some(suffix) = normalized
        .strip_prefix(&config.room_channel_prefix)
        .filter(|s| !s.is_empty())
    {
        return ChannelTarget::Room(suffix.to_string());
    }
    ChannelTarget::Private(normalized.to_string())
}

/// Notice text inviting a player IRC client to join a room channel.
pub fn room_join_notice(channel: &str) -> String {
    format!("Join {channel} to see speech and actions in your current location.")
}

/// Shared in-character channel for open-world play.
pub fn shared_ic_channel(config: &IrcConfig) -> &str {
    &config.world_channel
}

/// In-character IRC channel or presence key for speech/movement routing.
pub fn speech_channel(config: &IrcConfig, room_id: &ObjectId) -> String {
    match config.play_mode {
        PlayMode::Story => room_channel_name(&config.room_channel_prefix, room_id),
        PlayMode::Open => shared_ic_channel(config).to_string(),
    }
}

/// Login/movement notice for the active play mode.
pub fn ic_join_notice(config: &IrcConfig, room_id: &ObjectId) -> String {
    match config.play_mode {
        PlayMode::Story => room_join_notice(&room_channel_name(
            &config.room_channel_prefix,
            room_id,
        )),
        PlayMode::Open => format!(
            "Join {} for in-character speech and actions.",
            shared_ic_channel(config)
        ),
    }
}

/// Channels to join on login for the active play mode.
pub fn login_channel_joins(config: &IrcConfig, room_id: Option<&ObjectId>) -> Vec<String> {
    match config.play_mode {
        PlayMode::Story => {
            let mut joins = Vec::new();
            if !config.world_channel.is_empty() {
                joins.push(config.world_channel.clone());
            }
            if let Some(room_id) = room_id {
                joins.push(room_channel_name(&config.room_channel_prefix, room_id));
            }
            joins
        }
        PlayMode::Open => {
            if config.world_channel.is_empty() {
                Vec::new()
            } else {
                vec![config.world_channel.clone()]
            }
        }
    }
}

/// Channels to part on logout for the active play mode.
pub fn logout_channel_parts(config: &IrcConfig, room_id: Option<&ObjectId>) -> Vec<String> {
    match config.play_mode {
        PlayMode::Story => {
            let mut parts = Vec::new();
            if !config.world_channel.is_empty() {
                parts.push(config.world_channel.clone());
            }
            if let Some(room_id) = room_id {
                parts.push(room_channel_name(&config.room_channel_prefix, room_id));
            }
            parts
        }
        PlayMode::Open => {
            if config.world_channel.is_empty() {
                Vec::new()
            } else {
                vec![config.world_channel.clone()]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_channel_uses_object_slug() {
        let config = IrcConfig::default();
        let channel = room_channel_name(&config.room_channel_prefix, &ObjectId::new("room:void-001"));
        assert_eq!(channel, "#mudl-void-001");
    }

    #[test]
    fn classifies_world_room_and_bot_targets() {
        let config = IrcConfig::default();
        assert_eq!(classify_target("#mudl", &config), ChannelTarget::World);
        assert_eq!(
            classify_target("#mudl-void-001", &config),
            ChannelTarget::Room("void-001".to_string())
        );
        assert_eq!(classify_target("mudl", &config), ChannelTarget::Bot);
        assert_eq!(
            classify_target("alice", &config),
            ChannelTarget::Private("alice".to_string())
        );
    }
}