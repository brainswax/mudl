//! Slack Events API payload parsing.

use serde_json::Value;

/// Top-level Events API envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlackEventsPayload {
    /// Initial URL verification handshake.
    UrlVerification { challenge: String },
    /// Delivered event from a workspace subscription.
    EventCallback(SlackEventCallback),
    /// Ignored envelope (acks, unknown types).
    Ignored,
}

/// `event_callback` wrapper with the nested event object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackEventCallback {
    pub team_id: Option<String>,
    pub event: SlackEventBody,
}

/// Parsed Slack event bodies MUDL cares about today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlackEventBody {
    Message(SlackMessageEvent),
    Ignored,
}

/// Inbound `message` event (DM or channel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackMessageEvent {
    pub user: String,
    pub text: String,
    pub channel: String,
    pub channel_type: Option<String>,
    pub thread_ts: Option<String>,
    pub ts: Option<String>,
}

/// How an inbound message channel should be routed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlackChannelKind {
    /// Direct message to the bot — game commands.
    DirectMessage,
    /// Configured world/OOC channel.
    World,
    /// Public or private channel (in-character routing deferred).
    Room,
    /// Unknown channel — ignore for now.
    Other,
}

/// Parse the raw Events API JSON body.
pub fn parse_events_payload(body: &str) -> Result<SlackEventsPayload, serde_json::Error> {
    let root: Value = serde_json::from_str(body)?;
    let event_type = root
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();

    match event_type {
        "url_verification" => {
            let challenge = root
                .get("challenge")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Ok(SlackEventsPayload::UrlVerification { challenge })
        }
        "event_callback" => parse_event_callback(&root).map(SlackEventsPayload::EventCallback),
        _ => Ok(SlackEventsPayload::Ignored),
    }
}

fn parse_event_callback(root: &Value) -> Result<SlackEventCallback, serde_json::Error> {
    let team_id = root
        .get("team_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let event_value = root.get("event").cloned().unwrap_or(Value::Null);
    let event = parse_event_body(&event_value);
    Ok(SlackEventCallback { team_id, event })
}

fn parse_event_body(value: &Value) -> SlackEventBody {
    let event_type = value.get("type").and_then(Value::as_str).unwrap_or_default();
    if event_type != "message" {
        return SlackEventBody::Ignored;
    }

    if value.get("bot_id").is_some() {
        return SlackEventBody::Ignored;
    }
    if value
        .get("subtype")
        .and_then(Value::as_str)
        .is_some_and(|s| s == "bot_message")
    {
        return SlackEventBody::Ignored;
    }

    let Some(user) = value.get("user").and_then(Value::as_str) else {
        return SlackEventBody::Ignored;
    };
    let text = value
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let channel = value
        .get("channel")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if channel.is_empty() {
        return SlackEventBody::Ignored;
    }

    SlackEventBody::Message(SlackMessageEvent {
        user: user.to_string(),
        text,
        channel,
        channel_type: value
            .get("channel_type")
            .and_then(Value::as_str)
            .map(str::to_string),
        thread_ts: value
            .get("thread_ts")
            .and_then(Value::as_str)
            .map(str::to_string),
        ts: value.get("ts").and_then(Value::as_str).map(str::to_string),
    })
}

/// Classify where a message arrived relative to [`SlackConfig`](super::config::SlackConfig).
pub fn classify_slack_channel(
    channel_id: &str,
    channel_type: Option<&str>,
    world_channel: &str,
) -> SlackChannelKind {
    classify_slack_channel_with_rooms(channel_id, channel_type, world_channel, None)
}

/// Classify with optional shared rooms channel for threaded in-character play.
pub fn classify_slack_channel_with_rooms(
    channel_id: &str,
    channel_type: Option<&str>,
    world_channel: &str,
    rooms_channel: Option<&str>,
) -> SlackChannelKind {
    if channel_type == Some("im") {
        return SlackChannelKind::DirectMessage;
    }
    if !world_channel.is_empty() && channel_id == world_channel {
        return SlackChannelKind::World;
    }
    if rooms_channel.is_some_and(|rooms| channel_id == rooms) {
        return SlackChannelKind::Room;
    }
    if channel_type == Some("channel") || channel_type == Some("group") {
        return SlackChannelKind::Room;
    }
    SlackChannelKind::Other
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_url_verification() {
        let body = r#"{"type":"url_verification","challenge":"abc123"}"#;
        let payload = parse_events_payload(body).expect("parse");
        assert_eq!(
            payload,
            SlackEventsPayload::UrlVerification {
                challenge: "abc123".to_string()
            }
        );
    }

    #[test]
    fn parses_dm_message_event() {
        let body = r#"{
            "type": "event_callback",
            "team_id": "T123",
            "event": {
                "type": "message",
                "user": "U456",
                "text": "look",
                "channel": "D789",
                "channel_type": "im"
            }
        }"#;
        let payload = parse_events_payload(body).expect("parse");
        let SlackEventsPayload::EventCallback(callback) = payload else {
            panic!("expected event_callback");
        };
        let SlackEventBody::Message(message) = callback.event else {
            panic!("expected message");
        };
        assert_eq!(message.user, "U456");
        assert_eq!(message.text, "look");
        assert_eq!(message.channel, "D789");
        assert_eq!(message.channel_type.as_deref(), Some("im"));
    }

    #[test]
    fn ignores_bot_messages() {
        let body = r#"{
            "type": "event_callback",
            "event": {
                "type": "message",
                "bot_id": "B1",
                "text": "loop",
                "channel": "C1"
            }
        }"#;
        let payload = parse_events_payload(body).expect("parse");
        let SlackEventsPayload::EventCallback(callback) = payload else {
            panic!("expected event_callback");
        };
        assert_eq!(callback.event, SlackEventBody::Ignored);
    }

    #[test]
    fn classifies_world_and_dm_channels() {
        assert_eq!(
            classify_slack_channel("D1", Some("im"), "C_WORLD"),
            SlackChannelKind::DirectMessage
        );
        assert_eq!(
            classify_slack_channel("C_WORLD", Some("channel"), "C_WORLD"),
            SlackChannelKind::World
        );
        assert_eq!(
            classify_slack_channel("C_ROOM", Some("channel"), "C_WORLD"),
            SlackChannelKind::Room
        );
    }

    #[test]
    fn classifies_shared_rooms_channel_as_room() {
        assert_eq!(
            classify_slack_channel_with_rooms(
                "C_ROOMS",
                Some("channel"),
                "C_WORLD",
                Some("C_ROOMS")
            ),
            SlackChannelKind::Room
        );
    }
}