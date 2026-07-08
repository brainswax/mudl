//! Load and concurrency stress tests for multi-user session handling (M5).

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use tokio::sync::Mutex;
    use tokio::time::timeout;

    use crate::gateway::SessionManager;
    use crate::irc::{dispatch_command, IrcBot, IrcConfig, MockTransport};
    use crate::mudl::{AnatomyRegistry, BodySlotDef, CreatureDef, SlotType};
    use crate::object::{Object, ObjectId, PermissionFlags};
    use crate::persistence::{Persistence, SqlitePersistence};

    const LOAD_TIMEOUT: Duration = Duration::from_secs(5);
    const MIXED_COMMAND_BUDGET: Duration = Duration::from_millis(1500);

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

    async fn load_world(
        player_count: usize,
    ) -> (SqlitePersistence, SessionManager<SqlitePersistence>, IrcConfig) {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let void = ObjectId::new("room:void-001");
        let north = ObjectId::new("room:north-001");

        let mut void_room = bare("room:void-001", "The Void");
        void_room.set_property_string("description", "A featureless void.");
        void_room.set_property_map(
            "exits",
            HashMap::from([("north".to_string(), north.clone())]),
        );
        let mut north_room = bare("room:north-001", "North Passage");
        north_room.set_property_string("description", "A narrow passage north.");
        north_room.add_exit("south", void.clone());
        persistence.save_object(&void_room).await.unwrap();
        persistence.save_object(&north_room).await.unwrap();

        for i in 0..player_count {
            let id = format!("player:hero-{i:03}");
            let mut hero = bare(&id, &format!("Hero {i}"));
            hero.set_property_string("body_plan", "human");
            hero.location = Some(void.clone());
            persistence.save_object(&hero).await.unwrap();
        }

        let mut manager = SessionManager::open(persistence.clone(), human_anatomy())
            .await
            .unwrap();
        let config = IrcConfig::default();

        for i in 0..player_count {
            let nick = format!("player{i}");
            let actor = ObjectId::new(&format!("player:hero-{i:03}"));
            manager
                .login(&nick, actor, Some(void.clone()))
                .await
                .unwrap();
        }

        (persistence, manager, config)
    }

    #[tokio::test]
    async fn concurrent_look_from_many_players_completes_without_deadlock() {
        let (_persistence, manager, config) = load_world(8).await;
        let manager = Arc::new(Mutex::new(manager));
        let started = Instant::now();

        let tasks: Vec<_> = (0..8)
            .map(|i| {
                let mgr = Arc::clone(&manager);
                let cfg = config.clone();
                let nick = format!("player{i}");
                tokio::spawn(async move {
                    dispatch_command(mgr, &nick, "look", &cfg).await
                })
            })
            .collect();

        let result = timeout(LOAD_TIMEOUT, async {
            for task in tasks {
                let outcome = task.await.expect("task join");
                assert!(outcome
                    .to_sender
                    .iter()
                    .any(|line| line.contains("featureless void")));
            }
        })
        .await;

        assert!(result.is_ok(), "concurrent look deadlocked or timed out");
        assert!(
            started.elapsed() < LOAD_TIMEOUT,
            "concurrent look exceeded budget: {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn concurrent_mixed_commands_finish_within_latency_budget() {
        let (_persistence, manager, config) = load_world(4).await;
        let manager = Arc::new(Mutex::new(manager));
        let started = Instant::now();

        let commands = [
            ("player0", "look"),
            ("player1", "inventory"),
            ("player2", "say hello all"),
            ("player3", "go north"),
        ];

        let tasks: Vec<_> = commands
            .into_iter()
            .map(|(nick, cmd)| {
                let mgr = Arc::clone(&manager);
                let cfg = config.clone();
                tokio::spawn(async move {
                    dispatch_command(mgr, nick, cmd, &cfg).await
                })
            })
            .collect();

        let result = timeout(MIXED_COMMAND_BUDGET, async {
            for task in tasks {
                task.await.expect("task join");
            }
        })
        .await;

        assert!(result.is_ok(), "mixed commands timed out — possible lock contention");
        assert!(
            started.elapsed() < MIXED_COMMAND_BUDGET,
            "mixed commands too slow: {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn bot_parallel_handle_input_from_all_connections() {
        let (_persistence, manager, config) = load_world(6).await;
        assert_eq!(manager.connection_count(), 6);
        let transport = Arc::new(MockTransport::new());
        let bot = Arc::new(IrcBot::new(manager, Arc::clone(&transport), config));

        let started = Instant::now();
        let tasks: Vec<_> = (0..6)
            .map(|i| {
                let bot = Arc::clone(&bot);
                let nick = format!("player{i}");
                tokio::spawn(async move { bot.handle_input(&nick, "look").await })
            })
            .collect();

        let result = timeout(LOAD_TIMEOUT, async {
            for task in tasks {
                task.await.expect("task join").expect("handle_input");
            }
        })
        .await;

        assert!(result.is_ok(), "parallel bot input deadlocked");
        assert!(
            started.elapsed() < LOAD_TIMEOUT,
            "parallel bot input too slow: {:?}",
            started.elapsed()
        );
        for i in 0..6 {
            let nick = format!("player{i}");
            assert!(
                transport
                    .privmsgs_to(&nick)
                    .iter()
                    .any(|line| line.contains("featureless void")),
                "player{i} should receive look output"
            );
        }
    }

    #[tokio::test]
    async fn persist_during_concurrent_reads_does_not_deadlock() {
        let (persistence, manager, config) = load_world(4).await;
        let world = manager.world().clone();
        let manager = Arc::new(Mutex::new(manager));

        let readers: Vec<_> = (0..4)
            .map(|i| {
                let mgr = Arc::clone(&manager);
                let cfg = config.clone();
                let nick = format!("player{i}");
                tokio::spawn(async move {
                    for _ in 0..5 {
                        dispatch_command(Arc::clone(&mgr), &nick, "look", &cfg).await;
                    }
                })
            })
            .collect();

        let writer = tokio::spawn(async move {
            for _ in 0..3 {
                let _ = world.persist_changes(&persistence).await;
            }
        });

        let result = timeout(LOAD_TIMEOUT, async {
            for task in readers {
                task.await.expect("reader join");
            }
            writer.await.expect("writer join");
        })
        .await;

        assert!(result.is_ok(), "persist + concurrent reads deadlocked");
    }
}