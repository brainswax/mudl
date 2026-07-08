//! Multi-user integration tests — shared world, visibility, concurrency (M5).

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use tokio::sync::Mutex;

    use crate::gateway::SessionManager;
    use crate::irc::{dispatch_command, IrcBot, IrcConfig, MockTransport};
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

    fn player(id: &str, name: &str, room: &ObjectId) -> Object {
        let mut obj = bare(id, name);
        obj.set_property_string("body_plan", "human");
        obj.location = Some(room.clone());
        obj
    }

    async fn three_player_world() -> (SqlitePersistence, SessionManager<SqlitePersistence>) {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let void = ObjectId::new("room:void-001");
        let north = ObjectId::new("room:north-001");

        let hero1 = player("player:hero-001", "Alice", &void);
        let hero2 = player("player:hero-002", "Bob", &void);
        let hero3 = player("player:hero-003", "Scout", &north);

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
        persistence.save_object(&hero3).await.unwrap();
        persistence.save_object(&void_room).await.unwrap();
        persistence.save_object(&north_room).await.unwrap();
        persistence.save_object(&sword).await.unwrap();

        let manager = SessionManager::open(persistence.clone(), human_anatomy())
            .await
            .unwrap();
        (persistence, manager)
    }

    async fn login_trio(manager: &mut SessionManager<SqlitePersistence>) {
        let void = ObjectId::new("room:void-001");
        let north = ObjectId::new("room:north-001");
        manager
            .login("alice", ObjectId::new("player:hero-001"), Some(void.clone()))
            .await
            .unwrap();
        manager
            .login("bob", ObjectId::new("player:hero-002"), Some(void))
            .await
            .unwrap();
        manager
            .login("scout", ObjectId::new("player:hero-003"), Some(north))
            .await
            .unwrap();
    }

    async fn sword_holder(manager: &SessionManager<SqlitePersistence>, nick: &str) -> bool {
        manager
            .with_session(nick, |session| {
                session.with_world(|world, player| {
                    world
                        .object(&ObjectId::new("item:sword-001"))
                        .and_then(|o| o.location.as_ref())
                        .map(|loc| loc == player.actor_id())
                        .unwrap_or(false)
                })
            })
            .await
            .unwrap_or(false)
    }

    #[tokio::test]
    async fn movement_by_one_player_visible_in_shared_world() {
        let (_persistence, mut manager) = three_player_world().await;
        login_trio(&mut manager).await;

        manager
            .with_session("alice", |session| session.go("north"))
            .await
            .unwrap()
            .unwrap();

        let hero1_loc = manager
            .with_session("bob", |session| {
                session.with_world(|world, _| {
                    world
                        .object(&ObjectId::new("player:hero-001"))
                        .and_then(|p| p.location.as_ref().map(|id| id.as_str().to_string()))
                })
            })
            .await
            .unwrap();
        let bob_loc = manager
            .with_session("bob", |session| session.current_location().cloned())
            .await
            .unwrap();
        assert_eq!(hero1_loc, Some("room:north-001".to_string()));
        assert_eq!(bob_loc.as_ref().map(|id| id.as_str()), Some("room:void-001"));
    }

    #[tokio::test]
    async fn say_does_not_cross_room_boundaries() {
        let (_persistence, mut manager) = three_player_world().await;
        let config = IrcConfig::default();
        login_trio(&mut manager).await;
        let manager = Arc::new(Mutex::new(manager));

        dispatch_command(Arc::clone(&manager), "alice", "go north", &config).await;

        let outcome =
            dispatch_command(manager, "alice", "say anyone there?", &config).await;

        let audience: Vec<_> = outcome
            .room_audience
            .iter()
            .flat_map(|d| d.audience.iter().cloned())
            .collect();
        assert!(!audience.contains(&"bob".to_string()));
        assert!(audience.contains(&"scout".to_string()));
    }

    #[tokio::test]
    async fn emote_does_not_reach_distant_players() {
        let (_persistence, mut manager) = three_player_world().await;
        let config = IrcConfig::default();
        login_trio(&mut manager).await;
        let manager = Arc::new(Mutex::new(manager));

        dispatch_command(Arc::clone(&manager), "alice", "go north", &config).await;
        let outcome = dispatch_command(manager, "alice", "emote waves.", &config).await;

        let audience: Vec<_> = outcome
            .room_audience
            .iter()
            .flat_map(|d| d.audience.iter().cloned())
            .collect();
        assert!(!audience.contains(&"bob".to_string()));
    }

    #[tokio::test]
    async fn tell_is_private_without_room_broadcast() {
        let (_persistence, mut manager) = three_player_world().await;
        let config = IrcConfig::default();
        login_trio(&mut manager).await;
        let manager = Arc::new(Mutex::new(manager));

        let outcome = dispatch_command(
            manager,
            "alice",
            "tell scout meet me north",
            &config,
        )
        .await;

        assert!(outcome.room_audience.is_empty());
        assert_eq!(outcome.private.len(), 1);
        assert_eq!(outcome.private[0].0, "scout");
        assert!(outcome.private[0].1.contains("meet me north"));
    }

    #[tokio::test]
    async fn take_by_one_player_updates_shared_room_for_other() {
        let (_persistence, mut manager) = three_player_world().await;
        let config = IrcConfig::default();
        login_trio(&mut manager).await;
        let manager = Arc::new(Mutex::new(manager));

        let take = dispatch_command(Arc::clone(&manager), "alice", "take rusty", &config).await;
        assert!(take.to_sender.iter().any(|l| l.contains("pick up")));

        let sword_in_void = manager.lock().await.with_session("bob", |session| {
            session.with_world(|world, _| {
                world
                    .object(&ObjectId::new("item:sword-001"))
                    .and_then(|o| o.location.as_ref().map(|id| id.as_str().to_string()))
            })
        }).await.unwrap();
        assert_eq!(sword_in_void, Some("player:hero-001".to_string()));
    }

    #[tokio::test]
    async fn concurrent_go_moves_both_players() {
        let (_persistence, manager) = three_player_world().await;
        let config = IrcConfig::default();
        let manager = Arc::new(Mutex::new(manager));
        {
            let mut guard = manager.lock().await;
            login_trio(&mut guard).await;
        }

        let north = ObjectId::new("room:north-001");
        let m1 = Arc::clone(&manager);
        let m2 = Arc::clone(&manager);
        let c = config.clone();
        let c_alice = c.clone();

        let alice = tokio::spawn(async move {
            dispatch_command(m1, "alice", "go north", &c_alice).await
        });
        let bob = tokio::spawn(async move {
            dispatch_command(m2, "bob", "north", &c).await
        });

        let (a, b) = tokio::join!(alice, bob);
        assert!(a.unwrap().to_sender.iter().any(|l| l.contains("north")));
        assert!(b.unwrap().to_sender.iter().any(|l| l.contains("north")));

        let guard = manager.lock().await;
        let alice_loc = guard
            .with_session("alice", |session| session.current_location().cloned())
            .await
            .unwrap();
        let bob_loc = guard
            .with_session("bob", |session| session.current_location().cloned())
            .await
            .unwrap();
        assert_eq!(alice_loc.as_ref(), Some(&north));
        assert_eq!(bob_loc.as_ref(), Some(&north));
    }

    #[tokio::test]
    async fn concurrent_take_only_one_player_gets_sword() {
        let (_persistence, manager) = three_player_world().await;
        let config = IrcConfig::default();
        let manager = Arc::new(Mutex::new(manager));
        {
            let mut guard = manager.lock().await;
            login_trio(&mut guard).await;
        }

        let m1 = Arc::clone(&manager);
        let m2 = Arc::clone(&manager);
        let c = config.clone();
        let c_alice = c.clone();

        let alice = tokio::spawn(async move {
            dispatch_command(m1, "alice", "take rusty", &c_alice).await
        });
        let bob = tokio::spawn(async move {
            dispatch_command(m2, "bob", "take rusty", &c).await
        });

        let (a, b) = tokio::join!(alice, bob);
        let a_ok = a.unwrap().to_sender.iter().any(|l| l.contains("pick up"));
        let b_ok = b.unwrap().to_sender.iter().any(|l| l.contains("pick up"));
        assert!(a_ok ^ b_ok);

        let guard = manager.lock().await;
        assert_eq!(
            sword_holder(&guard, "alice").await as u8 + sword_holder(&guard, "bob").await as u8,
            1
        );
    }

    #[tokio::test]
    async fn logout_one_player_leaves_other_connected() {
        let (_persistence, mut manager) = three_player_world().await;
        login_trio(&mut manager).await;

        manager.logout("alice").await.unwrap();

        assert_eq!(manager.connection_count(), 2);
        assert!(manager.is_connected("bob"));
        assert!(manager.is_connected("scout"));
        assert!(!manager.is_connected("alice"));
    }

    #[tokio::test]
    async fn mixed_case_nick_receives_replies() {
        let (_persistence, mut manager) = three_player_world().await;
        let config = IrcConfig::default();

        manager
            .login(
                "alice",
                ObjectId::new("player:hero-001"),
                Some(ObjectId::new("room:void-001")),
            )
            .await
            .unwrap();

        let manager = Arc::new(Mutex::new(manager));
        let outcome = dispatch_command(manager, "Alice", "look", &config).await;

        assert_eq!(outcome.sender, "alice");
        assert!(outcome
            .to_sender
            .iter()
            .any(|l| l.contains("featureless void")));
    }

    #[tokio::test]
    async fn bot_cross_room_say_visibility_via_transport() {
        let (_persistence, mut manager) = three_player_world().await;
        let config = IrcConfig::default();
        login_trio(&mut manager).await;

        let transport = Arc::new(MockTransport::new());
        let bot = IrcBot::new(manager, Arc::clone(&transport), config);

        bot.handle_input("alice", "go north").await.unwrap();
        transport.clear();
        bot.handle_input("alice", "say north only").await.unwrap();

        assert!(transport.privmsgs_to("scout").iter().any(|l| l.contains("north only")));
        assert!(transport.privmsgs_to("bob").is_empty());
    }

    #[tokio::test]
    async fn bot_concurrent_handle_input_runs_in_parallel() {
        let (_persistence, mut manager) = three_player_world().await;
        let config = IrcConfig::default();
        login_trio(&mut manager).await;

        let transport = Arc::new(MockTransport::new());
        let bot = Arc::new(IrcBot::new(manager, Arc::clone(&transport), config));

        let b1 = Arc::clone(&bot);
        let b2 = Arc::clone(&bot);
        let t1 = tokio::spawn(async move { b1.handle_input("alice", "go north").await });
        let t2 = tokio::spawn(async move { b2.handle_input("bob", "say hello").await });
        let (a, b) = tokio::join!(t1, t2);

        a.unwrap().unwrap();
        let bob_out = b.unwrap().unwrap();
        assert!(bob_out.to_sender.iter().any(|l| l.contains("hello")));

        let manager = bot.manager();
        let guard = manager.lock().await;
        let location = guard
            .with_session("alice", |session| session.current_location().cloned())
            .await
            .unwrap();
        assert_eq!(location.as_ref().map(|id| id.as_str()), Some("room:north-001"));
    }
}