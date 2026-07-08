//! M5 acceptance scenarios — multi-user IRC flows end-to-end.

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use tokio::sync::Mutex;

    use crate::gateway::SessionManager;
    use crate::irc::{dispatch_command, IrcBot, IrcConfig, IrcMessage, MockTransport};
    use crate::transport::OutgoingAction;
    use crate::mudl::{AnatomyRegistry, BodySlotDef, CreatureDef, SlotType};
    use crate::object::{Object, ObjectId, PermissionFlags};
    use crate::persistence::{Persistence, SqlitePersistence};

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
        IrcConfig,
        Arc<MockTransport>,
        IrcBot<SqlitePersistence, MockTransport>,
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

        let mut sword = bare("item:sword-001", "Rusty Sword");
        sword.location = Some(void.clone());

        persistence.save_object(&hero1).await.unwrap();
        persistence.save_object(&hero2).await.unwrap();
        persistence.save_object(&void_room).await.unwrap();
        persistence.save_object(&north_room).await.unwrap();
        persistence.save_object(&sword).await.unwrap();

        let mut manager = SessionManager::open(persistence.clone(), human_anatomy())
            .await
            .unwrap();
        manager
            .login("alice", ObjectId::new("player:hero-001"), Some(void.clone()))
            .await
            .unwrap();
        manager
            .login("bob", ObjectId::new("player:hero-002"), Some(void))
            .await
            .unwrap();

        let config = IrcConfig::default();
        let transport = Arc::new(MockTransport::new());
        let bot = IrcBot::new(manager, Arc::clone(&transport), config.clone());
        let manager = bot.manager();
        (persistence, manager, config, transport, bot)
    }

    #[tokio::test]
    async fn login_by_explicit_player_id() {
        let (persistence, manager, config, ..) = fixture().await;
        manager.lock().await.logout("alice").await.unwrap();

        let outcome = dispatch_command(
            Arc::clone(&manager),
            "alice",
            "login player:hero-001",
            &config,
        )
        .await;
        assert!(outcome.to_sender.iter().any(|l| l.contains("Welcome")));
        assert!(manager.lock().await.is_connected("alice"));

        let stored = persistence
            .load_object(&ObjectId::new("player:hero-001"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.name, "Alice");
    }

    #[tokio::test]
    async fn shorthand_movement_without_go_verb() {
        let (_persistence, manager, config, ..) = fixture().await;
        let outcome = dispatch_command(Arc::clone(&manager), "alice", "north", &config).await;
        assert!(outcome.to_sender.iter().any(|l| l.contains("north")));
        let loc = manager
            .lock()
            .await
            .with_session("alice", |s| s.current_location().cloned())
            .await
            .unwrap();
        assert_eq!(loc.as_ref().map(|id| id.as_str()), Some("room:north-001"));
    }

    #[tokio::test]
    async fn whisper_alias_delivers_private_message() {
        let (_persistence, manager, config, ..) = fixture().await;
        let outcome =
            dispatch_command(manager, "alice", "whisper bob meet at north", &config).await;
        assert_eq!(outcome.private.len(), 1);
        assert_eq!(outcome.private[0].0, "bob");
        assert!(outcome.private[0].1.contains("meet at north"));
    }

    #[tokio::test]
    async fn apostrophe_say_shorthand() {
        let (_persistence, _manager, _config, transport, bot) = fixture().await;
        transport.clear();
        bot.handle_input("alice", "' hello void").await.unwrap();
        assert!(transport
            .privmsgs_to("bob")
            .iter()
            .any(|l| l.contains("hello void")));
    }

    #[tokio::test]
    async fn colon_emote_shorthand() {
        let (_persistence, _manager, _config, transport, bot) = fixture().await;
        transport.clear();
        bot.handle_input("alice", ": waves.").await.unwrap();
        assert!(transport
            .privmsgs_to("bob")
            .iter()
            .any(|l| l.contains("waves.")));
    }

    #[tokio::test]
    async fn ooc_on_world_channel_requires_login() {
        let (_persistence, manager, _config, transport, bot) = fixture().await;
        manager.lock().await.logout("alice").await.unwrap();
        transport.clear();

        bot.handle_message(IrcMessage::Privmsg {
            from: "stranger".to_string(),
            target: "#mudl".to_string(),
            text: "anyone?".to_string(),
        })
        .await
        .unwrap();

        assert!(transport.recorded().iter().any(|entry| {
            matches!(
                entry,
                OutgoingAction::Notice { recipient, text }
                    if recipient == "stranger" && text.contains("not logged in")
            )
        }));
        assert!(transport.channel_messages("#mudl").is_empty());
    }

    #[tokio::test]
    async fn go_syncs_room_channels_for_bot_transport() {
        let (_persistence, _manager, _config, transport, bot) = fixture().await;
        transport.clear();
        bot.handle_input("alice", "go north").await.unwrap();

        assert!(transport.recorded().iter().any(|entry| {
            matches!(
                entry,
                OutgoingAction::Join { presence }
                    if presence == "#mudl-north-001"
            )
        }));
        assert!(transport.recorded().iter().any(|entry| {
            matches!(
                entry,
                OutgoingAction::Leave { presence, .. }
                    if presence == "#mudl-void-001"
            )
        }));
    }

    #[tokio::test]
    async fn take_updates_inventory_for_actor_only() {
        let (_persistence, manager, config, ..) = fixture().await;
        dispatch_command(Arc::clone(&manager), "alice", "take rusty", &config).await;

        let alice_inv = manager
            .lock()
            .await
            .with_session("alice", |s| {
                s.with_world(|world, player| {
                    world
                        .object(player.actor_id())
                        .and_then(|p| p.body_slot_item("right_hand").map(|id| id.clone()))
                })
            })
            .await
            .unwrap();
        let bob_inv = manager
            .lock()
            .await
            .with_session("bob", |s| {
                s.with_world(|world, player| {
                    world
                        .object(player.actor_id())
                        .and_then(|p| p.body_slot_item("right_hand").map(|id| id.clone()))
                })
            })
            .await
            .unwrap();

        assert_eq!(alice_inv.as_ref().map(|id| id.as_str()), Some("item:sword-001"));
        assert!(bob_inv.is_none());
    }
}