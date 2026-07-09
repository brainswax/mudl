//! Transport-specific routing adapters for the shared message router.

use crate::gateway::normalize_nick;
use crate::irc::resolve_connected_nick;
use crate::object::ObjectId;
use crate::slack::resolve_connected_user_async;
use crate::transport::message::PresenceResolver;
use crate::transport::router::TellResolver;

/// IRC presence routing via [`IrcConfig`](crate::irc::IrcConfig).
pub struct IrcPresenceResolver<'a> {
    pub config: &'a crate::irc::IrcConfig,
}

impl PresenceResolver for IrcPresenceResolver<'_> {
    fn speech_presence(&self, room: &ObjectId) -> String {
        crate::irc::speech_channel(self.config, room)
    }

    fn ic_join_notice(&self, room: &ObjectId) -> String {
        crate::irc::ic_join_notice(self.config, room)
    }
}

/// Slack presence routing via [`SlackConfig`](crate::slack::SlackConfig).
pub struct SlackPresenceResolver<'a> {
    pub config: &'a crate::slack::SlackConfig,
}

impl PresenceResolver for SlackPresenceResolver<'_> {
    fn speech_presence(&self, room: &ObjectId) -> String {
        crate::slack::speech_presence(self.config, room)
    }

    fn ic_join_notice(&self, room: &ObjectId) -> String {
        crate::slack::ic_join_notice(self.config, room)
    }

    fn story_movement_visibility(&self) -> bool {
        true
    }
}

/// IRC tell resolution by nick.
#[derive(Debug, Default)]
pub struct IrcTellResolver;

#[async_trait::async_trait]
impl TellResolver for IrcTellResolver {
    async fn resolve<P: crate::persistence::Persistence + Clone>(
        &self,
        manager: &crate::gateway::SessionManager<P>,
        identity: &str,
    ) -> Option<String> {
        resolve_connected_nick(manager, identity)
    }

    fn actor_matches(&self, actor_id: &str, resolved: &str) -> bool {
        normalize_nick(actor_id) == normalize_nick(resolved)
    }
}

/// Slack tell resolution by user id or display name.
#[derive(Debug, Default)]
pub struct SlackTellResolver;

#[async_trait::async_trait]
impl TellResolver for SlackTellResolver {
    async fn resolve<P: crate::persistence::Persistence + Clone>(
        &self,
        manager: &crate::gateway::SessionManager<P>,
        identity: &str,
    ) -> Option<String> {
        resolve_connected_user_async(manager, identity).await
    }

    fn actor_matches(&self, actor_id: &str, resolved: &str) -> bool {
        normalize_nick(actor_id) == normalize_nick(resolved)
    }
}