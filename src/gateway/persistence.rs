//! Persist per-connection player state back to the shared world graph.

use crate::object::ObjectId;
use crate::persistence::Persistence;
use crate::repl::PlayerSession;
use crate::world::SharedWorld;

/// Flush a disconnecting player's actor row and any world dirty objects.
pub async fn persist_connection_state<P: Persistence>(
    world: &SharedWorld,
    persistence: &P,
    player: &PlayerSession,
) -> anyhow::Result<()> {
    {
        let mut guard = world.lock().await;
        if let Some(actor) = guard.object_mut(player.actor_id()) {
            player.persist_to_actor(actor);
        } else {
            guard.mark_dirty(player.actor_id());
        }
    }
    world.persist_changes(persistence).await?;
    Ok(())
}

/// Ensure the actor object is present in the shared graph.
pub async fn hydrate_actor<P: Persistence>(
    world: &SharedWorld,
    persistence: &P,
    actor_id: &ObjectId,
) -> anyhow::Result<bool> {
    world.ensure_object(persistence, actor_id).await
}