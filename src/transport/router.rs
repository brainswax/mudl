//! Message router — maps [`CommandResult`] to [`DeliveryPlan`] by play mode.
//!
//! Story mode keeps command output and most feedback on the actor's private surface;
//! open mode posts public command output to the shared in-character channel.

use crate::command::{CommandResult, SocialIntent};
use crate::gateway::{
    actor_place_context, is_open_private_actor_line, open_channel_broadcast_body, PlayMode,
    SessionManager,
};
use crate::irc::connected_speech_audience_async;
use crate::object::ObjectId;
use crate::persistence::Persistence;

use super::message::{
    DeliveryPlan, DeliveryTarget, GameMessage, PresenceResolver, PresenceSyncPlan,
};

/// Async tell-target resolution (IRC nick vs Slack user id).
#[async_trait::async_trait]
pub trait TellResolver: Send + Sync {
    async fn resolve<P: Persistence + Clone>(
        &self,
        manager: &SessionManager<P>,
        identity: &str,
    ) -> Option<String>;

    fn actor_matches(&self, actor_id: &str, resolved: &str) -> bool;
}

/// Map a transport-neutral command result to a delivery plan.
pub struct MessageRouter<'a, P: Persistence + Clone> {
    pub mode: PlayMode,
    pub actor_id: &'a str,
    pub manager: &'a SessionManager<P>,
}

impl<'a, P: Persistence + Clone> MessageRouter<'a, P> {
    pub fn new(mode: PlayMode, actor_id: &'a str, manager: &'a SessionManager<P>) -> Self {
        Self {
            mode,
            actor_id,
            manager,
        }
    }

    pub async fn plan_command_deliveries<R: TellResolver>(
        &self,
        result: CommandResult,
        presence: &dyn PresenceResolver,
        tell_resolver: &R,
        actor_label: &str,
    ) -> DeliveryPlan {
        let mut plan = DeliveryPlan {
            persist: result.persist_world,
            ..Default::default()
        };

        let social_for_broadcast = result.social.as_ref();

        self.route_actor_feedback(
            &mut plan,
            &result.lines_to_actor,
            social_for_broadcast,
            presence,
        )
        .await;

        if let Some(social) = result.social {
            self.route_social(&mut plan, social, presence, tell_resolver)
                .await;
        }

        if let Some(movement) = result.movement {
            self.route_movement(
                &mut plan,
                movement.old_room.as_ref(),
                movement.new_room.as_ref(),
                &movement.lines,
                presence,
                actor_label,
            )
            .await;
        }

        plan
    }

    async fn route_actor_feedback(
        &self,
        plan: &mut DeliveryPlan,
        lines: &[String],
        social: Option<&SocialIntent>,
        presence: &dyn PresenceResolver,
    ) {
        if self.mode.is_story() {
            for line in lines {
                if !line.trim().is_empty() {
                    plan.actor_plain(line.clone());
                }
            }
            return;
        }

        for line in lines {
            if is_open_private_actor_line(line) {
                plan.actor_plain(line.clone());
            }
        }

        if let Some(body) = open_channel_broadcast_body(social, lines) {
            let Some((speaker, room_id, room_name)) =
                actor_place_context(self.manager, self.actor_id).await
            else {
                return;
            };
            plan.shared(
                presence.speech_presence(&room_id),
                GameMessage::OpenContext {
                    speaker,
                    room: room_name,
                    body,
                },
            );
        }
    }

    async fn route_social<R: TellResolver>(
        &self,
        plan: &mut DeliveryPlan,
        social: SocialIntent,
        presence: &dyn PresenceResolver,
        tell_resolver: &R,
    ) {
        match social {
            SocialIntent::Say {
                room_id,
                speaker_name,
                text,
            } => {
                let msg = GameMessage::Say {
                    speaker: speaker_name,
                    text,
                };
                if self.mode.is_story() {
                    plan.push(DeliveryTarget::Actor, msg.clone());
                    let audience = connected_speech_audience_async(
                        self.manager,
                        &room_id,
                        Some(self.actor_id),
                        self.mode,
                    )
                    .await;
                    if !audience.is_empty() {
                        plan.push(DeliveryTarget::RoomAudience(audience), msg.clone());
                    }
                }
                plan.shared(presence.speech_presence(&room_id), msg);
            }
            SocialIntent::Emote {
                room_id,
                speaker_name,
                text,
            } => {
                let msg = GameMessage::Emote {
                    speaker: speaker_name,
                    text,
                };
                if self.mode.is_story() {
                    plan.push(DeliveryTarget::Actor, msg.clone());
                    let audience = connected_speech_audience_async(
                        self.manager,
                        &room_id,
                        Some(self.actor_id),
                        self.mode,
                    )
                    .await;
                    if !audience.is_empty() {
                        plan.push(DeliveryTarget::RoomAudience(audience), msg.clone());
                    }
                }
                plan.shared(presence.speech_presence(&room_id), msg);
            }
            SocialIntent::Tell {
                target_identity,
                speaker_name,
                text,
            } => {
                let Some(resolved) = tell_resolver.resolve(self.manager, &target_identity).await
                else {
                    plan.deliveries.clear();
                    plan.actor_plain(format!("{target_identity} is not connected."));
                    plan.persist = false;
                    return;
                };
                if tell_resolver.actor_matches(self.actor_id, &resolved) {
                    plan.deliveries.clear();
                    plan.actor_plain("You talk to yourself.".to_string());
                    plan.persist = false;
                    return;
                }
                plan.push(
                    DeliveryTarget::Actor,
                    GameMessage::TellSent {
                        to: target_identity.clone(),
                        text: text.clone(),
                    },
                );
                plan.push(
                    DeliveryTarget::User(resolved),
                    GameMessage::Tell {
                        from: speaker_name,
                        text,
                    },
                );
            }
        }
    }

    async fn route_movement(
        &self,
        plan: &mut DeliveryPlan,
        old_room: Option<&ObjectId>,
        new_room: Option<&ObjectId>,
        lines: &[String],
        presence: &dyn PresenceResolver,
        actor_label: &str,
    ) {
        for line in lines {
            if !line.trim().is_empty() {
                plan.actor_plain(line.clone());
            }
        }

        let (Some(old_id), Some(new_id)) = (old_room, new_room) else {
            return;
        };
        if old_id == new_id {
            return;
        }

        if self.mode.is_story() {
            plan.presence_sync = Some(PresenceSyncPlan {
                actor: actor_label.to_string(),
                join: vec![presence.speech_presence(new_id)],
                part: vec![presence.speech_presence(old_id)],
            });
            plan.actor_plain(presence.ic_join_notice(new_id));
            if presence.story_movement_visibility() {
                self.route_story_movement_visibility(plan, old_id, new_id, presence, actor_label)
                    .await;
            }
        } else {
            self.route_open_movement_visibility(plan, new_id, actor_label, presence)
                .await;
        }
    }

    async fn route_story_movement_visibility(
        &self,
        plan: &mut DeliveryPlan,
        old_room: &ObjectId,
        new_room: &ObjectId,
        presence: &dyn PresenceResolver,
        actor_label: &str,
    ) {
        let departure = GameMessage::Departure {
            speaker: actor_label.to_string(),
        };
        let arrival = GameMessage::Arrival {
            speaker: actor_label.to_string(),
        };

        let old_audience = connected_speech_audience_async(
            self.manager,
            old_room,
            Some(self.actor_id),
            self.mode,
        )
        .await;
        if !old_audience.is_empty() {
            plan.push(DeliveryTarget::RoomAudience(old_audience), departure.clone());
        }
        plan.shared(presence.speech_presence(old_room), departure);

        let new_audience = connected_speech_audience_async(
            self.manager,
            new_room,
            Some(self.actor_id),
            self.mode,
        )
        .await;
        if !new_audience.is_empty() {
            plan.push(DeliveryTarget::RoomAudience(new_audience), arrival.clone());
        }
        plan.shared(presence.speech_presence(new_room), arrival);
    }

    async fn route_open_movement_visibility(
        &self,
        plan: &mut DeliveryPlan,
        new_room: &ObjectId,
        actor_label: &str,
        presence: &dyn PresenceResolver,
    ) {
        let shared = presence.speech_presence(new_room);
        plan.shared(
            shared.clone(),
            GameMessage::Departure {
                speaker: actor_label.to_string(),
            },
        );
        plan.shared(
            shared,
            GameMessage::Arrival {
                speaker: actor_label.to_string(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::CommandResult;
    use crate::gateway::SessionManager;
    use crate::object::{Object, ObjectId, PermissionFlags};
    use crate::persistence::SqlitePersistence;
    use std::collections::HashMap;

    struct TestPresence;

    impl PresenceResolver for TestPresence {
        fn speech_presence(&self, room: &ObjectId) -> String {
            format!("presence:{}", room.as_str())
        }

        fn ic_join_notice(&self, room: &ObjectId) -> String {
            format!("Join {}", room.as_str())
        }
    }

    struct NoTell;

    #[async_trait::async_trait]
    impl TellResolver for NoTell {
        async fn resolve<P: Persistence + Clone>(
            &self,
            _manager: &SessionManager<P>,
            _identity: &str,
        ) -> Option<String> {
            None
        }

        fn actor_matches(&self, _actor_id: &str, _resolved: &str) -> bool {
            false
        }
    }

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

    #[tokio::test]
    async fn story_mode_routes_command_output_to_actor() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let room = ObjectId::new("room:void-001");
        let mut hero = bare("player:hero-001", "Alice");
        hero.location = Some(room.clone());
        let place = bare("room:void-001", "The Void");
        persistence.save_object(&hero).await.unwrap();
        persistence.save_object(&place).await.unwrap();

        let mut manager = SessionManager::open(persistence, Default::default())
            .await
            .unwrap();
        manager
            .login("alice", hero.id.clone(), Some(room))
            .await
            .unwrap();

        let router = MessageRouter::new(PlayMode::Story, "alice", &manager);
        let plan = router
            .plan_command_deliveries(
                CommandResult {
                    lines_to_actor: vec!["You see a void.".to_string()],
                    ..Default::default()
                },
                &TestPresence,
                &NoTell,
                "Alice",
            )
            .await;

        assert!(plan.deliveries.iter().any(|d| {
            d.target == DeliveryTarget::Actor
                && matches!(&d.message, GameMessage::Plain(s) if s.contains("void"))
        }));
        assert!(!plan
            .deliveries
            .iter()
            .any(|d| matches!(d.target, DeliveryTarget::SharedPresence(_))));
    }

    #[tokio::test]
    async fn open_mode_routes_public_output_to_shared_presence() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let room = ObjectId::new("room:void-001");
        let mut hero = bare("player:hero-001", "Alice");
        hero.location = Some(room.clone());
        let place = bare("room:void-001", "The Void");
        persistence.save_object(&hero).await.unwrap();
        persistence.save_object(&place).await.unwrap();

        let mut manager = SessionManager::open(persistence, Default::default())
            .await
            .unwrap();
        manager
            .login("alice", hero.id.clone(), Some(room))
            .await
            .unwrap();

        let router = MessageRouter::new(PlayMode::Open, "alice", &manager);
        let plan = router
            .plan_command_deliveries(
                CommandResult {
                    lines_to_actor: vec!["You pick up a sword.".to_string()],
                    ..Default::default()
                },
                &TestPresence,
                &NoTell,
                "Alice",
            )
            .await;

        assert!(plan.deliveries.iter().any(|d| {
            matches!(
                (&d.target, &d.message),
                (
                    DeliveryTarget::SharedPresence(_),
                    GameMessage::OpenContext { body, .. }
                ) if body.contains("pick up")
            )
        }));
        assert!(!plan
            .deliveries
            .iter()
            .any(|d| matches!(d.target, DeliveryTarget::Actor)));
    }
}