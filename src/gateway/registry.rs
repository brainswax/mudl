//! Transport identity → player actor mapping, owned by [`SessionManager`](super::SessionManager).

use std::collections::HashMap;

use crate::object::ObjectId;

/// Normalize IRC nicks for case-insensitive lookup.
pub fn normalize_nick(nick: &str) -> String {
    nick.trim().to_ascii_lowercase()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    NickInUse(String),
    NickNotBound(String),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NickInUse(nick) => write!(f, "nick '{nick}' is already connected"),
            Self::NickNotBound(nick) => write!(f, "nick '{nick}' is not connected"),
        }
    }
}

impl std::error::Error for RegistryError {}

/// Maps transport identities (IRC nick, future Slack user id) to player [`ObjectId`]s.
///
/// Always accessed through [`SessionManager`](super::SessionManager); not a standalone registry.
#[derive(Debug, Default, Clone)]
pub struct ConnectionRegistry {
    by_nick: HashMap<String, ObjectId>,
}

impl ConnectionRegistry {
    pub fn bind(&mut self, nick: &str, actor_id: ObjectId) -> Result<(), RegistryError> {
        let key = normalize_nick(nick);
        if self.by_nick.contains_key(&key) {
            return Err(RegistryError::NickInUse(key));
        }
        self.by_nick.insert(key, actor_id);
        Ok(())
    }

    /// Replace an existing binding (reconnect / takeover).
    pub fn rebind(&mut self, nick: &str, actor_id: ObjectId) {
        self.by_nick.insert(normalize_nick(nick), actor_id);
    }

    pub fn resolve(&self, nick: &str) -> Option<&ObjectId> {
        self.by_nick.get(&normalize_nick(nick))
    }

    pub fn unbind(&mut self, nick: &str) -> Result<ObjectId, RegistryError> {
        self.by_nick
            .remove(&normalize_nick(nick))
            .ok_or_else(|| RegistryError::NickNotBound(normalize_nick(nick)))
    }

    pub fn len(&self) -> usize {
        self.by_nick.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_nick.is_empty()
    }

    /// Whether this actor is already bound to a connected nick.
    pub fn is_actor_bound(&self, actor_id: &ObjectId) -> bool {
        self.by_nick.values().any(|id| id == actor_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_resolves_case_insensitive_nick() {
        let mut registry = ConnectionRegistry::default();
        let actor = ObjectId::new("player:hero-001");
        registry.bind("Brains", actor.clone()).unwrap();
        assert_eq!(registry.resolve("brains"), Some(&actor));
    }

    #[test]
    fn registry_rejects_duplicate_nick() {
        let mut registry = ConnectionRegistry::default();
        registry
            .bind("alice", ObjectId::new("player:a-001"))
            .unwrap();
        assert!(registry.bind("alice", ObjectId::new("player:b-001")).is_err());
    }

    #[test]
    fn registry_tracks_actor_binding() {
        let mut registry = ConnectionRegistry::default();
        let actor = ObjectId::new("player:a-001");
        registry.bind("alice", actor.clone()).unwrap();
        assert!(registry.is_actor_bound(&actor));
        assert!(!registry.is_actor_bound(&ObjectId::new("player:b-001")));
    }
}