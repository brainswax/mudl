//! Slack Web API transport — [`GameTransport`] mapping plus protocol-specific calls.
//!
//! Recipient conventions (shared with IRC dispatch adapters):
//!
//! | Recipient | Delivery |
//! |-----------|----------|
//! | `C…` / `D…` | `chat.postMessage` to conversation |
//! | `C…:thread:TS` | `chat.postMessage` in thread |
//! | `C…:notice:U…` | `chat.postEphemeral` |
//! | `U…` | `conversations.open` + DM post |
//! | `mudl-void-001` | `conversations.join` + post by channel name |

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::Serialize;
use serde_json::json;
use tracing::warn;

use crate::transport::GameTransport;

use super::api::slack_api_post_async;
use super::presence::{parse_recipient, SlackRecipient};

/// Slack-specific outgoing action (includes Web API calls).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutgoingSlack {
    PostMessage {
        channel: String,
        text: String,
        thread_ts: Option<String>,
    },
    PostEphemeral {
        channel: String,
        user: String,
        text: String,
        thread_ts: Option<String>,
    },
    Join { channel: String },
    Leave {
        channel: String,
        message: Option<String>,
    },
    OpenDm { user: String, channel: String },
}

impl From<crate::transport::OutgoingAction> for OutgoingSlack {
    fn from(action: crate::transport::OutgoingAction) -> Self {
        match action {
            crate::transport::OutgoingAction::Direct { recipient, text } => {
                let parsed = parse_recipient(&recipient);
                match parsed {
                    SlackRecipient::Channel { id, thread_ts } => OutgoingSlack::PostMessage {
                        channel: id,
                        text,
                        thread_ts,
                    },
                    SlackRecipient::User { id } => OutgoingSlack::OpenDm {
                        user: id,
                        channel: recipient,
                    },
                    SlackRecipient::ChannelName(name) => OutgoingSlack::Join { channel: name },
                    SlackRecipient::Notice { channel, user } => OutgoingSlack::PostEphemeral {
                        channel,
                        user,
                        text,
                        thread_ts: None,
                    },
                }
            }
            crate::transport::OutgoingAction::Notice { recipient, text } => {
                let parsed = parse_recipient(&recipient);
                match parsed {
                    SlackRecipient::Notice { channel, user } => OutgoingSlack::PostEphemeral {
                        channel,
                        user,
                        text,
                        thread_ts: None,
                    },
                    _ => OutgoingSlack::PostMessage {
                        channel: recipient,
                        text,
                        thread_ts: None,
                    },
                }
            }
            crate::transport::OutgoingAction::Join { presence } => OutgoingSlack::Join {
                channel: presence,
            },
            crate::transport::OutgoingAction::Leave { presence, message } => OutgoingSlack::Leave {
                channel: presence,
                message,
            },
        }
    }
}

/// Slack extension of [`GameTransport`] for Web API delivery.
#[async_trait]
pub trait SlackTransport: GameTransport {
    /// Post a visible message to a channel, DM, or thread.
    async fn post_message(&self, channel: &str, text: &str, thread_ts: Option<&str>);

    /// Post an ephemeral notice visible only to one user in a channel.
    async fn post_ephemeral(
        &self,
        channel: &str,
        user: &str,
        text: &str,
        thread_ts: Option<&str>,
    );

    /// Resolve (and cache) the DM conversation id for a workspace user.
    async fn open_dm(&self, user_id: &str) -> Option<String>;
}

/// Live Slack transport backed by the Web API.
#[derive(Debug, Clone)]
pub struct SlackWebTransport {
    bot_token: String,
    dm_cache: Arc<Mutex<HashMap<String, String>>>,
    record: Arc<Mutex<Vec<OutgoingSlack>>>,
}

impl SlackWebTransport {
    pub fn new(bot_token: impl Into<String>) -> Self {
        Self {
            bot_token: bot_token.into(),
            dm_cache: Arc::new(Mutex::new(HashMap::new())),
            record: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn recorded(&self) -> Vec<OutgoingSlack> {
        self.record.lock().expect("slack transport lock").clone()
    }

    pub fn clear(&self) {
        self.record.lock().expect("slack transport lock").clear();
        self.dm_cache.lock().expect("slack dm cache lock").clear();
    }

    fn record_action(&self, action: OutgoingSlack) {
        self.record
            .lock()
            .expect("slack transport lock")
            .push(action);
    }

    fn network_enabled(&self) -> bool {
        !self.bot_token.is_empty()
    }

    async fn deliver_recipient(&self, recipient: &str, text: &str, ephemeral: bool) {
        match parse_recipient(recipient) {
            SlackRecipient::Notice { channel, user } if ephemeral => {
                self.post_ephemeral(&channel, &user, text, None).await;
            }
            SlackRecipient::Channel { id, thread_ts } => {
                self.post_message(&id, text, thread_ts.as_deref()).await;
            }
            SlackRecipient::Notice { channel, user } => {
                self.post_ephemeral(&channel, &user, text, None).await;
            }
            SlackRecipient::User { id } => {
                if let Some(dm) = self.open_dm(&id).await {
                    self.post_message(&dm, text, None).await;
                } else {
                    warn!(user = %id, "failed to open slack DM");
                }
            }
            SlackRecipient::ChannelName(name) => {
                self.join(&name).await;
                self.post_message(&name, text, None).await;
            }
        }
    }

    async fn api_post_message(
        &self,
        channel: &str,
        text: &str,
        thread_ts: Option<&str>,
    ) -> anyhow::Result<()> {
        #[derive(Serialize)]
        struct Body<'a> {
            channel: &'a str,
            text: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            thread_ts: Option<&'a str>,
        }

        if !self.network_enabled() {
            return Ok(());
        }

        slack_api_post_async(
            self.bot_token.clone(),
            "chat.postMessage".to_string(),
            json!(Body {
                channel,
                text,
                thread_ts,
            }),
        )
        .await?;
        Ok(())
    }

    async fn api_post_ephemeral(
        &self,
        channel: &str,
        user: &str,
        text: &str,
        thread_ts: Option<&str>,
    ) -> anyhow::Result<()> {
        #[derive(Serialize)]
        struct Body<'a> {
            channel: &'a str,
            user: &'a str,
            text: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            thread_ts: Option<&'a str>,
        }

        if !self.network_enabled() {
            return Ok(());
        }

        slack_api_post_async(
            self.bot_token.clone(),
            "chat.postEphemeral".to_string(),
            json!(Body {
                channel,
                user,
                text,
                thread_ts,
            }),
        )
        .await?;
        Ok(())
    }

    async fn api_conversations_join(&self, channel: &str) -> anyhow::Result<()> {
        if !self.network_enabled() {
            return Ok(());
        }
        slack_api_post_async(
            self.bot_token.clone(),
            "conversations.join".to_string(),
            json!({ "channel": channel }),
        )
        .await?;
        Ok(())
    }

    async fn api_conversations_leave(&self, channel: &str) -> anyhow::Result<()> {
        if !self.network_enabled() {
            return Ok(());
        }
        slack_api_post_async(
            self.bot_token.clone(),
            "conversations.leave".to_string(),
            json!({ "channel": channel }),
        )
        .await?;
        Ok(())
    }

    async fn api_conversations_open(&self, user_id: &str) -> anyhow::Result<String> {
        if !self.network_enabled() {
            return Ok(format!("D_{user_id}"));
        }
        let body = slack_api_post_async(
            self.bot_token.clone(),
            "conversations.open".to_string(),
            json!({ "users": user_id }),
        )
        .await?;
        let channel = body
            .pointer("/channel/id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("conversations.open missing channel id"))?;
        Ok(channel.to_string())
    }
}

#[async_trait]
impl GameTransport for SlackWebTransport {
    async fn send_direct(&self, recipient: &str, text: &str) {
        self.deliver_recipient(recipient, text, false).await;
    }

    async fn send_notice(&self, recipient: &str, text: &str) {
        self.deliver_recipient(recipient, text, true).await;
    }

    async fn join(&self, presence: &str) {
        self.record_action(OutgoingSlack::Join {
            channel: presence.to_string(),
        });

        match parse_recipient(presence) {
            SlackRecipient::Channel { id, thread_ts: Some(thread_ts) } => {
                if let Err(err) = self
                    .api_post_message(&id, "_(entered the location)_", Some(&thread_ts))
                    .await
                {
                    warn!(error = %err, channel = %id, thread = %thread_ts, "slack thread join notice failed");
                }
            }
            SlackRecipient::Channel { id, thread_ts: None } => {
                if let Err(err) = self.api_conversations_join(&id).await {
                    warn!(error = %err, channel = %id, "slack conversations.join failed");
                }
            }
            SlackRecipient::ChannelName(name) => {
                if let Err(err) = self.api_conversations_join(&name).await {
                    warn!(error = %err, channel = %name, "slack conversations.join failed");
                }
            }
            SlackRecipient::User { .. } | SlackRecipient::Notice { .. } => {}
        }
    }

    async fn leave(&self, presence: &str, message: Option<&str>) {
        self.record_action(OutgoingSlack::Leave {
            channel: presence.to_string(),
            message: message.map(str::to_string),
        });

        if let Some(text) = message.filter(|m| !m.is_empty()) {
            self.send_direct(presence, text).await;
        }

        match parse_recipient(presence) {
            SlackRecipient::Channel { id, thread_ts: Some(thread_ts) } => {
                let _ = self
                    .api_post_message(&id, "_(left the location)_", Some(&thread_ts))
                    .await;
            }
            SlackRecipient::Channel { id, thread_ts: None } => {
                if let Err(err) = self.api_conversations_leave(&id).await {
                    warn!(error = %err, channel = %id, "slack conversations.leave failed");
                }
            }
            SlackRecipient::ChannelName(name) => {
                if let Err(err) = self.api_conversations_leave(&name).await {
                    warn!(error = %err, channel = %name, "slack conversations.leave failed");
                }
            }
            SlackRecipient::User { .. } | SlackRecipient::Notice { .. } => {}
        }
    }
}

#[async_trait]
impl SlackTransport for SlackWebTransport {
    async fn post_message(&self, channel: &str, text: &str, thread_ts: Option<&str>) {
        self.record_action(OutgoingSlack::PostMessage {
            channel: channel.to_string(),
            text: text.to_string(),
            thread_ts: thread_ts.map(str::to_string),
        });
        if let Err(err) = self.api_post_message(channel, text, thread_ts).await {
            warn!(error = %err, channel = %channel, "slack post_message failed");
        }
    }

    async fn post_ephemeral(
        &self,
        channel: &str,
        user: &str,
        text: &str,
        thread_ts: Option<&str>,
    ) {
        self.record_action(OutgoingSlack::PostEphemeral {
            channel: channel.to_string(),
            user: user.to_string(),
            text: text.to_string(),
            thread_ts: thread_ts.map(str::to_string),
        });
        if let Err(err) = self
            .api_post_ephemeral(channel, user, text, thread_ts)
            .await
        {
            warn!(
                error = %err,
                channel = %channel,
                user = %user,
                "slack post_ephemeral failed"
            );
        }
    }

    async fn open_dm(&self, user_id: &str) -> Option<String> {
        if let Some(cached) = self
            .dm_cache
            .lock()
            .expect("slack dm cache lock")
            .get(user_id)
            .cloned()
        {
            return Some(cached);
        }

        match self.api_conversations_open(user_id).await {
            Ok(channel) => {
                self.record_action(OutgoingSlack::OpenDm {
                    user: user_id.to_string(),
                    channel: channel.clone(),
                });
                self.dm_cache
                    .lock()
                    .expect("slack dm cache lock")
                    .insert(user_id.to_string(), channel.clone());
                Some(channel)
            }
            Err(err) => {
                warn!(error = %err, user = %user_id, "slack conversations.open failed");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::OutgoingAction;

    #[tokio::test]
    async fn records_post_message_without_network() {
        let transport = SlackWebTransport::new("");
        transport.post_message("C1", "hello", None).await;
        assert_eq!(
            transport.recorded(),
            vec![OutgoingSlack::PostMessage {
                channel: "C1".to_string(),
                text: "hello".to_string(),
                thread_ts: None
            }]
        );
    }

    #[tokio::test]
    async fn records_thread_post() {
        let transport = SlackWebTransport::new("");
        transport
            .post_message("C1", "line", Some("111.222"))
            .await;
        assert!(transport.recorded().iter().any(|entry| matches!(
            entry,
            OutgoingSlack::PostMessage {
                channel,
                thread_ts: Some(ts),
                ..
            } if channel == "C1" && ts == "111.222"
        )));
    }

    #[tokio::test]
    async fn join_and_leave_record_actions() {
        let transport = SlackWebTransport::new("");
        transport.join("mudl-void-001").await;
        transport.leave("C_OLD", Some("bye")).await;
        assert!(transport
            .recorded()
            .iter()
            .any(|e| matches!(e, OutgoingSlack::Join { channel } if channel == "mudl-void-001")));
        assert!(transport.recorded().iter().any(|e| matches!(
            e,
            OutgoingSlack::Leave { channel, message } if channel == "C_OLD" && message.as_deref() == Some("bye")
        )));
    }

    #[tokio::test]
    async fn send_direct_to_user_opens_dm() {
        let transport = SlackWebTransport::new("");
        transport.send_direct("U1", "psst").await;
        assert!(transport.recorded().iter().any(|e| matches!(
            e,
            OutgoingSlack::OpenDm { user, .. } if user == "U1"
        )));
        assert!(transport.recorded().iter().any(|e| matches!(
            e,
            OutgoingSlack::PostMessage { text, .. } if text == "psst"
        )));
    }

    #[tokio::test]
    async fn outgoing_action_converts_to_slack_variants() {
        let action = OutgoingAction::Direct {
            recipient: "C1:thread:9.9".to_string(),
            text: "hi".to_string(),
        };
        assert!(matches!(
            OutgoingSlack::from(action),
            OutgoingSlack::PostMessage { thread_ts: Some(_), .. }
        ));
    }

    #[tokio::test]
    async fn mock_transport_records_game_actions() {
        use crate::transport::MockTransport;

        let transport = MockTransport::new();
        transport.send_direct("C1", "hello").await;
        transport.send_notice("C1:notice:U1", "notice").await;
        transport.join("mudl-void-001").await;
        transport.leave("C1", Some("later")).await;
        assert_eq!(transport.direct_messages_to("C1"), vec!["hello".to_string()]);
        assert_eq!(
            transport
                .recorded()
                .iter()
                .filter(|e| matches!(e, OutgoingAction::Join { .. }))
                .count(),
            1
        );
    }
}