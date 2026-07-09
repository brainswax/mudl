//! Multi-user group play — room visibility, private tells, and channel fan-out.

use crate::gateway::{PlayMode, SessionManager};
use crate::irc::connected_speech_audience_async;
use crate::object::ObjectId;
use crate::persistence::Persistence;

use super::config::SlackConfig;
use super::dispatch::{DispatchOutcome, RoomDelivery};
use super::format::{format_arrival, format_departure};
use super::channels::speech_presence;

/// Resolve a connected player's in-world display name from their Slack user id.
pub async fn speaker_display_name_async<P: Persistence + Clone>(
    manager: &SessionManager<P>,
    user_id: &str,
) -> String {
    let Some(handle) = manager.session_handle(user_id) else {
        return user_id.to_string();
    };
    let session = handle.lock().await;
    session
        .with_world(|world, player| {
            world
                .object(player.actor_id())
                .map(|obj| obj.name.clone())
        })
        .unwrap_or_else(|| user_id.to_string())
}

/// Fan movement arrival/departure to co-located players and room channels.
pub async fn append_movement_visibility<P: Persistence + Clone + Send + Sync>(
    outcome: &mut DispatchOutcome,
    manager: &SessionManager<P>,
    user_id: &str,
    old_room: &ObjectId,
    new_room: &ObjectId,
    config: &SlackConfig,
) {
    if old_room == new_room {
        return;
    }

    let speaker = speaker_display_name_async(manager, user_id).await;
    let departure = format_departure(&speaker);
    let arrival = format_arrival(&speaker);

    let shared = speech_presence(config, new_room);
    match config.play_mode {
        PlayMode::Story => {
            let old_audience = connected_speech_audience_async(
                manager,
                old_room,
                Some(user_id),
                config.play_mode,
            )
            .await;
            if !old_audience.is_empty() {
                outcome.room_audience.push(RoomDelivery {
                    audience: old_audience,
                    lines: vec![departure.clone()],
                });
            }
            outcome
                .channel
                .push((speech_presence(config, old_room), departure));

            let new_audience = connected_speech_audience_async(
                manager,
                new_room,
                Some(user_id),
                config.play_mode,
            )
            .await;
            if !new_audience.is_empty() {
                outcome.room_audience.push(RoomDelivery {
                    audience: new_audience,
                    lines: vec![arrival.clone()],
                });
            }
            outcome.channel.push((shared, arrival));
        }
        PlayMode::Open => {
            outcome.channel.push((shared.clone(), departure));
            outcome.channel.push((shared, arrival));
        }
    }
}

/// Whether `text` is a private tell line (not broadcast to room channels).
pub fn is_private_tell_line(text: &str) -> bool {
    text.contains(" tells you, ")
        || text.contains(" whispers, ")
        || text.contains("You whisper to ")
        || text.contains("You tell ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_private_tell_lines() {
        assert!(is_private_tell_line("Bob tells you, \"hi\""));
        assert!(is_private_tell_line(":envelope: *Bob* whispers, “hi”"));
        assert!(!is_private_tell_line("Alice says, \"hi\""));
    }

    #[test]
    fn movement_formatting_escapes_speaker() {
        assert_eq!(
            format_arrival("Alice"),
            "*Alice* has arrived."
        );
        assert_eq!(
            format_departure("Bob & Co"),
            "*Bob &amp; Co* has left."
        );
    }
}