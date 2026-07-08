//! Slack bot — event relay and multi-session coordination (M6 skeleton).

use std::sync::Arc;

use tokio::sync::Mutex;

use crate::gateway::SessionManager;
use crate::persistence::Persistence;
use crate::transport::{split_delivery_lines, GameTransport};

use super::config::SlackConfig;
use super::events::{classify_slack_channel, SlackChannelKind, SlackEventBody, SlackMessageEvent};
use super::input::normalize_slack_command_input;

/// Slack gateway bot backed by a shared [`SessionManager`].
///
/// Command dispatch via [`CommandDispatcher`](crate::command::CommandDispatcher) lands in
/// `slack/dispatch.rs` (next M6 step). This skeleton receives messages and acknowledges
/// them through [`GameTransport`].
pub struct SlackBot<P, T> {
    manager: Arc<Mutex<SessionManager<P>>>,
    transport: Arc<T>,
    config: SlackConfig,
}

impl<P, T> SlackBot<P, T>
where
    P: Persistence + Clone + Send + Sync + 'static,
    T: GameTransport + 'static,
{
    pub fn new(manager: SessionManager<P>, transport: Arc<T>, config: SlackConfig) -> Self {
        Self {
            manager: Arc::new(Mutex::new(manager)),
            transport,
            config,
        }
    }

    pub fn config(&self) -> &SlackConfig {
        &self.config
    }

    pub fn manager(&self) -> Arc<Mutex<SessionManager<P>>> {
        Arc::clone(&self.manager)
    }

    /// Handle one parsed Slack event body from the Events API.
    pub async fn handle_event(&self, event: SlackEventBody) -> anyhow::Result<()> {
        match event {
            SlackEventBody::Message(message) => self.handle_message(message).await,
            SlackEventBody::Ignored => Ok(()),
        }
    }

    /// Handle a raw command line from a Slack user id (tests and mock mode).
    pub async fn handle_input(
        &self,
        user_id: &str,
        channel_id: &str,
        text: &str,
    ) -> anyhow::Result<()> {
        let command = normalize_slack_command_input(text, self.config.app_id.as_deref());
        if command.is_empty() {
            self.send_to_channel(channel_id, "Send a command, or 'help' once dispatch is wired.")
                .await;
            return Ok(());
        }

        tracing::info!(user = %user_id, channel = %channel_id, command = %command, "slack command received");
        self.send_to_channel(
            channel_id,
            &format!(
                "MUDL Slack bot received `{command}`. Full command dispatch arrives in the next M6 step — use the IRC bot or REPL for play today."
            ),
        )
        .await;
        Ok(())
    }

    async fn handle_message(&self, message: SlackMessageEvent) -> anyhow::Result<()> {
        let kind = classify_slack_channel(
            &message.channel,
            message.channel_type.as_deref(),
            &self.config.world_channel,
        );

        match kind {
            SlackChannelKind::DirectMessage => {
                self.handle_input(&message.user, &message.channel, &message.text)
                    .await
            }
            SlackChannelKind::World => self.handle_world_ooc(&message).await,
            SlackChannelKind::Room => {
                self.send_notice(
                    &message.channel,
                    &message.user,
                    "Send game commands in a DM to the MUDL bot. Use channel threads for in-character speech once routing lands.",
                )
                .await;
                Ok(())
            }
            SlackChannelKind::Other => Ok(()),
        }
    }

    async fn handle_world_ooc(&self, message: &SlackMessageEvent) -> anyhow::Result<()> {
        let trimmed = message.text.trim();
        if trimmed.is_empty() {
            return Ok(());
        }

        let manager = self.manager.lock().await;
        if !manager.is_connected(&message.user) {
            self.send_notice(
                &message.channel,
                &message.user,
                "You are not logged in. DM the bot with `login`.",
            )
            .await;
            return Ok(());
        }
        drop(manager);

        let line = format!("<@{}> (OOC): {}", message.user, trimmed);
        self.send_to_channel(&self.config.world_channel, &line).await;
        Ok(())
    }

    async fn send_to_channel(&self, channel: &str, text: &str) {
        for part in split_delivery_lines(text) {
            if !part.is_empty() {
                self.transport.send_direct(channel, part).await;
            }
        }
    }

    async fn send_notice(&self, channel: &str, user: &str, text: &str) {
        let recipient = format!("{channel}:{user}");
        for part in split_delivery_lines(text) {
            if !part.is_empty() {
                self.transport.send_notice(&recipient, part).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::SessionManager;
    use crate::mudl::AnatomyRegistry;
    use crate::persistence::SqlitePersistence;
    use crate::transport::OutgoingAction;
    use crate::transport::MockTransport;

    async fn test_bot() -> SlackBot<SqlitePersistence, MockTransport> {
        let persistence = SqlitePersistence::new("sqlite::memory:").await.unwrap();
        let manager = SessionManager::open(persistence, AnatomyRegistry::default())
            .await
            .unwrap();
        let transport = Arc::new(MockTransport::new());
        let config = SlackConfig {
            world_channel: "C_WORLD".to_string(),
            ..SlackConfig::default()
        };
        SlackBot::new(manager, transport, config)
    }

    #[tokio::test]
    async fn dm_input_is_acknowledged() {
        let bot = test_bot().await;
        let transport = Arc::clone(&bot.transport);
        bot.handle_input("U1", "D1", "look").await.unwrap();
        assert_eq!(
            transport.direct_messages_to("D1"),
            vec!["MUDL Slack bot received `look`. Full command dispatch arrives in the next M6 step — use the IRC bot or REPL for play today.".to_string()]
        );
    }

    #[tokio::test]
    async fn world_ooc_requires_login() {
        let bot = test_bot().await;
        let transport = Arc::clone(&bot.transport);
        bot.handle_event(SlackEventBody::Message(SlackMessageEvent {
            user: "U1".to_string(),
            text: "brb".to_string(),
            channel: "C_WORLD".to_string(),
            channel_type: Some("channel".to_string()),
            thread_ts: None,
            ts: None,
        }))
        .await
        .unwrap();
        let notices: Vec<String> = transport
            .recorded()
            .into_iter()
            .filter_map(|entry| match entry {
                OutgoingAction::Notice { recipient, text }
                    if recipient == "C_WORLD:U1" =>
                {
                    Some(text)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            notices,
            vec!["You are not logged in. DM the bot with `login`.".to_string()]
        );
    }

    #[tokio::test]
    async fn strips_app_mention_in_dm() {
        let bot = test_bot().await;
        let mut config = bot.config().clone();
        config.app_id = Some("A_APP".to_string());
        let transport = Arc::clone(&bot.transport);
        let manager = bot.manager();
        let rebuilt = {
            let guard = manager.lock().await;
            SessionManager::from_world(guard.persistence().clone(), guard.world().clone())
        };
        let bot = SlackBot::new(rebuilt, transport, config);
        bot.handle_input("U1", "D1", "<@A_APP> help").await.unwrap();
        assert!(bot
            .transport
            .direct_messages_to("D1")
            .first()
            .is_some_and(|m| m.contains("`help`")));
    }
}