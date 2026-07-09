//! Transport-neutral message semantics and delivery targets.
//!
//! [`GameMessage`] describes *what* to say; [`DeliveryTarget`] describes *where* it goes.
//! Play-mode routing lives in [`super::router`]; client formatting in [`super::formatter`].

use crate::object::ObjectId;

/// Semantic game output before client-specific formatting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameMessage {
    /// Raw command or system text (look output, errors, help, …).
    Plain(String),
    /// In-character speech (`say`).
    Say {
        speaker: String,
        text: String,
    },
    /// In-character action (`emote`).
    Emote {
        speaker: String,
        text: String,
    },
    /// Private tell received by the target.
    Tell {
        from: String,
        text: String,
    },
    /// Confirmation shown to the tell sender.
    TellSent {
        to: String,
        text: String,
    },
    /// Open-mode command output labeled with actor place.
    OpenContext {
        speaker: String,
        room: String,
        body: String,
    },
    /// Movement: actor entered a room.
    Arrival {
        speaker: String,
    },
    /// Movement: actor left a room.
    Departure {
        speaker: String,
    },
    /// Out-of-character chat on the world channel (story mode).
    Ooc {
        speaker: String,
        text: String,
    },
}

/// Where a formatted message should be delivered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryTarget {
    /// Direct message to the command actor (private DM surface).
    Actor,
    /// Direct message to another connected player.
    User(String),
    /// Direct messages to co-located players (story mode speech/movement).
    RoomAudience(Vec<String>),
    /// Shared in-character presence (IRC channel, Slack thread/channel).
    SharedPresence(String),
}

/// One routed message with an explicit destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedDelivery {
    pub target: DeliveryTarget,
    pub message: GameMessage,
}

/// Join/leave instructions for room-scoped presence surfaces.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PresenceSyncPlan {
    pub actor: String,
    pub join: Vec<String>,
    pub part: Vec<String>,
}

/// Full delivery plan produced by the message router.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeliveryPlan {
    pub deliveries: Vec<PlannedDelivery>,
    pub presence_sync: Option<PresenceSyncPlan>,
    pub persist: bool,
}

impl DeliveryPlan {
    pub fn push(&mut self, target: DeliveryTarget, message: GameMessage) {
        self.deliveries.push(PlannedDelivery { target, message });
    }

    pub fn actor_plain(&mut self, text: impl Into<String>) {
        self.push(DeliveryTarget::Actor, GameMessage::Plain(text.into()));
    }

    pub fn shared(&mut self, presence: impl Into<String>, message: GameMessage) {
        self.push(DeliveryTarget::SharedPresence(presence.into()), message);
    }
}

/// Resolve speech/movement presence for a room (transport-specific).
pub trait PresenceResolver: Send + Sync {
    fn speech_presence(&self, room: &ObjectId) -> String;
    fn ic_join_notice(&self, room: &ObjectId) -> String;
    /// Whether story-mode movement should fan out arrival/departure to co-located players.
    fn story_movement_visibility(&self) -> bool {
        false
    }
}