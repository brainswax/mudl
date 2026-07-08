//! M6 multi-user Slack scenarios — shared visibility, tells, and channel routing.

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use tokio::sync::Mutex;

    use crate::gateway::SessionManager;
    use crate::mudl::{AnatomyRegistry, BodySlotDef, CreatureDef, SlotType};
    use crate::object::{Object, ObjectId, PermissionFlags};
    use crate::persistence::{Persistence, SqlitePersistence};
    use crate::slack::{dispatch_command, SlackBot, SlackConfig};
    use crate::transport::MockTransport;

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

    async fn trio_fixture() -> (
        Arc<Mutex<SessionManager<SqlitePersistence>>>,
        SlackConfig,
        SlackBot<SqlitePersistence, MockTransport>,
        Arc<MockTransport>,
    ) {
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

        persistence.save_object(&hero1).await.unwrap();
        persistence.save_object(&hero2).await.unwrap();
        persistence.save_object(&hero3).await.unwrap();
        persistence.save_object(&void_room).await.unwrap();
        persistence.save_object(&north_room).await.unwrap();

        let manager = SessionManager::open(persistence, human_anatomy())
            .await
            .unwrap();
        let config = SlackConfig {
            world_channel: "C_WORLD".to_string(),
            ..SlackConfig::default()
        };
        let transport = Arc::new(MockTransport::new());
        let bot = SlackBot::new(manager, Arc::clone(&transport), config.clone());
        let manager = bot.manager();

        bot.handle_input("U_ALICE", "D_ALICE", "login player:hero-001")
            .await
            .unwrap();
        bot.handle_input("U_BOB", "D_BOB", "login player:hero-002")
            .await
            .unwrap();
        bot.handle_input("U_SCOUT", "D_SCOUT", "login player:hero-003")
            .await
            .unwrap();
        transport.clear();

        (manager, config, bot, transport)
    }

    #[tokio::test]
    async fn say_does_not_cross_room_boundaries() {
        let (manager, config, ..) = trio_fixture().await;
        dispatch_command(
            Arc::clone(&manager),
            "U_ALICE",
            "D_ALICE",
            "go north",
            &config,
        )
        .await;

        let outcome = dispatch_command(
            manager,
            "U_ALICE",
            "D_ALICE",
            "say anyone there?",
            &config,
        )
        .await;

        let audience: Vec<_> = outcome
            .room_audience
            .iter()
            .flat_map(|d| d.audience.iter().cloned())
            .collect();
        assert!(!audience.iter().any(|id| id.eq_ignore_ascii_case("U_BOB")));
        assert!(audience.iter().any(|id| id.eq_ignore_ascii_case("U_SCOUT")));
        assert!(outcome.channel.iter().any(|(p, _)| p.contains("north-001")));
    }

    #[tokio::test]
    async fn tell_is_private_without_room_broadcast() {
        let (manager, config, ..) = trio_fixture().await;
        let outcome = dispatch_command(
            manager,
            "U_ALICE",
            "D_ALICE",
            "tell Scout meet me north",
            &config,
        )
        .await;

        assert!(outcome.room_audience.is_empty());
        assert!(outcome.channel.is_empty());
        assert_eq!(outcome.private.len(), 1);
        assert_eq!(outcome.private[0].0.to_ascii_lowercase(), "u_scout");
    }

    #[tokio::test]
    async fn tell_delivers_to_target_dm_not_user_id() {
        let (_manager, _config, bot, transport) = trio_fixture().await;
        transport.clear();
        bot.handle_input("U_ALICE", "D_ALICE", "tell Scout secret plan")
            .await
            .unwrap();

        assert!(
            transport
                .direct_messages_to("D_SCOUT")
                .iter()
                .any(|l| l.contains("secret plan")),
            "tell should arrive in target DM channel"
        );
        assert!(transport.direct_messages_to("U_SCOUT").is_empty());
    }

    #[tokio::test]
    async fn movement_notifies_co_located_players_via_dm() {
        let (_manager, _config, bot, transport) = trio_fixture().await;
        transport.clear();
        bot.handle_input("U_ALICE", "D_ALICE", "go north")
            .await
            .unwrap();

        assert!(
            transport
                .direct_messages_to("D_BOB")
                .iter()
                .any(|l| l.contains("left")),
            "Bob in void should see Alice depart"
        );
        assert!(
            transport
                .direct_messages_to("D_SCOUT")
                .iter()
                .any(|l| l.contains("arrived")),
            "Scout in north should see Alice arrive"
        );
        assert!(
            transport
                .presence_messages("mudl-north-001")
                .iter()
                .any(|l| l.contains("arrived")),
            "arrival should post to room channel"
        );
    }

    #[tokio::test]
    async fn look_includes_co_located_player_in_shared_room() {
        let (manager, config, ..) = trio_fixture().await;
        let outcome = dispatch_command(manager, "U_ALICE", "D_ALICE", "look", &config).await;
        assert!(
            outcome
                .to_sender
                .iter()
                .any(|l| l.contains("Bob") || l.contains("bob")),
            "Alice should see Bob in the void: {:?}",
            outcome.to_sender
        );
    }
}