//! Slack Web API transport — [`GameTransport`] mapping plus protocol-specific calls.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::Serialize;

use crate::transport::GameTransport;

/// Slack-specific outgoing action (includes Web API calls).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutgoingSlack {
    PostMessage { channel: String, text: String },
    PostEphemeral {
        channel: String,
        user: String,
        text: String,
    },
}

/// Slack extension of [`GameTransport`] for Web API delivery.
#[async_trait]
pub trait SlackTransport: GameTransport {
    /// Post a visible message to a channel or DM conversation.
    async fn post_message(&self, channel: &str, text: &str);

    /// Post an ephemeral notice visible only to one user in a channel.
    async fn post_ephemeral(&self, channel: &str, user: &str, text: &str);
}

/// Live Slack transport backed by `chat.postMessage` / `chat.postEphemeral`.
#[derive(Debug, Clone)]
pub struct SlackWebTransport {
    bot_token: String,
    record: Arc<Mutex<Vec<OutgoingSlack>>>,
}

impl SlackWebTransport {
    pub fn new(bot_token: impl Into<String>) -> Self {
        Self {
            bot_token: bot_token.into(),
            record: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn recorded(&self) -> Vec<OutgoingSlack> {
        self.record.lock().expect("slack transport lock").clone()
    }

    pub fn clear(&self) {
        self.record.lock().expect("slack transport lock").clear();
    }

    fn record_action(&self, action: OutgoingSlack) {
        self.record
            .lock()
            .expect("slack transport lock")
            .push(action);
    }

    async fn api_post_message(&self, channel: &str, text: &str) -> anyhow::Result<()> {
        #[derive(Serialize)]
        struct Body<'a> {
            channel: &'a str,
            text: &'a str,
        }

        let token = self.bot_token.clone();
        let channel = channel.to_string();
        let text = text.to_string();
        tokio::task::spawn_blocking(move || {
            let payload = serde_json::to_string(&Body {
                channel: &channel,
                text: &text,
            })?;
            let response = ureq::post("https://slack.com/api/chat.postMessage")
                .set("Authorization", &format!("Bearer {token}"))
                .set("Content-Type", "application/json; charset=utf-8")
                .send_string(&payload)?;
            if response.status() >= 400 {
                anyhow::bail!("chat.postMessage HTTP {}", response.status());
            }
            let body: serde_json::Value =
                serde_json::from_str(&response.into_string()?)?;
            if body.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
                let error = body
                    .get("error")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown_error");
                anyhow::bail!("chat.postMessage failed: {error}");
            }
            Ok(())
        })
        .await??;
        Ok(())
    }

    async fn api_post_ephemeral(&self, channel: &str, user: &str, text: &str) -> anyhow::Result<()> {
        #[derive(Serialize)]
        struct Body<'a> {
            channel: &'a str,
            user: &'a str,
            text: &'a str,
        }

        let token = self.bot_token.clone();
        let channel = channel.to_string();
        let user = user.to_string();
        let text = text.to_string();
        tokio::task::spawn_blocking(move || {
            let payload = serde_json::to_string(&Body {
                channel: &channel,
                user: &user,
                text: &text,
            })?;
            let response = ureq::post("https://slack.com/api/chat.postEphemeral")
                .set("Authorization", &format!("Bearer {token}"))
                .set("Content-Type", "application/json; charset=utf-8")
                .send_string(&payload)?;
            if response.status() >= 400 {
                anyhow::bail!("chat.postEphemeral HTTP {}", response.status());
            }
            let body: serde_json::Value =
                serde_json::from_str(&response.into_string()?)?;
            if body.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
                let error = body
                    .get("error")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown_error");
                anyhow::bail!("chat.postEphemeral failed: {error}");
            }
            Ok(())
        })
        .await??;
        Ok(())
    }
}

#[async_trait]
impl GameTransport for SlackWebTransport {
    /// Deliver to a Slack channel or DM conversation ID.
    async fn send_direct(&self, recipient: &str, text: &str) {
        self.record_action(OutgoingSlack::PostMessage {
            channel: recipient.to_string(),
            text: text.to_string(),
        });
        if let Err(err) = self.api_post_message(recipient, text).await {
            tracing::warn!(error = %err, channel = %recipient, "slack post_message failed");
        }
    }

    /// Ephemeral notices require both channel and user — pass `recipient` as `channel:user`.
    async fn send_notice(&self, recipient: &str, text: &str) {
        if let Some((channel, user)) = recipient.split_once(':') {
            self.record_action(OutgoingSlack::PostEphemeral {
                channel: channel.to_string(),
                user: user.to_string(),
                text: text.to_string(),
            });
            if let Err(err) = self.api_post_ephemeral(channel, user, text).await {
                tracing::warn!(error = %err, channel = %channel, user = %user, "slack post_ephemeral failed");
            }
        } else {
            self.send_direct(recipient, text).await;
        }
    }

    /// Room/thread presence — recorded for future routing; no Web API call yet.
    async fn join(&self, presence: &str) {
        tracing::debug!(presence = %presence, "slack join (deferred thread routing)");
    }

    async fn leave(&self, presence: &str, message: Option<&str>) {
        tracing::debug!(
            presence = %presence,
            message = message.unwrap_or_default(),
            "slack leave (deferred thread routing)"
        );
    }
}

#[async_trait]
impl SlackTransport for SlackWebTransport {
    async fn post_message(&self, channel: &str, text: &str) {
        self.send_direct(channel, text).await;
    }

    async fn post_ephemeral(&self, channel: &str, user: &str, text: &str) {
        self.send_notice(&format!("{channel}:{user}"), text).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[tokio::test]
    async fn mock_transport_records_game_actions() {
        use crate::transport::OutgoingAction;

        let transport = MockTransport::new();
        transport.send_direct("C1", "hello").await;
        transport.send_notice("U1", "notice").await;
        assert_eq!(transport.direct_messages_to("C1"), vec!["hello".to_string()]);
        let notices: Vec<String> = transport
            .recorded()
            .into_iter()
            .filter_map(|entry| match entry {
                OutgoingAction::Notice { recipient, text } if recipient == "U1" => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(notices, vec!["notice".to_string()]);
    }

    #[tokio::test]
    async fn web_transport_records_without_network_when_token_empty() {
        let transport = SlackWebTransport::new("");
        transport.post_message("C1", "hi").await;
        assert_eq!(
            transport.recorded(),
            vec![OutgoingSlack::PostMessage {
                channel: "C1".to_string(),
                text: "hi".to_string()
            }]
        );
    }
}