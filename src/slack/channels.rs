//! Slack channel naming for world and per-room visibility.

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

/// Ephemeral notice inviting a player to a room channel or thread.
pub fn room_join_notice(presence: &str) -> String {
    format!("Follow {presence} for speech and actions in your current location.")
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