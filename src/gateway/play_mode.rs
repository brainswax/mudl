//! Configurable play mode for multi-user transports.
//!
//! - **Story** (default): private command output, per-room channels, room-local speech.
//! - **Open**: single shared in-character channel; `look` stays room-scoped; speech/movement
//!   visible to all connected players; DMs/tells unchanged.

use crate::display::ResolveScope;

use super::open_delivery::transport_look_scope;

/// Arrival/departure fan-out on movement in open-world mode.
///
/// Movement narration and room context still post via [`OpenContext`](crate::transport::GameMessage::OpenContext)
/// on the shared channel; this only controls generic "has left" / "has arrived" lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpenMovementNotices {
    /// No generic movement notices (default — avoids spam on the shared channel).
    #[default]
    Off,
    /// One line: `Alice enters The North Woods.`
    Compact,
    /// Legacy: separate departure and arrival lines.
    Full,
}

impl OpenMovementNotices {
    /// Load from `MUDL_OPEN_MOVEMENT_NOTICES` (`off`, `compact`, `full`).
    pub fn from_env() -> Self {
        match std::env::var("MUDL_OPEN_MOVEMENT_NOTICES")
            .ok()
            .as_deref()
            .map(str::trim)
            .map(|s| s.to_ascii_lowercase())
            .as_deref()
        {
            Some("full" | "true" | "1" | "yes" | "on") => Self::Full,
            Some("compact") => Self::Compact,
            Some("off" | "false" | "0" | "no") | None | Some(_) => Self::Off,
        }
    }
}

/// How a live transport maps game visibility to chat surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayMode {
    /// Private DMs for commands/output; rooms map to per-room channels; room-local `say`/`emote`.
    #[default]
    Story,
    /// One shared in-character channel; speech and movement visible to all connected players.
    Open,
}

impl PlayMode {
    /// Load from `MUDL_PLAY_MODE` (`story` default, `open` for open world).
    pub fn from_env() -> Self {
        match std::env::var("MUDL_PLAY_MODE")
            .ok()
            .as_deref()
            .map(str::trim)
            .map(|s| s.to_ascii_lowercase())
            .as_deref()
        {
            Some("open" | "open_world" | "open-world") => PlayMode::Open,
            Some("story") | None | Some(_) => PlayMode::Story,
        }
    }

    pub fn is_story(self) -> bool {
        matches!(self, PlayMode::Story)
    }

    pub fn is_open(self) -> bool {
        matches!(self, PlayMode::Open)
    }

    /// Target resolution scope for player `look` over IRC/Slack.
    pub fn look_scope(self) -> ResolveScope {
        transport_look_scope(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn story_is_default_look_scope() {
        assert_eq!(PlayMode::Story.look_scope(), ResolveScope::RoomOnly);
    }

    #[test]
    fn open_look_stays_room_scoped() {
        assert_eq!(PlayMode::Open.look_scope(), ResolveScope::RoomOnly);
    }

    #[test]
    fn open_movement_notices_default_off() {
        assert_eq!(OpenMovementNotices::default(), OpenMovementNotices::Off);
    }
}