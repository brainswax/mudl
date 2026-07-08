//! M6 acceptance scenarios — multi-user Slack session flows end-to-end.

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use tokio::sync::Mutex;

    use crate::gateway::{LoginAuthPolicy, SessionManager};
    use crate::mudl::{AnatomyRegistry, BodySlotDef, CreatureDef, SlotType};
    use crate::object::{Object, ObjectId, PermissionFlags};
    use crate::persistence::{Persistence, SqlitePersistence};
    use crate::slack::{
        dispatch_command, SlackBot, SlackConfig, SlackEventBody, SlackMessageEvent,
    };
    use crate::transport::{MockTransport, OutgoingAction};

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

    fn human_anatomy() -> AnatomyRegistry {
        let mut anatomy = AnatomyRegistry::default();
        anatomy.creatures.insert(
            "human".to_string(),
            CreatureDef {
                name: "human".to_string(),
                slots: vec![
                    BodySlotDef {
                        name: "left_hand".to_string(),
                        capacity: 1,
                        slot_type: SlotType::Grasp,
                        hands: 1,
                        effect: None,
                    },
                    BodySlotDef {
                        name: "right_hand".to_string(),
                        capacity: 1,
                        slot_type: SlotType::Grasp,
                        hands: 1,
                        effect: None,
                    },
                ],
                max_health: 100,
                base_max_weight: Some(100),
                stats: HashMap::new(),
                skills: HashMap::new(),
            },
        );
        anatomy
    }

    async fn fixture() -> (
        SqlitePersistence,
        Arc<Mutex<SessionManager<SqlitePersistence>>>,
        SlackConfig,
        Arc<MockTransport>,
        SlackBot<SqlitePersistence, MockTransport>,
    ) {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let void = ObjectId::new("room:void-001");
        let north = ObjectId::new("room:north-001");

        let hero1 = {
            let mut obj = bare("player:hero-001", "Alice");
            obj.set_property_string("body_plan", "human");
            obj.location = Some(void.clone());
            obj
        };
        let hero2 = {
            let mut obj = bare("player:hero-002", "Bob");
            obj.set_property_string("body_plan", "human");
            obj.location = Some(void.clone());
            obj
        };

        let mut void_room = bare("room:void-001", "The Void");
        void_room.set_property_string("description", "A featureless void.");
        void_room.set_property_map(
            "exits",
            HashMap::from([("north".to_string(), north.clone())]),
        );
        let mut north_room = bare("room:north-001", "North Passage");
        north_room.set_property_string("description", "A narrow passage north.");
        north_room.add_exit("south", void.clone());

        persistence.save_object(&hero1).await.unwrap();
        persistence.save_object(&hero2).await.unwrap();
        persistence.save_object(&void_room).await.unwrap();
        persistence.save_object(&north_room).await.unwrap();

        let manager = SessionManager::open(persistence.clone(), human_anatomy())
            .await
            .unwrap();
        let config = SlackConfig {
            world_channel: "C_WORLD".to_string(),
            ..SlackConfig::default()
        };
        let transport = Arc::new(MockTransport::new());
        let bot = SlackBot::new(manager, Arc::clone(&transport), config.clone());
        let manager = bot.manager();
        (persistence, manager, config, transport, bot)
    }

    #[tokio::test]
    async fn login_binds_slack_user_id_to_player_session() {
        let (_persistence, manager, config, ..) = fixture().await;
        let outcome = dispatch_command(
            Arc::clone(&manager),
            "U_ALICE",
            "D_ALICE",
            "login player:hero-001",
            &config,
        )
        .await;
        assert!(outcome.to_sender.iter().any(|l| l.contains("Welcome")));
        assert!(manager.lock().await.is_connected("U_ALICE"));
        assert_eq!(manager.lock().await.connection_count(), 1);
    }

    #[tokio::test]
    async fn logout_clears_session_and_persists_location() {
        let (persistence, manager, config, ..) = fixture().await;
        dispatch_command(
            Arc::clone(&manager),
            "U_ALICE",
            "D_ALICE",
            "login player:hero-001",
            &config,
        )
        .await;
        dispatch_command(Arc::clone(&manager), "U_ALICE", "D_ALICE", "go north", &config).await;

        let outcome =
            dispatch_command(manager, "U_ALICE", "D_ALICE", "quit", &config).await;
        assert!(outcome.to_sender.iter().any(|l| l.contains("Goodbye")));

        let stored = persistence
            .load_object(&ObjectId::new("player:hero-001"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            stored.location.as_ref().map(|id| id.as_str()),
            Some("room:north-001")
        );
    }

    #[tokio::test]
    async fn reconnect_after_logout_allows_same_actor() {
        let (_persistence, manager, config, ..) = fixture().await;
        dispatch_command(
            Arc::clone(&manager),
            "U_ALICE",
            "D_ALICE",
            "login player:hero-001",
            &config,
        )
        .await;
        dispatch_command(Arc::clone(&manager), "U_ALICE", "D_ALICE", "quit", &config).await;

        let outcome = dispatch_command(
            manager,
            "U_ALICE",
            "D_ALICE",
            "login player:hero-001",
            &config,
        )
        .await;
        assert!(outcome.to_sender.iter().any(|l| l.contains("Welcome")));
    }

    #[tokio::test]
    async fn token_login_with_slack_identity_binding() {
        let (_persistence, manager, mut config, ..) = fixture().await;
        config.login_auth = LoginAuthPolicy {
            require_auth: true,
            identity_bindings: HashMap::from([(
                "u_alice".to_string(),
                ObjectId::new("player:hero-001"),
            )]),
            env_tokens: HashMap::from([(
                "player:hero-001".to_string(),
                "sekrit".to_string(),
            )]),
            ..LoginAuthPolicy::permissive()
        };

        let outcome = dispatch_command(
            Arc::clone(&manager),
            "U_ALICE",
            "D_ALICE",
            "login sekrit",
            &config,
        )
        .await;
        assert!(outcome.to_sender.iter().any(|l| l.contains("Welcome")));
        assert!(manager.lock().await.is_connected("U_ALICE"));
    }

    #[tokio::test]
    async fn bot_records_dm_channel_for_ooc_relay() {
        let (_persistence, _manager, _config, transport, bot) = fixture().await;
        bot.handle_input("U_ALICE", "D_ALICE", "login player:hero-001")
            .await
            .unwrap();
        bot.handle_input("U_BOB", "D_BOB", "login player:hero-002")
            .await
            .unwrap();
        transport.clear();

        bot.handle_event(SlackEventBody::Message(SlackMessageEvent {
                user: "U_ALICE".to_string(),
                text: "brb dinner".to_string(),
                channel: "C_WORLD".to_string(),
                channel_type: Some("channel".to_string()),
                thread_ts: None,
                ts: None,
        }))
        .await
        .unwrap();

        assert!(transport
            .direct_messages_to("D_BOB")
            .iter()
            .any(|l| l.contains("brb dinner")));
        assert_eq!(
            bot.slack_sessions()
                .lock()
                .await
                .reply_channel("U_BOB"),
            Some("D_BOB")
        );
    }

    #[tokio::test]
    async fn ooc_on_world_channel_requires_login() {
        let (_persistence, _manager, _config, transport, bot) = fixture().await;
        transport.clear();

        bot.handle_event(SlackEventBody::Message(SlackMessageEvent {
                user: "U_STRANGER".to_string(),
                text: "anyone?".to_string(),
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
                    if recipient == "C_WORLD:notice:U_STRANGER" =>
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
}