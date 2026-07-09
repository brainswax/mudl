//! Edge-case tests: disconnect/reconnect, RBAC denials, revision conflicts (M5).

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use tokio::sync::Mutex;

    use crate::gateway::{LoginError, SessionManager};
    use crate::irc::{dispatch_command, IrcBot, IrcConfig, IrcMessage, MockTransport};
    use crate::mudl::AnatomyRegistry;
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

    async fn two_player_world() -> (SqlitePersistence, SessionManager<SqlitePersistence>) {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let room_id = ObjectId::new("room:void-001");

        let mut hero1 = bare("player:hero-001", "Alice");
        hero1.set_property_string(crate::object::LOGIN_NAME_PROPERTY, "alice");
        hero1.location = Some(room_id.clone());
        let mut hero2 = bare("player:hero-002", "Bob");
        hero2.set_property_string(crate::object::LOGIN_NAME_PROPERTY, "bob");
        hero2.location = Some(room_id.clone());

        let mut room = bare("room:void-001", "The Void");
        room.set_property_map(
            "exits",
            HashMap::from([("north".to_string(), ObjectId::new("room:north-001"))]),
        );
        let mut north = bare("room:north-001", "North Passage");
        north.add_exit("south", room_id.clone());

        persistence.save_object(&hero1).await.unwrap();
        persistence.save_object(&hero2).await.unwrap();
        persistence.save_object(&room).await.unwrap();
        persistence.save_object(&north).await.unwrap();

        let manager = SessionManager::open(persistence.clone(), AnatomyRegistry::default())
            .await
            .unwrap();
        (persistence, manager)
    }

    #[tokio::test]
    async fn reconnect_after_logout_restores_persisted_location() {
        let (persistence, mut manager) = two_player_world().await;
        let void = ObjectId::new("room:void-001");
        let hero = ObjectId::new("player:hero-001");

        manager
            .login("alice", hero.clone(), Some(void.clone()))
            .await
            .unwrap();
        manager
            .with_session("alice", |session| session.go("north"))
            .await
            .unwrap()
            .unwrap();
        manager.logout("alice").await.unwrap();

        manager
            .login("alice", hero.clone(), Some(void))
            .await
            .unwrap();

        let location = manager
            .with_session("alice", |session| session.current_location().cloned())
            .await
            .unwrap();
        assert_eq!(location.as_ref().map(|id| id.as_str()), Some("room:north-001"));

        let stored = persistence.load_object(&hero).await.unwrap().unwrap();
        assert_eq!(
            stored.location.as_ref().map(|id| id.as_str()),
            Some("room:north-001")
        );
    }

    #[tokio::test]
    async fn double_logout_reports_not_connected() {
        let (_persistence, mut manager) = two_player_world().await;
        let void = ObjectId::new("room:void-001");

        manager
            .login("alice", ObjectId::new("player:hero-001"), Some(void))
            .await
            .unwrap();
        manager.logout("alice").await.unwrap();

        let err = manager.logout("alice").await.unwrap_err();
        assert!(err.to_string().contains("not connected"));
    }

    #[tokio::test]
    async fn login_while_connected_via_irc_returns_clear_message() {
        let (_persistence, manager) = two_player_world().await;
        let manager = Arc::new(Mutex::new(manager));
        let config = IrcConfig::default();
        let void = ObjectId::new("room:void-001");

        {
            let mut guard = manager.lock().await;
            guard
                .login("alice", ObjectId::new("player:hero-001"), Some(void))
                .await
                .unwrap();
        }

        let outcome = dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;
        assert!(outcome
            .to_sender
            .iter()
            .any(|line| line.contains("already logged in")));
    }

    #[tokio::test]
    async fn irc_quit_when_not_logged_in_is_harmless() {
        let (_persistence, manager) = two_player_world().await;
        let transport = Arc::new(MockTransport::new());
        let bot = IrcBot::new(manager, Arc::clone(&transport), IrcConfig::default());

        bot.handle_message(IrcMessage::Quit {
            nick: "stranger".to_string(),
            account: None,
            reason: Some("gone".to_string()),
        })
        .await
        .unwrap();

        assert_eq!(bot.manager().lock().await.connection_count(), 0);
    }

    #[tokio::test]
    async fn irc_disconnect_persists_then_allows_relogin() {
        let (_persistence, manager) = two_player_world().await;
        let transport = Arc::new(MockTransport::new());
        let config = IrcConfig::default();
        let void = ObjectId::new("room:void-001");

        let bot = IrcBot::new(manager, Arc::clone(&transport), config.clone());
        bot.handle_input("alice", "login").await.unwrap();
        bot.handle_input("alice", "go north").await.unwrap();

        bot.handle_message(IrcMessage::Quit {
            nick: "alice".to_string(),
            account: None,
            reason: None,
        })
        .await
        .unwrap();

        let manager = bot.manager();
        assert!(!manager.lock().await.is_connected("alice"));

        let login = bot.handle_input("alice", "login").await.unwrap();
        assert!(login.to_sender.iter().any(|line| line.contains("Welcome")));

        let location = manager
            .lock()
            .await
            .with_session("alice", |session| session.current_location().cloned())
            .await
            .unwrap();
        assert_eq!(
            location.as_ref().map(|id| id.as_str()),
            Some("room:north-001")
        );
        let _ = void;
    }

    #[tokio::test]
    async fn player_meta_command_denied_over_irc() {
        let (_persistence, manager) = two_player_world().await;
        let manager = Arc::new(Mutex::new(manager));
        let config = IrcConfig::default();
        let void = ObjectId::new("room:void-001");
        let hero = ObjectId::new("player:hero-001");

        {
            let mut guard = manager.lock().await;
            guard.login("alice", hero.clone(), Some(void)).await.unwrap();
            guard
                .with_session("alice", |session| {
                    session.object_mut(&hero, |player| {
                        player.permissions = PermissionFlags::player_default();
                    });
                })
                .await;
        }

        let outcome = dispatch_command(manager, "alice", "@set name x", &config).await;
        assert!(outcome
            .to_sender
            .iter()
            .any(|line| line.contains("wizard privilege")));
    }

    #[tokio::test]
    async fn builder_meta_passes_rbac_but_stays_deferred_on_irc() {
        let (_persistence, manager) = two_player_world().await;
        let manager = Arc::new(Mutex::new(manager));
        let config = IrcConfig::default();
        let void = ObjectId::new("room:void-001");
        let hero = ObjectId::new("player:hero-001");

        {
            let mut guard = manager.lock().await;
            guard.login("alice", hero.clone(), Some(void)).await.unwrap();
            guard
                .with_session("alice", |session| {
                    session.object_mut(&hero, |player| {
                        player.permissions = PermissionFlags::builder_role();
                    });
                })
                .await;
        }

        let outcome = dispatch_command(manager, "alice", "@examine self", &config).await;
        assert!(outcome
            .to_sender
            .iter()
            .any(|line| line.contains("not enabled yet")));
    }

    #[tokio::test]
    async fn logout_persists_despite_revision_conflict() {
        let (persistence, mut manager) = two_player_world().await;
        let void = ObjectId::new("room:void-001");
        let hero = ObjectId::new("player:hero-001");

        manager
            .login("alice", hero.clone(), Some(void.clone()))
            .await
            .unwrap();
        manager
            .with_session("alice", |session| session.go("north"))
            .await
            .unwrap()
            .unwrap();

        let mut stale = persistence.load_object(&hero).await.unwrap().unwrap();
        stale.location = Some(void);
        persistence.save_object(&stale).await.unwrap();

        manager.logout("alice").await.unwrap();

        let stored = persistence.load_object(&hero).await.unwrap().unwrap();
        assert_eq!(
            stored.location.as_ref().map(|id| id.as_str()),
            Some("room:north-001")
        );
        assert!(stored.revision >= 2);
    }

    #[tokio::test]
    async fn connect_orphan_reclaimed_on_login() {
        let (_persistence, mut manager) = two_player_world().await;
        let void = ObjectId::new("room:void-001");
        let hero = ObjectId::new("player:hero-001");

        let session = manager
            .connect("alice", hero.clone(), Some(void.clone()))
            .await
            .unwrap();
        drop(session);

        assert!(manager.is_connected("alice"));
        assert!(manager.session_handle("alice").is_none());

        manager.login("alice", hero, Some(void)).await.unwrap();
        assert!(manager.session_handle("alice").is_some());
    }

    #[tokio::test]
    async fn login_rejects_actor_bound_to_different_nick() {
        let (_persistence, mut manager) = two_player_world().await;
        let void = ObjectId::new("room:void-001");
        let hero = ObjectId::new("player:hero-001");

        let session = manager.connect("alice", hero, Some(void)).await.unwrap();
        drop(session);

        let err = manager
            .login("bob", ObjectId::new("player:hero-001"), Some(ObjectId::new("room:void-001")))
            .await
            .unwrap_err();
        assert!(matches!(err, LoginError::ActorInUse(_)));
    }

    #[tokio::test]
    async fn irc_login_requires_token_when_auth_policy_enabled() {
        use std::collections::HashMap;

        use crate::gateway::LoginAuthPolicy;
        use crate::irc::{dispatch_command, IrcConfig};

        let (_persistence, manager) = two_player_world().await;
        let manager = Arc::new(Mutex::new(manager));
        let mut config = IrcConfig::default();
        config.login_auth = LoginAuthPolicy {
            require_auth: true,
            env_tokens: HashMap::from([(
                "player:hero-001".to_string(),
                "hero-secret".to_string(),
            )]),
            ..LoginAuthPolicy::permissive()
        };

        let denied = dispatch_command(Arc::clone(&manager), "alice", "login", &config).await;
        assert!(denied
            .to_sender
            .iter()
            .any(|l| l.contains("Invalid login credentials")));

        let ok = dispatch_command(
            manager,
            "alice",
            "login player:hero-001 hero-secret",
            &config,
        )
        .await;
        assert!(ok.to_sender.iter().any(|l| l.contains("Welcome")));
    }
}