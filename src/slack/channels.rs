//! Slack channel naming for world and per-room visibility.

use crate::gateway::PlayMode;
use crate::object::ObjectId;

use super::config::SlackConfig;

/// Classify a Slack channel relative to bot configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelTarget {
    World,
    Room(String),
    DirectMessage,
    Other,
}

/// Build the per-room channel slug for a place object (no leading `#`).
///
/// Mirrors IRC [`room_channel_name`](crate::irc::channels::room_channel_name) but
/// returns a Slack-friendly name such as `mudl-void-001`.
pub fn room_channel_name(prefix: &str, room_id: &ObjectId) -> String {
    let slug = room_id
        .as_str()
        .split_once(':')
        .map(|(_, rest)| rest)
        .unwrap_or(room_id.as_str());
    format!("{prefix}{slug}")
}

/// Thread presence key for in-character speech in a shared rooms channel.
pub fn room_thread_presence(rooms_channel_id: &str, room_id: &ObjectId) -> String {
    let slug = room_id
        .as_str()
        .split_once(':')
        .map(|(_, rest)| rest)
        .unwrap_or(room_id.as_str());
    super::presence::encode_thread(rooms_channel_id, &format!("room-{slug}"))
}

/// Classify a channel id/type relative to configuration.
pub fn classify_channel(
    channel_id: &str,
    channel_type: Option<&str>,
    config: &SlackConfig,
) -> ChannelTarget {
    if channel_type == Some("im") {
        return ChannelTarget::DirectMessage;
    }
    if !config.world_channel.is_empty() && channel_id == config.world_channel {
        return ChannelTarget::World;
    }
    if channel_type == Some("channel") || channel_type == Some("group") {
        let prefix = &config.room_channel_prefix;
        // Room channels are joined by slug; classification deferred to name lookup in dispatch.
        if !prefix.is_empty() && channel_id.starts_with('C') {
            return ChannelTarget::Room(channel_id.to_string());
        }
        return ChannelTarget::Room(channel_id.to_string());
    }
    ChannelTarget::Other
}

/// Whether in-character routing uses threads in a shared channel or one channel per room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomRoutingMode {
    /// Each room maps to `mudl-<slug>` joined via `conversations.join`.
    NamedChannel,
    /// Each room maps to a thread in [`SlackConfig::rooms_channel`].
    Threaded,
}

/// Routing mode derived from configuration.
pub fn room_routing_mode(config: &SlackConfig) -> RoomRoutingMode {
    if config.rooms_channel.is_some() {
        RoomRoutingMode::Threaded
    } else {
        RoomRoutingMode::NamedChannel
    }
}

/// Shared in-character Slack presence for open-world play.
pub fn shared_ic_presence(config: &SlackConfig) -> String {
    config
        .rooms_channel
        .clone()
        .unwrap_or_else(|| config.world_channel.clone())
}

/// Presence key for in-character speech and movement sync for a game room.
pub fn room_presence(config: &SlackConfig, room_id: &ObjectId) -> String {
    match room_routing_mode(config) {
        RoomRoutingMode::Threaded => {
            let rooms = config
                .rooms_channel
                .as_deref()
                .expect("rooms_channel required for threaded mode");
            room_thread_presence(rooms, room_id)
        }
        RoomRoutingMode::NamedChannel => {
            room_channel_name(&config.room_channel_prefix, room_id)
        }
    }
}

/// In-character presence key for speech/movement routing.
pub fn speech_presence(config: &SlackConfig, room_id: &ObjectId) -> String {
    match config.play_mode {
        PlayMode::Story => room_presence(config, room_id),
        PlayMode::Open => shared_ic_presence(config),
    }
}

/// Presence surfaces to join on login for the active play mode.
pub fn login_presence_joins(config: &SlackConfig, room_id: Option<&ObjectId>) -> Vec<String> {
    match config.play_mode {
        PlayMode::Story => {
            let mut joins = Vec::new();
            if !config.world_channel.is_empty() {
                joins.push(config.world_channel.clone());
            }
            if let Some(room_id) = room_id {
                joins.push(room_presence(config, room_id));
            }
            joins
        }
        PlayMode::Open => {
            let mut joins = Vec::new();
            let shared = shared_ic_presence(config);
            if !shared.is_empty() {
                joins.push(shared);
            }
            joins
        }
    }
}

/// Presence surfaces to leave on logout for the active play mode.
pub fn logout_presence_parts(config: &SlackConfig, room_id: Option<&ObjectId>) -> Vec<String> {
    login_presence_joins(config, room_id)
}

/// Ephemeral notice inviting a player to a room channel or thread.
pub fn room_join_notice(presence: &str) -> String {
    format!("Follow {presence} for speech and actions in your current location.")
}

/// Login/movement notice for the active play mode.
pub fn ic_join_notice(config: &SlackConfig, room_id: &ObjectId) -> String {
    match config.play_mode {
        PlayMode::Story => room_join_notice(&room_presence(config, room_id)),
        PlayMode::Open => format!(
            "Follow {} for in-character speech and actions.",
            shared_ic_presence(config)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_channel_uses_object_slug() {
        let config = SlackConfig::default();
        let name = room_channel_name(&config.room_channel_prefix, &ObjectId::new("room:void-001"));
        assert_eq!(name, "mudl-void-001");
    }

    #[test]
    fn room_thread_presence_encodes_thread_ts() {
        let presence = room_thread_presence("C_ROOMS", &ObjectId::new("room:void-001"));
        assert_eq!(presence, "C_ROOMS:thread:room-void-001");
    }

    #[test]
    fn classifies_world_and_dm() {
        let config = SlackConfig {
            world_channel: "C_WORLD".to_string(),
            ..SlackConfig::default()
        };
        assert_eq!(
            classify_channel("D1", Some("im"), &config),
            ChannelTarget::DirectMessage
        );
        assert_eq!(
            classify_channel("C_WORLD", Some("channel"), &config),
            ChannelTarget::World
        );
    }
}