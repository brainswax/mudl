//! Transport-facing gateway: IRC nick registry, session lifecycle, and RBAC (M5).

mod persistence;
mod rbac;
mod registry;
mod session_manager;

#[cfg(test)]
mod edge_cases;
#[cfg(test)]
mod load;
#[cfg(test)]
mod m5_scenarios;
#[cfg(test)]
mod multi_user;

pub use persistence::{hydrate_actor, persist_connection_state};
pub use rbac::{
    actor_has_tier, actor_tier, authorize_meta_command, authorize_plain_command,
    required_tier_for_meta_verb, required_tier_for_plain_command, tier_denied_message, ActorTier,
    AuthError,
};
pub use registry::{normalize_nick, ConnectionRegistry, RegistryError};
pub use session_manager::{LoginError, LogoutError, SessionManager};

use std::collections::HashMap;

use crate::object::ObjectId;
use crate::persistence::Persistence;
use crate::repl::PlayerSession;
use crate::world::SharedWorld;

/// Multi-connection entry point: one shared world, many nicks / player sessions.
pub struct Gateway {
    world: SharedWorld,
    registry: ConnectionRegistry,
    players: HashMap<ObjectId, PlayerSession>,
}

impl std::fmt::Debug for Gateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gateway")
            .field("connections", &self.registry.len())
            .field("players", &self.players.len())
            .finish_non_exhaustive()
    }
}

impl Gateway {
    pub fn new(world: SharedWorld) -> Self {
        Self {
            world,
            registry: ConnectionRegistry::default(),
            players: HashMap::new(),
        }
    }

    pub fn world(&self) -> &SharedWorld {
        &self.world
    }

    pub fn registry(&self) -> &ConnectionRegistry {
        &self.registry
    }

    pub fn registry_mut(&mut self) -> &mut ConnectionRegistry {
        &mut self.registry
    }

    pub fn player_session(&self, nick: &str) -> Option<&PlayerSession> {
        let actor_id = self.registry.resolve(nick)?;
        self.players.get(actor_id)
    }

    pub fn player_session_mut(&mut self, nick: &str) -> Option<&mut PlayerSession> {
        let actor_id = self.registry.resolve(nick)?.clone();
        self.players.get_mut(&actor_id)
    }

    /// Bind an IRC nick to a player actor and hydrate a per-connection session view.
    pub fn register_connection(
        &mut self,
        nick: &str,
        actor_id: ObjectId,
        bootstrap_location: Option<ObjectId>,
    ) -> Result<(), RegistryError> {
        self.registry.bind(nick, actor_id.clone())?;
        let player = {
            let guard = self.world.lock_blocking();
            PlayerSession::connect(actor_id.clone(), bootstrap_location, &guard)
        };
        self.players.insert(actor_id, player);
        Ok(())
    }

    pub fn disconnect(&mut self, nick: &str) -> Result<(), RegistryError> {
        let actor_id = self.registry.unbind(nick)?;
        self.players.remove(&actor_id);
        Ok(())
    }

    /// Drop a connection and flush the player's actor row plus world dirty objects.
    pub async fn disconnect_persisting<P: Persistence>(
        &mut self,
        nick: &str,
        persistence: &P,
    ) -> Result<(), RegistryError> {
        let actor_id = self.registry.unbind(nick)?;
        let player = self
            .players
            .remove(&actor_id)
            .ok_or_else(|| RegistryError::NickNotBound(normalize_nick(nick)))?;
        persist_connection_state(&self.world, persistence, &player)
            .await
            .map_err(|_| RegistryError::NickNotBound(normalize_nick(nick)))?;
        Ok(())
    }

    pub fn authorize_meta_for_nick(&self, nick: &str, verb: &str) -> Result<(), AuthError> {
        let actor_id = self
            .registry
            .resolve(nick)
            .ok_or(AuthError::UnknownNick)?;
        let guard = self.world.lock_blocking();
        let actor = guard.object(actor_id).ok_or(AuthError::ActorNotFound)?;
        authorize_meta_command(actor, verb)
    }

    pub fn authorize_plain_for_nick(
        &self,
        nick: &str,
        cmd: &str,
        subcommand: Option<&str>,
    ) -> Result<(), AuthError> {
        let actor_id = self
            .registry
            .resolve(nick)
            .ok_or(AuthError::UnknownNick)?;
        let guard = self.world.lock_blocking();
        let actor = guard.object(actor_id).ok_or(AuthError::ActorNotFound)?;
        authorize_plain_command(actor, cmd, subcommand)
    }
}