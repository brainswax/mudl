//! Open-world delivery helpers — location context, shared-channel broadcast, plain chat.
//!
//! In **open** play mode the world/rooms channel is the single in-character surface.
//! Player `look` stays room-scoped; output is labeled with the actor's current place.

use crate::command::SocialIntent;
use crate::display::ResolveScope;
use crate::gateway::{PlayMode, SessionManager};
use crate::object::ObjectId;
use crate::persistence::Persistence;

/// Player `look` scope for live transports — always the actor's current room.
pub fn transport_look_scope(_mode: PlayMode) -> ResolveScope {
    ResolveScope::RoomOnly
}

/// Plain shared-channel chat in open mode (no OOC prefix).
pub fn format_open_chat(speaker: &str, text: &str) -> String {
    let speaker = speaker.trim();
    let text = sanitize_open_line(text);
    format!("{speaker}: {text}")
}

/// Prefix an action or command response with speaker and place context.
pub fn format_open_context_post(speaker: &str, room_name: &str, body: &str) -> String {
    let speaker = speaker.trim();
    let room_name = room_name.trim();
    let body = body.trim();
    if body.is_empty() {
        return format!("{speaker} @ {room_name}:");
    }
    format!("{speaker} @ {room_name}:\n{body}")
}

/// Whether a line should stay on the actor's private DM surface in open mode.
pub fn is_open_private_actor_line(line: &str) -> bool {
    let line = line.trim();
    if line.is_empty() {
        return true;
    }
    line.starts_with("MUDL ")
        || line.contains("Send '")
        || line.contains("Welcome to MUDL")
        || line.contains("Goodbye")
        || line.starts_with("Login name:")
        || line.contains("Join ")
        || line.contains("Follow ")
        || line.starts_with("You tell ")
        || line.starts_with("You whisper")
        || line.contains(" tells you, ")
        || line.contains(" whispers, ")
        || line.contains("is not connected")
        || line.contains("not logged in")
        || line.contains("too quickly")
        || line.contains("Invalid login")
        || line.contains("Registration is closed")
        || line.contains("don't see anything")
        || line.contains("You can't")
        || line.contains("You are not carrying")
        || line.contains("Say what")
}

/// Whether a [`CommandResult`] line is already posted via [`SocialIntent`] channel routing.
pub fn is_social_channel_duplicate(line: &str, social: Option<&SocialIntent>) -> bool {
    let Some(social) = social else {
        return false;
    };
    match social {
        SocialIntent::Say { speaker_name, text, .. } => {
            line.contains(speaker_name)
                && line.contains(text)
                && (line.contains(" says, ") || line.contains(" says, \""))
        }
        SocialIntent::Emote { speaker_name, text, .. } => {
            line.contains(speaker_name) && line.contains(text) && !line.contains(" says, ")
        }
        SocialIntent::Tell { .. } => false,
    }
}

/// Actor display name and current room label for open-mode context posts.
pub async fn actor_place_context<P: Persistence + Clone>(
    manager: &SessionManager<P>,
    actor_id: &str,
) -> Option<(String, ObjectId, String)> {
    let handle = manager.session_handle(actor_id)?;
    let session = handle.lock().await;
    session.with_world(|world, player| {
        let room_id = player.current_location()?.clone();
        let room_name = world
            .object(&room_id)
            .map(|obj| obj.name.clone())
            .unwrap_or_else(|| room_id.as_str().to_string());
        let speaker = world
            .object(player.actor_id())
            .map(|obj| obj.name.clone())
            .unwrap_or_else(|| actor_id.to_string());
        Some((speaker, room_id, room_name))
    })
}

/// Collect command output that should appear on the shared in-character channel.
pub fn open_channel_broadcast_body(
    social: Option<&SocialIntent>,
    outcome_lines: &[String],
) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for line in outcome_lines {
        let line = line.trim();
        if line.is_empty()
            || is_open_private_actor_line(line)
            || is_social_channel_duplicate(line, social)
        {
            continue;
        }
        parts.push(line.to_string());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn sanitize_open_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::ObjectId;

    #[test]
    fn open_chat_has_no_ooc_prefix() {
        assert_eq!(format_open_chat("Alice", "brb"), "Alice: brb");
    }

    #[test]
    fn context_post_labels_speaker_and_room() {
        let post = format_open_context_post("Alice", "The Void", "A dusty room.");
        assert!(post.starts_with("Alice @ The Void:"));
        assert!(post.contains("A dusty room."));
    }

    #[test]
    fn look_scope_is_room_only_in_open_mode() {
        assert_eq!(
            transport_look_scope(PlayMode::Open),
            ResolveScope::RoomOnly
        );
    }

    #[test]
    fn skips_social_duplicates_and_private_lines() {
        let result = CommandResult {
            social: Some(SocialIntent::Say {
                room_id: ObjectId::new("room:void-001"),
                speaker_name: "Alice".to_string(),
                text: "hi".to_string(),
            }),
            ..Default::default()
        };
        let lines = vec![
            "Alice says, \"hi\"".to_string(),
            "You pick up the sword.".to_string(),
            "You tell bob, \"secret\"".to_string(),
        ];
        let body = open_channel_broadcast_body(result.social.as_ref(), &lines).expect("body");
        assert!(body.contains("pick up the sword"));
        assert!(!body.contains("Alice says"));
        assert!(!body.contains("You tell"));
    }
}