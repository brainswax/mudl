//! Silent auto-login for passwordless players on transport reconnect.

use crate::gateway::{
    resolve_player_for_auto_login, verify_identity_binding, LoginAuthPolicy, SessionManager,
};
use crate::object::ObjectId;
use crate::persistence::Persistence;

/// Bind a transport identity to a matching passwordless player when [`LoginAuthPolicy::auto_login`] is set.
///
/// Returns the player id on success. Does not emit welcome text — callers continue command dispatch.
pub async fn attempt_auto_login<P: Persistence + Clone>(
    manager: &mut SessionManager<P>,
    identity: &str,
    policy: &LoginAuthPolicy,
) -> Option<ObjectId> {
    if !policy.auto_login || manager.is_connected(identity) {
        return None;
    }

    let (player_id, bootstrap_location) = {
        let guard = manager.world().lock().await;
        let player_id = resolve_player_for_auto_login(identity, policy, guard.objects())?;
        verify_identity_binding(policy, identity, &player_id).ok()?;
        let bootstrap_location = guard
            .object(&player_id)
            .and_then(|obj| obj.location.clone());
        (player_id, bootstrap_location)
    };

    manager
        .login(identity, player_id.clone(), bootstrap_location)
        .await
        .ok()?;
    Some(player_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::LoginAuthPolicy;
    use crate::object::{Object, PermissionFlags, LOGIN_NAME_PROPERTY};
    use crate::persistence::SqlitePersistence;
    use std::collections::HashMap;

    fn bare(id: &str, name: &str) -> Object {
        Object {
            id: crate::object::ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: crate::object::ObjectId::new("player:hero-001"),
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
    async fn auto_login_binds_passwordless_player_by_nick() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let room = crate::object::ObjectId::new("room:void-001");
        let mut hero = bare("player:alice", "Alice");
        hero.set_property_string(LOGIN_NAME_PROPERTY, "alice");
        hero.location = Some(room.clone());
        persistence.save_object(&hero).await.unwrap();
        let place = bare("room:void-001", "The Void");
        persistence.save_object(&place).await.unwrap();

        let mut manager = SessionManager::open(persistence, Default::default())
            .await
            .unwrap();
        let policy = LoginAuthPolicy {
            auto_login: true,
            ..LoginAuthPolicy::permissive()
        };

        let player_id = attempt_auto_login(&mut manager, "alice", &policy)
            .await
            .expect("auto login");
        assert_eq!(player_id.as_str(), "player:alice");
        assert!(manager.is_connected("alice"));
    }

    #[tokio::test]
    async fn auto_login_skips_player_with_login_token() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let mut hero = bare("player:alice", "Alice");
        hero.set_property_string(LOGIN_NAME_PROPERTY, "alice");
        hero.set_property_string("login_token", "sekrit");
        persistence.save_object(&hero).await.unwrap();

        let mut manager = SessionManager::open(persistence, Default::default())
            .await
            .unwrap();
        let policy = LoginAuthPolicy {
            auto_login: true,
            require_auth: true,
            ..LoginAuthPolicy::permissive()
        };

        assert!(attempt_auto_login(&mut manager, "alice", &policy)
            .await
            .is_none());
        assert!(!manager.is_connected("alice"));
    }
}