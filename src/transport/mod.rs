//! Transport-neutral delivery surface for game frontends (IRC, Slack, WebSocket).
//!
//! [`GameTransport`] captures deliver/join/leave semantics shared across transports.
//! Protocol-specific setup (IRC registration, Slack socket mode) stays on per-transport
//! extensions such as [`crate::irc::IrcTransport`] and [`crate::slack::SlackWebTransport`].

use std::sync::{Arc, Mutex};

use async_trait::async_trait;

/// Outgoing action recorded by [`MockTransport`] for test assertions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutgoingAction {
    /// Direct message to one recipient (IRC PRIVMSG, Slack DM, WebSocket frame).
    Direct {
        recipient: String,
        text: String,
    },
    /// System or policy notice to one recipient (IRC NOTICE, Slack ephemeral).
    Notice {
        recipient: String,
        text: String,
    },
    /// Join a shared presence surface (IRC channel, Slack channel/thread).
    Join {
        presence: String,
    },
    /// Leave a shared presence surface (IRC PART, Slack leave).
    Leave {
        presence: String,
        message: Option<String>,
    },
}

/// Shared delivery interface for multi-user game frontends.
#[async_trait]
pub trait GameTransport: Send + Sync {
    /// Deliver a direct message to one recipient.
    async fn send_direct(&self, recipient: &str, text: &str);

    /// Deliver a notice or policy message to one recipient.
    async fn send_notice(&self, recipient: &str, text: &str);

    /// Join a shared presence surface (world channel, room channel, thread).
    async fn join(&self, presence: &str);

    /// Leave a shared presence surface, optionally with a parting message.
    async fn leave(&self, presence: &str, message: Option<&str>);
}

/// Split embedded newlines into separate delivery lines.
pub fn split_delivery_lines(text: &str) -> Vec<&str> {
    if text.contains('\n') {
        text.lines().collect()
    } else {
        vec![text]
    }
}

/// In-memory transport that records all outgoing actions for assertions.
#[derive(Debug, Default, Clone)]
pub struct MockTransport {
    log: Arc<Mutex<Vec<OutgoingAction>>>,
}

impl MockTransport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn recorded(&self) -> Vec<OutgoingAction> {
        self.log.lock().expect("mock transport lock").clone()
    }

    pub fn clear(&self) {
        self.log.lock().expect("mock transport lock").clear();
    }

    pub fn direct_messages_to(&self, recipient: &str) -> Vec<String> {
        self.recorded()
            .into_iter()
            .filter_map(|entry| match entry {
                OutgoingAction::Direct {
                    recipient: r,
                    text,
                } if r == recipient => Some(text),
                _ => None,
            })
            .collect()
    }

    /// Messages delivered to a shared presence surface (channel, thread, room).
    pub fn presence_messages(&self, presence: &str) -> Vec<String> {
        self.direct_messages_to(presence)
    }

    /// IRC-era alias for [`Self::direct_messages_to`].
    pub fn privmsgs_to(&self, target: &str) -> Vec<String> {
        self.direct_messages_to(target)
    }

    /// IRC-era alias for [`Self::presence_messages`].
    pub fn channel_messages(&self, channel: &str) -> Vec<String> {
        self.presence_messages(channel)
    }
}

#[async_trait]
impl GameTransport for MockTransport {
    async fn send_direct(&self, recipient: &str, text: &str) {
        self.log
            .lock()
            .expect("mock transport lock")
            .push(OutgoingAction::Direct {
                recipient: recipient.to_string(),
                text: text.to_string(),
            });
    }

    async fn send_notice(&self, recipient: &str, text: &str) {
        self.log
            .lock()
            .expect("mock transport lock")
            .push(OutgoingAction::Notice {
                recipient: recipient.to_string(),
                text: text.to_string(),
            });
    }

    async fn join(&self, presence: &str) {
        self.log
            .lock()
            .expect("mock transport lock")
            .push(OutgoingAction::Join {
                presence: presence.to_string(),
            });
    }

    async fn leave(&self, presence: &str, message: Option<&str>) {
        self.log
            .lock()
            .expect("mock transport lock")
            .push(OutgoingAction::Leave {
                presence: presence.to_string(),
                message: message.map(str::to_string),
            });
    }
}