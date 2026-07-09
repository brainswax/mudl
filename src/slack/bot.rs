//! Slack bot — command relay, visibility, and multi-session coordination.

use std::sync::Arc;

use tokio::sync::Mutex;

use crate::command::parse_command_line;
use crate::gateway::{
    is_open_channel_command, is_open_private_actor_line, RateLimitKind, SessionManager,
};
use crate::persistence::Persistence;

use super::config::SlackConfig;
use super::dispatch::{dispatch_command, DispatchOutcome, PresenceSync};
use super::format::{classify_slack_output, format_ooc, format_help_text, format_slack_message};
use super::transport::SlackFormattedDelivery;
use super::events::{SlackChannelKind, SlackEventBody, SlackMessageEvent};
use super::input::normalize_slack_command_input;
use super::presence::encode_notice;
use super::session::SlackSessionRegistry;

/// Slack gateway bot backed by a shared [`SessionManager`].
pub struct SlackBot<P, T> {
    manager: Arc<Mutex<SessionManager<P>>>,
    transport: Arc<T>,
    config: SlackConfig,
    /// DM conversation ids per connected Slack user (delivery sidecar).
    slack_sessions: Arc<Mutex<SlackSessionRegistry>>,
}

impl<P, T> SlackBot<P, T>
where
    P: Persistence + Clone + Send + Sync + 'static,
    T: SlackFormattedDelivery + 'static,
{
    pub fn new(manager: SessionManager<P>, transport: Arc<T>, config: SlackConfig) -> Self {
        Self {
            manager: Arc::new(Mutex::new(manager)),
            transport,
            config,
            slack_sessions: Arc::new(Mutex::new(SlackSessionRegistry::default())),
        }
    }

    pub fn slack_sessions(&self) -> Arc<Mutex<SlackSessionRegistry>> {
        Arc::clone(&self.slack_sessions)
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

    /// Handle a raw command line from a Slack user (tests and mock mode).
    pub async fn handle_input(
        &self,
        user_id: &str,
        channel_id: &str,
        text: &str,
    ) -> anyhow::Result<DispatchOutcome> {
        let command = normalize_slack_command_input(text, self.config.app_id.as_deref());
        let outcome = dispatch_command(
            Arc::clone(&self.manager),
            user_id,
            channel_id,
            &command,
            &self.config,
        )
        .await;
        self.sync_slack_session(&outcome).await;
        self.deliver(&outcome).await;
        Ok(outcome)
    }

    async fn sync_slack_session(&self, outcome: &DispatchOutcome) {
        let connected = {
            let manager = self.manager.lock().await;
            manager.is_connected(&outcome.user_id)
        };
        let mut sessions = self.slack_sessions.lock().await;
        if connected {
            sessions.record(&outcome.user_id, &outcome.reply_channel);
        } else {
            sessions.remove(&outcome.user_id);
        }
    }

    async fn handle_message(&self, message: SlackMessageEvent) -> anyhow::Result<()> {
        let kind = super::events::classify_slack_channel_with_rooms(
            &message.channel,
            message.channel_type.as_deref(),
            &self.config.world_channel,
            self.config.rooms_channel.as_deref(),
        );

        match kind {
            SlackChannelKind::DirectMessage => {
                self.handle_input(&message.user, &message.channel, &message.text)
                    .await?;
                Ok(())
            }
            SlackChannelKind::World if self.config.play_mode.is_open() => {
                self.handle_open_channel_message(&message).await
            }
            SlackChannelKind::World => self.handle_world_ooc(&message).await,
            SlackChannelKind::Room
                if self.config.play_mode.is_open()
                    && self.is_shared_open_channel(&message.channel) =>
            {
                self.handle_open_channel_message(&message).await
            }
            SlackChannelKind::Room => {
                self.send_notice(
                    &message.channel,
                    &message.user,
                    "Send game commands in a DM to the MUDL bot. Use `say` and `emote` for in-character speech in your room channel or thread.",
                )
                .await;
                Ok(())
            }
            SlackChannelKind::Other => Ok(()),
        }
    }

    fn is_shared_open_channel(&self, channel_id: &str) -> bool {
        if !self.config.play_mode.is_open() {
            return false;
        }
        channel_id == super::channels::shared_ic_presence(&self.config)
            || self
                .config
                .rooms_channel
                .as_deref()
                .is_some_and(|rooms| channel_id == rooms)
    }

    async fn handle_open_channel_message(
        &self,
        message: &SlackMessageEvent,
    ) -> anyhow::Result<()> {
        let trimmed = message.text.trim();
        if trimmed.is_empty() {
            return Ok(());
        }

        let line = parse_command_line(trimmed);
        let manager = self.manager.lock().await;
        let is_command = is_open_channel_command(
            &manager,
            &message.user,
            &line,
            self.config.login_auth.auto_login,
        )
        .await;
        drop(manager);

        if is_command {
            self.handle_input(&message.user, &message.channel, trimmed)
                .await?;
        }

        // Non-commands: no bot action — the player's line is already in the channel.
        Ok(())
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

        if let Err(denied) = manager.check_rate_limit(&message.user, RateLimitKind::Ooc) {
            self.send_notice(
                &message.channel,
                &message.user,
                &manager.rate_limit_denial_message(denied.kind),
            )
            .await;
            return Ok(());
        }

        let display = manager
            .with_session(&message.user, |session| {
                session.with_world(|world, player| {
                    world
                        .object(player.actor_id())
                        .map(|obj| obj.name.clone())
                        .unwrap_or_else(|| message.user.clone())
                })
            })
            .await
            .unwrap_or_else(|| message.user.clone());

        drop(manager);

        let line = format_ooc(&display, trimmed);
        self.send_to_presence(&self.config.world_channel, &line).await;

        // Story mode also relays OOC to connected players via DM; open mode keeps it in-channel.
        if self.config.play_mode.is_story() {
            let connected: Vec<String> = self.manager.lock().await.connected_nicks();
            let sessions = self.slack_sessions.lock().await;
            for user_id in connected {
                if user_id != message.user {
                    let target = sessions.delivery_target(&user_id);
                    let formatted = format_slack_message(
                        &line,
                        classify_slack_output(&target, &line),
                    );
                    self.transport
                        .send_slack_message(&target, &formatted)
                        .await;
                }
            }
        }
        Ok(())
    }

    async fn deliver(&self, outcome: &DispatchOutcome) {
        if outcome
            .to_sender
            .iter()
            .any(|l| l.starts_with("MUDL Slack commands"))
        {
            let formatted = format_help_text(&outcome.to_sender);
            self.transport
                .send_slack_message(&outcome.reply_channel, &formatted)
                .await;
        } else {
            for line in &outcome.to_sender {
                if self.config.play_mode.is_story() || is_open_private_actor_line(line) {
                    self.send_to_presence(&outcome.reply_channel, line).await;
                }
            }
        }

        for (user_id, line) in &outcome.private {
            self.send_to_user_dm(user_id, line).await;
        }

        for delivery in &outcome.room_audience {
            for user_id in &delivery.audience {
                for line in &delivery.lines {
                    self.send_to_user_dm(user_id, line).await;
                }
            }
        }

        for (presence, line) in &outcome.channel {
            self.send_to_presence(presence, line).await;
        }

        if let Some(sync) = &outcome.presence_sync {
            self.apply_presence_sync(sync).await;
        }

        if outcome.persist {
            let (world, persistence) = {
                let manager = self.manager.lock().await;
                (manager.world().clone(), manager.persistence().clone())
            };
            let _ = world.persist_changes(&persistence).await;
        }
    }

    async fn apply_presence_sync(&self, sync: &PresenceSync) {
        for presence in &sync.part {
            self.transport.leave(presence, Some("leaving")).await;
        }
        for presence in &sync.join {
            self.transport.join(presence).await;
            self.send_to_presence(
                &sync.user_id,
                &super::channels::room_join_notice(presence),
            )
            .await;
        }
    }

    /// Deliver a private line to one player's DM conversation.
    async fn send_to_user_dm(&self, user_id: &str, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let target = self
            .slack_sessions
            .lock()
            .await
            .delivery_target(user_id);
        let kind = super::format::SlackOutputKind::DirectMessage;
        let formatted = format_slack_message(text, kind);
        self.transport
            .send_slack_message(&target, &formatted)
            .await;
    }

    async fn send_to_presence(&self, presence: &str, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let kind = classify_slack_output(presence, text);
        let formatted = format_slack_message(text, kind);
        self.transport
            .send_slack_message(presence, &formatted)
            .await;
    }

    async fn send_notice(&self, channel: &str, user: &str, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let recipient = encode_notice(channel, user);
        let formatted = format_slack_message(text, super::format::SlackOutputKind::Notice);
        self.transport
            .send_slack_notice(&recipient, &formatted)
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::SessionManager;
    use crate::mudl::AnatomyRegistry;
    use crate::object::{Object, ObjectId, PermissionFlags};
    use crate::persistence::SqlitePersistence;
    use crate::transport::OutgoingAction;
    use crate::transport::MockTransport;
    use std::collections::HashMap;

    fn bare(id: &str, name: &str) -> Object {
        Object {
            id: ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new("player:hero-001"),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            revision: 0,
            updated_at: None,
            is_deleted: false,
            deleted_at: None,
        }
    }

    async fn test_bot() -> SlackBot<SqlitePersistence, MockTransport> {
        let persistence = SqlitePersistence::new("sqlite::memory:").await.unwrap();
        let room = ObjectId::new("room:void-001");
        let mut hero = bare("player:hero-001", "alice");
        hero.set_property_string(crate::object::LOGIN_NAME_PROPERTY, "alice");
        hero.location = Some(room);
        let mut place = bare("room:void-001", "The Void");
        place.set_property_string("description", "void");
        persistence.save_object(&hero).await.unwrap();
        persistence.save_object(&place).await.unwrap();

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
    async fn dm_look_replies_in_channel() {
        let bot = test_bot().await;
        let transport = Arc::clone(&bot.transport);
        bot.handle_input("alice", "d1", "login").await.unwrap();
        transport.clear();
        bot.handle_input("alice", "d1", "look").await.unwrap();
        assert!(transport
            .direct_messages_to("d1")
            .iter()
            .any(|l| l.contains("void") || l.contains("Void")));
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
                    if recipient == "C_WORLD:notice:U1" =>
                {
                    Some(text)
                }
                _ => None,
            })
            .collect();
        assert!(
            notices
                .iter()
                .any(|n| n.contains("not logged in") && n.contains("login")),
            "expected login notice, got {notices:?}"
        );
    }

    #[tokio::test]
    async fn login_joins_world_and_room_presence() {
        let bot = test_bot().await;
        let transport = Arc::clone(&bot.transport);
        bot.handle_input("alice", "d1", "login").await.unwrap();
        let joins: Vec<String> = transport
            .recorded()
            .into_iter()
            .filter_map(|entry| match entry {
                OutgoingAction::Join { presence } => Some(presence),
                _ => None,
            })
            .collect();
        assert!(joins.iter().any(|p| p == "C_WORLD"));
        assert!(joins.iter().any(|p| p.contains("void-001")));
    }
}