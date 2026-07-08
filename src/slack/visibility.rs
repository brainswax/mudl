//! Audience resolution for room-local speech and Slack look scope.

use crate::display::ResolveScope;
use crate::gateway::SessionManager;
use crate::persistence::Persistence;

/// Scope for Slack player `look` — current room only (SEC-60).
pub const SLACK_LOOK_SCOPE: ResolveScope = ResolveScope::RoomOnly;

pub fn slack_look_scope() -> ResolveScope {
    SLACK_LOOK_SCOPE
}

/// Resolve a connected Slack user id by id or in-world player display name.
pub async fn resolve_connected_user_async<P: Persistence + Clone>(
    manager: &SessionManager<P>,
    needle: &str,
) -> Option<String> {
    let needle = needle.trim();
    if needle.is_empty() {
        return None;
    }
    let needle_lower = needle.to_ascii_lowercase();

    for user_id in manager.connected_nicks() {
        if user_id == needle || user_id.to_ascii_lowercase() == needle_lower {
            return Some(user_id);
        }
        let Some(handle) = manager.session_handle(&user_id) else {
            continue;
        };
        let session = handle.lock().await;
        let display_name = session.with_world(|world, player| {
            world
                .object(player.actor_id())
                .map(|obj| obj.name.clone())
        });
        if display_name
            .as_ref()
            .is_some_and(|name| name.eq_ignore_ascii_case(needle))
        {
            return Some(user_id);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::SessionManager;
    use crate::object::{Object, ObjectId, PermissionFlags};
    use crate::persistence::SqlitePersistence;
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

    async fn manager_with_players() -> SessionManager<SqlitePersistence> {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let room = ObjectId::new("room:void-001");
        let mut hero1 = bare("player:hero-001", "Alice");
        hero1.location = Some(room.clone());
        let mut hero2 = bare("player:hero-002", "Bob");
        hero2.location = Some(room.clone());
        let place = bare("room:void-001", "Void");
        persistence.save_object(&hero1).await.unwrap();
        persistence.save_object(&hero2).await.unwrap();
        persistence.save_object(&place).await.unwrap();
        let mut manager =
            SessionManager::open(persistence, crate::mudl::AnatomyRegistry::default())
                .await
                .unwrap();
        manager
            .login("alice", ObjectId::new("player:hero-001"), Some(room.clone()))
            .await
            .unwrap();
        manager
            .login("bob", ObjectId::new("player:hero-002"), Some(room))
            .await
            .unwrap();
        manager
    }

    #[test]
    fn slack_look_scope_is_room_only() {
        assert_eq!(slack_look_scope(), ResolveScope::RoomOnly);
    }

    #[tokio::test]
    async fn resolve_user_by_display_name() {
        let manager = manager_with_players().await;
        assert_eq!(
            resolve_connected_user_async(&manager, "Alice").await,
            Some("alice".to_string())
        );
        assert_eq!(
            resolve_connected_user_async(&manager, "bob").await,
            Some("bob".to_string())
        );
        assert_eq!(resolve_connected_user_async(&manager, "nobody").await, None);
    }
}