//! IRC bot — command relay, visibility, and multi-session coordination.

use std::sync::Arc;

use tokio::sync::Mutex;

use crate::gateway::{normalize_nick, RateLimitKind, SessionManager};
use crate::persistence::Persistence;

use super::channels::{classify_target, ChannelTarget};
use super::config::IrcConfig;
use super::dispatch::{dispatch_command, DispatchOutcome};
use super::input::normalize_irc_command_input;
use super::message::IrcMessage;
use super::social::format_ooc;
use crate::transport::{split_delivery_lines, GameTransport};

/// IRC gateway bot backed by a shared [`SessionManager`].
pub struct IrcBot<P, T> {
    manager: Arc<Mutex<SessionManager<P>>>,
    transport: Arc<T>,
    config: IrcConfig,
}

impl<P, T> IrcBot<P, T>
where
    P: Persistence + Clone + Send + Sync + 'static,
    T: GameTransport + 'static,
{
    pub fn new(manager: SessionManager<P>, transport: Arc<T>, config: IrcConfig) -> Self {
        Self {
            manager: Arc::new(Mutex::new(manager)),
            transport,
            config,
        }
    }

    pub fn config(&self) -> &IrcConfig {
        &self.config
    }

    pub fn manager(&self) -> Arc<Mutex<SessionManager<P>>> {
        Arc::clone(&self.manager)
    }

    /// Handle one parsed IRC message and route game commands through the session manager.
    pub async fn handle_message(&self, message: IrcMessage) -> anyhow::Result<()> {
        match message {
            IrcMessage::Privmsg { from, target, text } => {
                self.handle_privmsg(&from, &target, &text).await
            }
            IrcMessage::Quit { nick, .. } => self.handle_disconnect(&nick).await,
            IrcMessage::Part { nick, channel, .. } if channel == self.config.world_channel => {
                self.handle_disconnect(&nick).await
            }
            _ => Ok(()),
        }
    }

    /// Handle a raw command line from an IRC nick (used by tests and direct adapters).
    pub async fn handle_input(&self, nick: &str, text: &str) -> anyhow::Result<DispatchOutcome> {
        let command = normalize_irc_command_input(text, &self.config.bot_nick);
        let outcome = dispatch_command(
            Arc::clone(&self.manager),
            nick,
            &command,
            &self.config,
        )
        .await;
        self.deliver(&outcome).await;
        Ok(outcome)
    }

    async fn handle_privmsg(&self, from: &str, target: &str, text: &str) -> anyhow::Result<()> {
        match classify_target(target, &self.config) {
            ChannelTarget::World => self.handle_world_ooc(from, text).await,
            ChannelTarget::Bot | ChannelTarget::Private(_) => {
                self.handle_input(from, text).await?;
                Ok(())
            }
            ChannelTarget::Room(_) => {
                self.transport
                    .send_notice(
                        from,
                        "Send commands to the bot directly. Use 'say' and 'emote' for in-character speech.",
                    )
                    .await;
                Ok(())
            }
        }
    }

    async fn handle_world_ooc(&self, from: &str, text: &str) -> anyhow::Result<()> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Ok(());
        }

        let manager = self.manager.lock().await;
        if !manager.is_connected(from) {
            self.transport
                .send_notice(from, "You are not logged in. Message the bot with 'login'.")
                .await;
            return Ok(());
        }

        if let Err(denied) = manager.check_rate_limit(from, RateLimitKind::Ooc) {
            self.transport
                .send_notice(from, &manager.rate_limit_denial_message(denied.kind))
                .await;
            return Ok(());
        }

        let line = format_ooc(from, trimmed);
        let nicks = manager.connected_nicks();
        drop(manager);

        self.transport
            .send_direct(&self.config.world_channel, &line)
            .await;
        let from_key = normalize_nick(from);
        for nick in nicks {
            if nick != from_key {
                self.transport.send_direct(&nick, &line).await;
            }
        }
        Ok(())
    }

    async fn handle_disconnect(&self, nick: &str) -> anyhow::Result<()> {
        let mut manager = self.manager.lock().await;
        if manager.is_connected(nick) {
            let _ = manager.logout(nick).await;
        }
        Ok(())
    }

    async fn deliver(&self, outcome: &DispatchOutcome) {
        for line in &outcome.to_sender {
            self.send_direct_lines(&outcome.sender, line).await;
        }

        for (nick, line) in &outcome.private {
            self.send_direct_lines(nick, line).await;
        }

        for delivery in &outcome.room_audience {
            for nick in &delivery.audience {
                for line in &delivery.lines {
                    self.send_direct_lines(nick, line).await;
                }
            }
        }

        for (channel, line) in &outcome.channel {
            self.send_direct_lines(channel, line).await;
        }

        if let Some(sync) = &outcome.channel_sync {
            for channel in &sync.part {
                self.transport.leave(channel, Some("leaving")).await;
            }
            for channel in &sync.join {
                self.transport.join(channel).await;
                self.transport
                    .send_notice(&sync.nick, &super::channels::room_join_notice(channel))
                    .await;
            }
        }

        if outcome.persist {
            let (world, persistence) = {
                let manager = self.manager.lock().await;
                (manager.world().clone(), manager.persistence().clone())
            };
            let _ = world.persist_changes(&persistence).await;
        }
    }

    /// Frontends expect one protocol line per direct message — split embedded newlines.
    async fn send_direct_lines(&self, target: &str, text: &str) {
        for part in split_delivery_lines(text) {
            if !part.is_empty() {
                self.transport.send_direct(target, part).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::SessionManager;
    use crate::object::ObjectId;
    use crate::irc::MockTransport;
    use crate::transport::OutgoingAction;
    use crate::object::{Object, PermissionFlags};
    use crate::persistence::SqlitePersistence;
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

    async fn bot_fixture() -> (
        IrcBot<SqlitePersistence, MockTransport>,
        Arc<MockTransport>,
    ) {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let room = ObjectId::new("room:void-001");

        let mut hero1 = bare("player:hero-001", "Alice");
        hero1.location = Some(room.clone());
        let mut hero2 = bare("player:hero-002", "Bob");
        hero2.location = Some(room);

        let mut place = bare("room:void-001", "The Void");
        place.set_property_string("description", "A featureless void stretches in every direction.");
        persistence.save_object(&hero1).await.unwrap();
        persistence.save_object(&hero2).await.unwrap();
        persistence.save_object(&place).await.unwrap();

        let manager = SessionManager::open(persistence, crate::mudl::AnatomyRegistry::default())
            .await
            .unwrap();
        let transport = Arc::new(MockTransport::new());
        let bot = IrcBot::new(manager, Arc::clone(&transport), IrcConfig::default());
        (bot, transport)
    }

    #[tokio::test]
    async fn bot_relays_say_to_co_located_player() {
        let (bot, transport) = bot_fixture().await;
        bot.handle_input("alice", "login").await.unwrap();
        bot.handle_input("bob", "login").await.unwrap();
        transport.clear();

        bot.handle_input("alice", "say hi bob").await.unwrap();

        let bob_lines = transport.privmsgs_to("bob");
        assert_eq!(bob_lines.len(), 1);
        assert!(bob_lines[0].contains("hi bob"));
    }

    #[tokio::test]
    async fn bot_delivers_private_tell() {
        let (bot, transport) = bot_fixture().await;
        bot.handle_input("alice", "login").await.unwrap();
        bot.handle_input("bob", "login").await.unwrap();
        transport.clear();

        bot.handle_input("alice", "tell bob psst").await.unwrap();

        let tells = transport.privmsgs_to("bob");
        assert!(tells.iter().any(|line| line.contains("psst")));
    }

    #[tokio::test]
    async fn world_channel_ooc_broadcasts_to_logged_in_players() {
        let (bot, transport) = bot_fixture().await;
        bot.handle_input("alice", "login").await.unwrap();
        bot.handle_input("bob", "login").await.unwrap();
        transport.clear();

        bot.handle_message(IrcMessage::Privmsg {
            from: "alice".to_string(),
            target: "#mudl".to_string(),
            text: "brb".to_string(),
        })
        .await
        .unwrap();

        assert!(transport
            .channel_messages("#mudl")
            .iter()
            .any(|line| line.contains("[OOC]") && line.contains("brb")));
        assert!(transport
            .privmsgs_to("bob")
            .iter()
            .any(|line| line.contains("brb")));
    }

    #[tokio::test]
    async fn concurrent_commands_from_two_players() {
        let (bot, transport) = bot_fixture().await;
        bot.handle_input("alice", "login").await.unwrap();
        bot.handle_input("bob", "login").await.unwrap();
        transport.clear();

        let bot_a = bot.manager();
        let bot_b = Arc::clone(&bot_a);
        let transport_a = Arc::clone(&transport);

        let alice = tokio::spawn(async move {
            dispatch_command(bot_a, "alice", "look", &IrcConfig::default()).await
        });
        let bob = tokio::spawn(async move {
            dispatch_command(bot_b, "bob", "look", &IrcConfig::default()).await
        });

        let (a_outcome, b_outcome) = tokio::join!(alice, bob);
        assert!(a_outcome
            .unwrap()
            .to_sender
            .iter()
            .any(|l| l.contains("featureless void")));
        assert!(b_outcome
            .unwrap()
            .to_sender
            .iter()
            .any(|l| l.contains("featureless void")));
        let _ = transport_a;
    }

    #[tokio::test]
    async fn ooc_flood_is_rate_limited() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let room = ObjectId::new("room:void-001");
        let mut hero = bare("player:hero-001", "Alice");
        hero.location = Some(room.clone());
        let place = bare("room:void-001", "The Void");
        persistence.save_object(&hero).await.unwrap();
        persistence.save_object(&place).await.unwrap();

        let rate_config = crate::gateway::RateLimitConfig {
            enabled: true,
            commands: crate::gateway::BucketSpec::new(30, 60.0),
            movement: crate::gateway::BucketSpec::new(8, 10.0),
            ooc: crate::gateway::BucketSpec::new(1, 30.0),
        };
        let manager = SessionManager::open_with_rate_limits(
            persistence,
            crate::mudl::AnatomyRegistry::default(),
            rate_config,
        )
        .await
        .unwrap();
        let transport = Arc::new(MockTransport::new());
        let bot = IrcBot::new(manager, Arc::clone(&transport), IrcConfig::default());
        bot.handle_input("alice", "login").await.unwrap();
        transport.clear();

        bot.handle_message(IrcMessage::Privmsg {
            from: "alice".to_string(),
            target: "#mudl".to_string(),
            text: "first".to_string(),
        })
        .await
        .unwrap();
        bot.handle_message(IrcMessage::Privmsg {
            from: "alice".to_string(),
            target: "#mudl".to_string(),
            text: "second".to_string(),
        })
        .await
        .unwrap();

        assert_eq!(transport.channel_messages("#mudl").len(), 1);
        assert!(transport.recorded().iter().any(|entry| {
            matches!(
                entry,
                OutgoingAction::Notice { recipient, text }
                    if recipient == "alice" && text.contains("out-of-character")
            )
        }));
    }

    #[tokio::test]
    async fn ooc_skips_duplicate_privmsg_for_mixed_case_sender() {
        let (bot, transport) = bot_fixture().await;
        bot.handle_input("alice", "login").await.unwrap();
        bot.handle_input("bob", "login").await.unwrap();
        transport.clear();

        bot.handle_message(IrcMessage::Privmsg {
            from: "Alice".to_string(),
            target: "#mudl".to_string(),
            text: "brb".to_string(),
        })
        .await
        .unwrap();

        let alice_priv = transport.privmsgs_to("alice");
        assert!(alice_priv.is_empty());
        assert!(transport.privmsgs_to("bob").iter().any(|l| l.contains("brb")));
    }

    #[tokio::test]
    async fn help_sends_one_privmsg_per_line() {
        let (bot, transport) = bot_fixture().await;
        bot.handle_input("alice", "login").await.unwrap();
        transport.clear();

        bot.handle_input("alice", "help").await.unwrap();

        let lines = transport.privmsgs_to("alice");
        assert!(lines.len() >= 8);
        assert!(lines.iter().any(|l| l.contains("MUDL IRC commands")));
        assert!(!lines.iter().any(|l| l.contains('\n')));
    }

    #[tokio::test]
    async fn accepts_msg_prefix_in_command_text() {
        let (bot, transport) = bot_fixture().await;
        bot.handle_input("alice", "login").await.unwrap();
        transport.clear();

        bot.handle_input("alice", "/msg mudl look")
            .await
            .unwrap();

        let lines = transport.privmsgs_to("alice");
        assert!(lines.iter().any(|l| l.contains("featureless void")));
    }

    #[tokio::test]
    async fn tell_confirmation_uses_resolved_nick() {
        let (bot, transport) = bot_fixture().await;
        bot.handle_input("alice", "login").await.unwrap();
        bot.handle_input("BOB", "login").await.unwrap();
        transport.clear();

        bot.handle_input("alice", "tell BOB hi").await.unwrap();

        let alice_lines = transport.privmsgs_to("alice");
        assert!(alice_lines.iter().any(|l| l.contains("You tell bob")));
    }

    #[tokio::test]
    async fn quit_persists_and_disconnects() {
        let (bot, transport) = bot_fixture().await;
        bot.handle_input("alice", "login").await.unwrap();
        bot.handle_input("alice", "quit").await.unwrap();
        assert!(!bot.manager().lock().await.is_connected("alice"));
        assert!(transport
            .recorded()
            .iter()
            .any(|entry| matches!(entry, OutgoingAction::Leave { .. })));
    }
}