//! Multi-user session lifecycle: login, active connections, disconnect persistence.

use std::collections::HashMap;

use crate::mudl::AnatomyRegistry;
use crate::object::ObjectId;
use crate::persistence::Persistence;
use crate::repl::{PlayerSession, Session};
use crate::world::{SharedWorld, WorldState};

use super::persistence::{hydrate_actor, persist_connection_state};
use super::registry::{normalize_nick, ConnectionRegistry, RegistryError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginError {
    NickInUse(String),
    ActorInUse(ObjectId),
    ActorNotFound(ObjectId),
    NotAPlayer(ObjectId),
    PersistFailed(String),
}

impl std::fmt::Display for LoginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NickInUse(nick) => write!(f, "nick '{nick}' is already connected"),
            Self::ActorInUse(id) => write!(f, "player {id} is already connected"),
            Self::ActorNotFound(id) => write!(f, "player {id} not found"),
            Self::NotAPlayer(id) => write!(f, "{id} is not a player object"),
            Self::PersistFailed(msg) => write!(f, "failed to load player: {msg}"),
        }
    }
}

impl std::error::Error for LoginError {}

impl From<RegistryError> for LoginError {
    fn from(err: RegistryError) -> Self {
        match err {
            RegistryError::NickInUse(nick) => Self::NickInUse(nick),
            RegistryError::NickNotBound(nick) => Self::NickInUse(nick),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogoutError {
    NotConnected(String),
    PersistFailed(String),
}

impl std::fmt::Display for LogoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConnected(nick) => write!(f, "nick '{nick}' is not connected"),
            Self::PersistFailed(msg) => write!(f, "failed to persist player state: {msg}"),
        }
    }
}

impl std::error::Error for LogoutError {}

impl From<RegistryError> for LogoutError {
    fn from(err: RegistryError) -> Self {
        match err {
            RegistryError::NickNotBound(nick) => Self::NotConnected(nick),
            RegistryError::NickInUse(nick) => Self::NotConnected(nick),
        }
    }
}

/// Hosts one shared world and many simultaneous player connections (IRC / REPL).
pub struct SessionManager<P> {
    world: SharedWorld,
    registry: ConnectionRegistry,
    persistence: P,
    sessions: HashMap<String, Session>,
}

impl<P: Persistence + Clone> SessionManager<P> {
    /// Hydrate the world graph once; connections attach via [`Self::login`].
    pub async fn open(persistence: P, anatomy: AnatomyRegistry) -> anyhow::Result<Self> {
        let world = WorldState::restore(&persistence, anatomy).await?.into_shared();
        Ok(Self {
            world,
            registry: ConnectionRegistry::default(),
            persistence,
            sessions: HashMap::new(),
        })
    }

    pub fn from_world(persistence: P, world: SharedWorld) -> Self {
        Self {
            world,
            registry: ConnectionRegistry::default(),
            persistence,
            sessions: HashMap::new(),
        }
    }

    pub fn world(&self) -> &SharedWorld {
        &self.world
    }

    pub fn persistence(&self) -> &P {
        &self.persistence
    }

    pub fn registry(&self) -> &ConnectionRegistry {
        &self.registry
    }

    pub fn connection_count(&self) -> usize {
        self.registry.len()
    }

    pub fn is_connected(&self, nick: &str) -> bool {
        self.registry.resolve(nick).is_some()
    }

    pub fn actor_for_nick(&self, nick: &str) -> Option<&ObjectId> {
        self.registry.resolve(nick)
    }

    pub fn connected_nicks(&self) -> Vec<String> {
        self.sessions.keys().cloned().collect()
    }

    pub fn session(&self, nick: &str) -> Option<&Session> {
        self.sessions.get(&normalize_nick(nick))
    }

    pub fn session_mut(&mut self, nick: &str) -> Option<&mut Session> {
        let key = normalize_nick(nick);
        self.sessions.get_mut(&key)
    }

    /// Bind nick → actor, hydrate graph row, and store a connection [`Session`].
    pub async fn login(
        &mut self,
        nick: &str,
        actor_id: ObjectId,
        bootstrap_location: Option<ObjectId>,
    ) -> Result<(), LoginError> {
        let session = self
            .build_connection(nick, actor_id, bootstrap_location)
            .await?;
        self.sessions.insert(normalize_nick(nick), session);
        Ok(())
    }

    /// Bind nick → actor and return an owned connection [`Session`] (caller holds it).
    ///
    /// The nick remains reserved until [`Self::release`].
    pub async fn connect(
        &mut self,
        nick: &str,
        actor_id: ObjectId,
        bootstrap_location: Option<ObjectId>,
    ) -> Result<Session, LoginError> {
        self.build_connection(nick, actor_id, bootstrap_location).await
    }

    /// Persist player state, flush world dirty set, and drop a caller-owned connection.
    pub async fn release(&mut self, nick: &str, player: &PlayerSession) -> Result<(), LogoutError> {
        let key = normalize_nick(nick);
        if self.registry.resolve(nick).is_none() {
            return Err(LogoutError::NotConnected(key));
        }

        match persist_connection_state(&self.world, &self.persistence, player).await {
            Ok(()) => {
                self.registry.unbind(nick)?;
                Ok(())
            }
            Err(err) => Err(LogoutError::PersistFailed(err.to_string())),
        }
    }

    /// Persist player state, flush world dirty set, and drop the stored connection.
    pub async fn logout(&mut self, nick: &str) -> Result<(), LogoutError> {
        let key = normalize_nick(nick);
        if self.registry.resolve(nick).is_none() {
            return Err(LogoutError::NotConnected(key));
        }

        let session = self
            .sessions
            .remove(&key)
            .ok_or_else(|| LogoutError::NotConnected(key.clone()))?;

        match persist_connection_state(&self.world, &self.persistence, &session.player).await {
            Ok(()) => {
                self.registry.unbind(nick)?;
                Ok(())
            }
            Err(err) => {
                self.sessions.insert(key, session);
                Err(LogoutError::PersistFailed(err.to_string()))
            }
        }
    }

    /// Persist every connected player (graceful shutdown).
    pub async fn logout_all(&mut self) -> anyhow::Result<()> {
        let nicks: Vec<String> = self.sessions.keys().cloned().collect();
        for nick in nicks {
            self.logout(&nick).await?;
        }
        Ok(())
    }

    async fn build_connection(
        &mut self,
        nick: &str,
        actor_id: ObjectId,
        bootstrap_location: Option<ObjectId>,
    ) -> Result<Session, LoginError> {
        if !actor_id.as_str().starts_with("player:") {
            return Err(LoginError::NotAPlayer(actor_id));
        }

        if self.registry.is_actor_bound(&actor_id) {
            return Err(LoginError::ActorInUse(actor_id));
        }

        if !hydrate_actor(&self.world, &self.persistence, &actor_id)
            .await
            .map_err(|e| LoginError::PersistFailed(e.to_string()))?
        {
            return Err(LoginError::ActorNotFound(actor_id));
        }

        {
            let guard = self.world.lock_blocking();
            if guard.object(&actor_id).is_none() {
                return Err(LoginError::ActorNotFound(actor_id));
            }
        }

        self.registry.bind(nick, actor_id.clone())?;

        let player = {
            let guard = self.world.lock_blocking();
            PlayerSession::connect(actor_id, bootstrap_location, &guard)
        };
        Ok(Session::attach(self.world.clone(), player))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::DisplayFlags;
    use crate::object::{Object, PermissionFlags};
    use crate::persistence::SqlitePersistence;
    use crate::repl::PlayerPrefs;
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

    async fn two_player_world() -> (SqlitePersistence, SessionManager<SqlitePersistence>) {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let room_id = ObjectId::new("room:void-001");

        let mut hero1 = bare("player:hero-001", "Hero One");
        hero1.location = Some(room_id.clone());
        let mut hero2 = bare("player:hero-002", "Hero Two");
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
    async fn login_registers_session_over_shared_world() {
        let (_persistence, mut manager) = two_player_world().await;
        let room_id = ObjectId::new("room:void-001");

        manager
            .login("alice", ObjectId::new("player:hero-001"), Some(room_id.clone()))
            .await
            .unwrap();

        assert_eq!(manager.connection_count(), 1);
        assert!(manager.is_connected("Alice"));
        let session = manager.session("alice").unwrap();
        assert_eq!(session.player_id().as_str(), "player:hero-001");
        assert_eq!(
            session.current_location().map(|id| id.as_str()),
            Some("room:void-001")
        );
    }

    #[tokio::test]
    async fn duplicate_nick_and_actor_rejected() {
        let (_persistence, mut manager) = two_player_world().await;
        let room_id = ObjectId::new("room:void-001");
        let hero1 = ObjectId::new("player:hero-001");
        let hero2 = ObjectId::new("player:hero-002");

        manager.login("alice", hero1.clone(), Some(room_id.clone())).await.unwrap();

        assert!(matches!(
            manager.login("alice", hero2, Some(room_id.clone())).await,
            Err(LoginError::NickInUse(_))
        ));
        assert!(matches!(
            manager.login("bob", hero1, Some(room_id)).await,
            Err(LoginError::ActorInUse(_))
        ));
    }

    #[tokio::test]
    async fn two_sessions_share_world_graph() {
        let (_persistence, mut manager) = two_player_world().await;
        let room_id = ObjectId::new("room:void-001");

        manager
            .login("alice", ObjectId::new("player:hero-001"), Some(room_id.clone()))
            .await
            .unwrap();
        manager
            .login("bob", ObjectId::new("player:hero-002"), Some(room_id))
            .await
            .unwrap();

        {
            let alice = manager.session_mut("alice").unwrap();
            alice.go("north").unwrap();
        }

        let bob = manager.session("bob").unwrap();
        assert_eq!(
            bob.with_world(|world, _| {
                world
                    .object(&ObjectId::new("player:hero-001"))
                    .and_then(|p| p.location.as_ref().map(|id| id.as_str().to_string()))
            }),
            Some("room:north-001".to_string())
        );
        assert_eq!(
            bob.current_location().map(|id| id.as_str()),
            Some("room:void-001")
        );
    }

    #[tokio::test]
    async fn logout_persists_player_location_and_prefs() {
        let (persistence, mut manager) = two_player_world().await;
        let room_id = ObjectId::new("room:void-001");
        let hero_id = ObjectId::new("player:hero-001");

        manager
            .login("alice", hero_id.clone(), Some(room_id))
            .await
            .unwrap();

        {
            let session = manager.session_mut("alice").unwrap();
            session.go("north").unwrap();
            *session.player.prefs_mut() = PlayerPrefs {
                look_flags: DisplayFlags::BRIEF,
            };
        }

        manager.logout("alice").await.unwrap();
        assert_eq!(manager.connection_count(), 0);

        let stored = persistence.load_object(&hero_id).await.unwrap().unwrap();
        assert_eq!(
            stored.location.as_ref().map(|id| id.as_str()),
            Some("room:north-001")
        );
        let prefs_raw = stored
            .get_property(crate::repl::player_session::SESSION_PREFS_KEY)
            .and_then(|p| {
                if let crate::object::Value::String(s) = &p.value {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .unwrap();
        assert!(prefs_raw.contains("\"brief_look\":true"));
    }

    #[tokio::test]
    async fn connect_and_release_for_owned_sessions() {
        let (persistence, mut manager) = two_player_world().await;
        let room_id = ObjectId::new("room:void-001");
        let hero_id = ObjectId::new("player:hero-001");

        let mut session = manager
            .connect("alice", hero_id.clone(), Some(room_id))
            .await
            .unwrap();
        session.go("north").unwrap();
        let player = session.player.clone();

        manager.release("alice", &player).await.unwrap();

        let stored = persistence.load_object(&hero_id).await.unwrap().unwrap();
        assert_eq!(
            stored.location.as_ref().map(|id| id.as_str()),
            Some("room:north-001")
        );
    }

    #[tokio::test]
    async fn logout_all_persists_every_connection() {
        let (persistence, mut manager) = two_player_world().await;
        let room_id = ObjectId::new("room:void-001");

        manager
            .login("alice", ObjectId::new("player:hero-001"), Some(room_id.clone()))
            .await
            .unwrap();
        manager
            .login("bob", ObjectId::new("player:hero-002"), Some(room_id))
            .await
            .unwrap();

        manager.session_mut("alice").unwrap().go("north").unwrap();
        manager.logout_all().await.unwrap();

        assert_eq!(manager.connection_count(), 0);

        let hero1 = persistence
            .load_object(&ObjectId::new("player:hero-001"))
            .await
            .unwrap()
            .unwrap();
        let hero2 = persistence
            .load_object(&ObjectId::new("player:hero-002"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            hero1.location.as_ref().map(|id| id.as_str()),
            Some("room:north-001")
        );
        assert_eq!(
            hero2.location.as_ref().map(|id| id.as_str()),
            Some("room:void-001")
        );
    }

    #[tokio::test]
    async fn rejects_non_player_and_missing_actor() {
        let (_persistence, mut manager) = two_player_world().await;

        assert!(matches!(
            manager
                .login("alice", ObjectId::new("room:void-001"), None)
                .await,
            Err(LoginError::NotAPlayer(_))
        ));

        assert!(matches!(
            manager
                .login("alice", ObjectId::new("player:ghost-999"), None)
                .await,
            Err(LoginError::ActorNotFound(_))
        ));
    }
}