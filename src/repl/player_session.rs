//! Per-connection player view — actor identity and location cache (M5).

use crate::display::{
    resolve_object, DisplayContext, DisplayMode, ResolveScope, TargetResolution,
};
use crate::inventory::InventoryContext;
use crate::object::ObjectId;
use crate::world::{resolve_player_location, WorldState};

/// One connected player's session state — lightweight and safe to clone the metadata only.
///
/// Does **not** own the object graph; all world mutations go through [`WorldState`].
#[derive(Debug, Clone)]
pub struct PlayerSession {
    actor_id: ObjectId,
    /// Cached current place; kept in sync with the actor object's `location` field.
    current_location: Option<ObjectId>,
}

impl PlayerSession {
    /// Resolve location from the shared world graph after hydrate/bootstrap.
    pub fn restore(
        actor_id: ObjectId,
        bootstrap_location: Option<ObjectId>,
        world: &WorldState,
    ) -> Self {
        let current_location =
            resolve_player_location(&actor_id, world.objects(), bootstrap_location);
        Self {
            actor_id,
            current_location,
        }
    }

    /// Test helper with a fixed location cache (graph supplied separately on [`WorldState`]).
    pub fn test(actor_id: ObjectId, current_location: Option<ObjectId>) -> Self {
        Self {
            actor_id,
            current_location,
        }
    }

    pub fn actor_id(&self) -> &ObjectId {
        &self.actor_id
    }

    /// Alias for REPL compatibility — the connected player is the event actor.
    pub fn player_id(&self) -> &ObjectId {
        &self.actor_id
    }

    pub fn current_location(&self) -> Option<&ObjectId> {
        self.current_location.as_ref()
    }

    pub fn set_current_location(&mut self, location: ObjectId, world: &mut WorldState) {
        self.set_location_cache(location);
        world.mark_dirty(&self.actor_id);
    }

    /// Update the location cache without touching persistence (caller marks the actor dirty).
    pub fn set_location_cache(&mut self, location: ObjectId) {
        self.current_location = Some(location);
    }

    /// Re-resolve current location from the actor object's persisted `location`.
    pub fn sync_location_from_world(&mut self, world: &WorldState) {
        self.current_location = resolve_player_location(
            &self.actor_id,
            world.objects(),
            self.current_location.clone(),
        );
    }

    /// Resolve a command target against the shared world graph from this player's perspective.
    pub fn resolve_target(&self, world: &WorldState, name: &str, scope: ResolveScope) -> TargetResolution {
        resolve_object(
            name,
            &self.actor_id,
            self.current_location.as_ref(),
            world.objects(),
            scope,
        )
    }

    /// Build display context for this actor over the shared graph.
    pub fn display_context(&self, world: &WorldState, mode: DisplayMode) -> DisplayContext {
        DisplayContext::new(self.actor_id.clone(), mode)
            .with_objects(world.objects().clone())
            .with_anatomy(world.anatomy().clone())
    }

    /// Mutable inventory command context wired to world dirty tracking.
    pub fn inventory_context<'a>(&'a self, world: &'a mut WorldState) -> InventoryContext<'a> {
        let (objects, anatomy, dirty) = world.borrow_for_inventory();
        InventoryContext {
            player_id: &self.actor_id,
            room_id: self.current_location.as_ref(),
            objects,
            anatomy,
            dirty: Some(dirty),
        }
    }
}