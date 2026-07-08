//! Player registration and bootstrap-wizard onboarding (SEC-01).

use std::collections::HashMap;

use crate::gateway::{hydrate_actor, LoginError, SessionManager};
use crate::object::{
    display_name_from_player_id, find_player_by_login_name, player_id_for_login_name,
};
use crate::mudl::AnatomyRegistry;
use crate::object::{Object, ObjectFactory, ObjectId, PermissionFlags};
use crate::persistence::Persistence;

/// Maximum display name length for `register`.
pub const MAX_PLAYER_DISPLAY_NAME_LEN: usize = 30;

/// Whether new player registration is allowed (requires a wizard in the world).
pub fn registrations_allowed(objects: &HashMap<ObjectId, Object>) -> bool {
    has_wizard(objects)
}

/// Whether any active player object has wizard privileges.
pub fn has_wizard(objects: &HashMap<ObjectId, Object>) -> bool {
    objects.values().any(|obj| {
        obj.is_active()
            && obj.id.as_str().starts_with("player:")
            && obj.permissions.contains(PermissionFlags::WIZARD)
    })
}

/// Normalize an in-character display name (may differ from login name).
pub fn normalize_player_display_name(raw: &str) -> Result<String, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Display name cannot be empty.");
    }
    let safe: String = trimmed
        .chars()
        .filter(|ch| !ch.is_control() && *ch != '\n' && *ch != '\r')
        .take(MAX_PLAYER_DISPLAY_NAME_LEN)
        .collect();
    if safe.trim().is_empty() {
        return Err("Display name cannot be empty.");
    }
    Ok(safe.trim().to_string())
}

/// Pick a starting location for a newly registered player.
pub fn default_spawn_location(objects: &HashMap<ObjectId, Object>) -> Option<ObjectId> {
    objects
        .values()
        .find(|obj| {
            obj.is_active()
                && obj.is_location()
                && matches!(obj.object_type(), "room" | "area")
        })
        .map(|obj| obj.id.clone())
}

/// Registration failures surfaced to transports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    RegistrationsClosed,
    LoginNameTaken {
        login_name: String,
        player_id: ObjectId,
    },
    InvalidName(&'static str),
    CreateFailed(String),
    Login(LoginError),
}

impl std::fmt::Display for RegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RegistrationsClosed => write!(
                f,
                "Registration is closed — no wizard is configured on this world. Ask an operator."
            ),
            Self::LoginNameTaken { login_name, player_id } => write!(
                f,
                "Login name '{login_name}' is already taken ({player_id}). Send 'login' or 'login {player_id}'."
            ),
            Self::InvalidName(msg) => write!(f, "{msg}"),
            Self::CreateFailed(msg) => write!(f, "Could not create player: {msg}"),
            Self::Login(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for RegisterError {}

impl From<LoginError> for RegisterError {
    fn from(err: LoginError) -> Self {
        Self::Login(err)
    }
}

/// Ensure [`DEFAULT_PLAYER`](crate::irc::IrcConfig::default_player) exists with wizard privileges.
///
/// Creates the bootstrap player when missing. Returns `true` when a new row was created or upgraded.
pub async fn ensure_bootstrap_wizard<P: Persistence>(
    factory: &ObjectFactory<P>,
    player_id: &ObjectId,
    anatomy: &AnatomyRegistry,
    start_location: Option<ObjectId>,
) -> anyhow::Result<bool> {
    if let Some(mut player) = factory.load_object(player_id).await? {
        if !player.permissions.contains(PermissionFlags::WIZARD) {
            player.permissions = PermissionFlags::wizard_role();
            crate::persistence::save_and_sync(factory.persistence(), &mut player).await?;
            return Ok(true);
        }
        return Ok(false);
    }
    let display_name = display_name_from_player_id(player_id);
    factory
        .create_player_at_id(
            player_id.clone(),
            &display_name,
            anatomy,
            PermissionFlags::wizard_role(),
            start_location,
        )
        .await?;
    Ok(true)
}

impl<P: Persistence + Clone> SessionManager<P> {
    /// Create a new player and bind the transport nick in one step.
    pub async fn register_and_login(
        &mut self,
        nick: &str,
        login_name: &str,
        display_name: &str,
        factory: &ObjectFactory<P>,
        anatomy: &AnatomyRegistry,
    ) -> Result<ObjectId, RegisterError> {
        let player_id = player_id_for_login_name(login_name);
        let spawn_location = {
            let guard = self.world().lock().await;
            if !registrations_allowed(guard.objects()) {
                return Err(RegisterError::RegistrationsClosed);
            }
            if let Some(existing) = find_player_by_login_name(guard.objects(), login_name) {
                return Err(RegisterError::LoginNameTaken {
                    login_name: login_name.to_string(),
                    player_id: existing.id.clone(),
                });
            }
            if guard.object(&player_id).is_some() {
                return Err(RegisterError::LoginNameTaken {
                    login_name: login_name.to_string(),
                    player_id: player_id.clone(),
                });
            }
            default_spawn_location(guard.objects())
        };

        if factory.load_object(&player_id).await.ok().flatten().is_some() {
            return Err(RegisterError::LoginNameTaken {
                login_name: login_name.to_string(),
                player_id,
            });
        }

        let mut player = factory
            .create_player_with_login_name(login_name, display_name, anatomy)
            .await
            .map_err(|e| RegisterError::CreateFailed(e.to_string()))?;
        player.owner = player.id.clone();
        player.permissions = PermissionFlags::player_default();
        if let Some(loc) = spawn_location.clone() {
            player.location = Some(loc.clone());
            player.set_property_object_ref("home_location", loc);
        }
        crate::persistence::save_and_sync(factory.persistence(), &mut player)
            .await
            .map_err(|e| RegisterError::CreateFailed(e.to_string()))?;

        hydrate_actor(self.world(), factory.persistence(), &player.id)
            .await
            .map_err(|e| RegisterError::CreateFailed(e.to_string()))?;

        let player_id = player.id.clone();
        let bootstrap_location = player.location.clone();
        self.login(nick, player_id.clone(), bootstrap_location).await?;
        Ok(player_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::LOGIN_NAME_PROPERTY;
    use crate::object::PermissionFlags;
    use crate::persistence::SqlitePersistence;

    fn bare(id: &str, name: &str, permissions: PermissionFlags) -> Object {
        let (revision, updated_at) = crate::object::object_persistence_defaults();
        Object {
            id: ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new(id),
            permissions,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
            revision,
            updated_at,
        }
    }

    #[test]
    fn registrations_blocked_without_wizard() {
        let mut objects = HashMap::new();
        objects.insert(
            ObjectId::new("player:hero"),
            bare("player:hero", "Hero", PermissionFlags::player_default()),
        );
        assert!(!registrations_allowed(&objects));
    }

    #[tokio::test]
    async fn ensure_bootstrap_wizard_creates_missing_player() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence);
        let anatomy = crate::mudl::load_module("modules/default")
            .unwrap()
            .active_world()
            .unwrap()
            .anatomy
            .clone();
        let player_id = ObjectId::new("player:wizard");
        let created = ensure_bootstrap_wizard(&factory, &player_id, &anatomy, None)
            .await
            .unwrap();
        assert!(created);
        let player = factory.load_object(&player_id).await.unwrap().unwrap();
        assert!(player.permissions.contains(PermissionFlags::WIZARD));
    }

    #[tokio::test]
    async fn register_uses_verbatim_login_name_id() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence.clone());
        let anatomy = crate::mudl::load_module("modules/default")
            .unwrap()
            .active_world()
            .unwrap()
            .anatomy
            .clone();

        let room = bare("room:void-001", "Void", PermissionFlags::EVERYONE);
        persistence.save_object(&room).await.unwrap();

        let wizard = bare(
            "player:admin",
            "Admin",
            PermissionFlags::wizard_role(),
        );
        persistence.save_object(&wizard).await.unwrap();

        let mut manager = SessionManager::open(persistence.clone(), anatomy.clone())
            .await
            .unwrap();
        let player_id = manager
            .register_and_login("alice", "alice", "Alice", &factory, &anatomy)
            .await
            .unwrap();
        assert_eq!(player_id.as_str(), "player:alice");
        let player = factory.load_object(&player_id).await.unwrap().unwrap();
        assert_eq!(player.name, "Alice");
        assert_eq!(
            player.get_string_property(LOGIN_NAME_PROPERTY).as_deref(),
            Some("alice")
        );
        assert!(manager.is_connected("alice"));
    }
}