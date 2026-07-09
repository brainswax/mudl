//! Audience resolution for room-local speech and actions.

use std::collections::HashMap;

use crate::display::ResolveScope;
use crate::gateway::{normalize_nick, PlayMode, SessionManager};
use crate::object::ObjectId;
use crate::persistence::Persistence;

/// Scope for IRC player `look` in story mode — current room only (SEC-60).
///
/// Builder `@look` over IRC remains deferred; REPL wizard `@look` uses [`ResolveScope::General`].
pub const IRC_LOOK_SCOPE: ResolveScope = ResolveScope::RoomOnly;

pub fn irc_look_scope(mode: PlayMode) -> ResolveScope {
    let _ = mode;
    IRC_LOOK_SCOPE
}

/// Connected nick whose [`Session`] shares a location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoLocatedPlayer {
    pub nick: String,
    pub actor_id: ObjectId,
}

/// Find all connected players in the same place as `room_id`.
pub async fn players_in_room_async<P: Persistence + Clone>(
    manager: &SessionManager<P>,
    room_id: &ObjectId,
    exclude_nick: Option<&str>,
) -> Vec<CoLocatedPlayer> {
    let exclude = exclude_nick.map(normalize_nick);
    let mut audience = Vec::new();
    for nick in manager.connected_nicks() {
        if exclude.as_deref() == Some(nick.as_str()) {
            continue;
        }
        let Some(handle) = manager.session_handle(&nick) else {
            continue;
        };
        let session = handle.lock().await;
        let Some(loc) = session.current_location() else {
            continue;
        };
        if loc != room_id {
            continue;
        }
        audience.push(CoLocatedPlayer {
            nick,
            actor_id: session.player_id().clone(),
        });
    }
    audience
}

/// Sync helper for tests — briefly locks each session handle (`try_lock` + yield).
pub fn players_in_room<P: Persistence + Clone>(
    manager: &SessionManager<P>,
    room_id: &ObjectId,
    exclude_nick: Option<&str>,
) -> Vec<CoLocatedPlayer> {
    let exclude = exclude_nick.map(normalize_nick);
    manager
        .connected_nicks()
        .into_iter()
        .filter_map(|nick| {
            if exclude.as_deref() == Some(nick.as_str()) {
                return None;
            }
            let handle = manager.session_handle(&nick)?;
            let session = loop {
                if let Ok(guard) = handle.try_lock() {
                    break guard;
                }
                std::thread::yield_now();
            };
            let loc = session.current_location()?;
            if loc != room_id {
                return None;
            }
            Some(CoLocatedPlayer {
                nick,
                actor_id: session.player_id().clone(),
            })
        })
        .collect()
}

/// Connected nicks for speech/movement fan-out, excluding an optional speaker.
pub fn connected_speech_audience<P: Persistence + Clone>(
    manager: &SessionManager<P>,
    room_id: &ObjectId,
    exclude_nick: Option<&str>,
    mode: PlayMode,
) -> Vec<String> {
    match mode {
        PlayMode::Story => players_in_room(manager, room_id, exclude_nick)
            .into_iter()
            .map(|p| p.nick)
            .collect(),
        PlayMode::Open => all_connected_nicks(manager, exclude_nick),
    }
}

/// Async variant of [`connected_speech_audience`].
pub async fn connected_speech_audience_async<P: Persistence + Clone>(
    manager: &SessionManager<P>,
    room_id: &ObjectId,
    exclude_nick: Option<&str>,
    mode: PlayMode,
) -> Vec<String> {
    match mode {
        PlayMode::Story => players_in_room_async(manager, room_id, exclude_nick)
            .await
            .into_iter()
            .map(|p| p.nick)
            .collect(),
        PlayMode::Open => all_connected_nicks(manager, exclude_nick),
    }
}

/// All connected transport identities except an optional speaker.
pub fn all_connected_nicks<P: Persistence + Clone>(
    manager: &SessionManager<P>,
    exclude_nick: Option<&str>,
) -> Vec<String> {
    let exclude = exclude_nick.map(normalize_nick);
    manager
        .connected_nicks()
        .into_iter()
        .filter(|nick| exclude.as_deref() != Some(nick.as_str()))
        .collect()
}

/// Resolve a connected player nick by case-insensitive match.
pub fn resolve_connected_nick<P: Persistence + Clone>(
    manager: &SessionManager<P>,
    needle: &str,
) -> Option<String> {
    let key = normalize_nick(needle);
    manager
        .connected_nicks()
        .into_iter()
        .find(|nick| normalize_nick(nick) == key)
}

/// Map nick → current room id for all connected sessions.
#[allow(dead_code)]
pub fn nick_room_map<P: Persistence + Clone>(
    manager: &SessionManager<P>,
) -> HashMap<String, ObjectId> {
    manager
        .connected_nicks()
        .into_iter()
        .filter_map(|nick| {
            let handle = manager.session_handle(&nick)?;
            let session = loop {
                if let Ok(guard) = handle.try_lock() {
                    break guard;
                }
                std::thread::yield_now();
            };
            let room = session.current_location()?.clone();
            Some((nick, room))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::ResolveScope;
    use crate::gateway::SessionManager;
    use crate::object::{Object, PermissionFlags};
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

    async fn manager_with_two_players() -> SessionManager<SqlitePersistence> {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let room = ObjectId::new("room:void-001");
        let north = ObjectId::new("room:north-001");

        let mut hero1 = bare("player:hero-001", "Hero One");
        hero1.location = Some(room.clone());
        let mut hero2 = bare("player:hero-002", "Hero Two");
        hero2.location = Some(room.clone());
        let mut scout = bare("player:hero-003", "Scout");
        scout.location = Some(north.clone());

        let place = bare("room:void-001", "Void");
        let north_room = bare("room:north-001", "North");

        persistence.save_object(&hero1).await.unwrap();
        persistence.save_object(&hero2).await.unwrap();
        persistence.save_object(&scout).await.unwrap();
        persistence.save_object(&place).await.unwrap();
        persistence.save_object(&north_room).await.unwrap();

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
            .login("scout", ObjectId::new("player:hero-003"), Some(north))
            .await
            .unwrap();
        manager
    }

    #[test]
    fn irc_look_scope_follows_play_mode() {
        assert_eq!(irc_look_scope(PlayMode::Story), ResolveScope::RoomOnly);
        assert_eq!(irc_look_scope(PlayMode::Open), ResolveScope::RoomOnly);
    }

    #[tokio::test]
    async fn open_mode_speech_audience_is_all_connected() {
        let manager = manager_with_two_players().await;
        let room = ObjectId::new("room:void-001");
        let audience =
            connected_speech_audience(&manager, &room, Some("alice"), PlayMode::Open);
        let mut expected = vec!["bob".to_string(), "scout".to_string()];
        let mut actual = audience;
        actual.sort();
        expected.sort();
        assert_eq!(actual, expected);
    }

    #[tokio::test]
    async fn players_in_room_excludes_other_places() {
        let manager = manager_with_two_players().await;
        let room = ObjectId::new("room:void-001");
        let audience = players_in_room(&manager, &room, Some("alice"));
        assert_eq!(audience.len(), 1);
        assert_eq!(audience[0].nick, "bob");
    }

    #[tokio::test]
    async fn resolve_connected_nick_is_case_insensitive() {
        let manager = manager_with_two_players().await;
        assert_eq!(resolve_connected_nick(&manager, "ALICE"), Some("alice".to_string()));
        assert_eq!(resolve_connected_nick(&manager, "nobody"), None);
    }

    #[tokio::test]
    async fn nick_room_map_tracks_locations() {
        let manager = manager_with_two_players().await;
        let rooms = nick_room_map(&manager);
        assert_eq!(rooms.get("alice").map(|id| id.as_str()), Some("room:void-001"));
        assert_eq!(rooms.get("scout").map(|id| id.as_str()), Some("room:north-001"));
    }
}