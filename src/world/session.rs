use std::collections::HashMap;

use crate::object::{Object, ObjectFactory, ObjectId};
use crate::persistence::Persistence;
use crate::world::dirty::DirtyTracker;

/// Deprecated alias — use [`WorldState`](crate::world::WorldState) for the shared graph.
#[deprecated(note = "use WorldState for the shared graph; PlayerSession for per-connection location")]
#[derive(Debug, Clone)]
pub struct WorldSession {
    pub objects: HashMap<ObjectId, Object>,
    pub dirty: DirtyTracker,
}

/// Load all active objects from persistence into an in-memory map.
pub async fn hydrate_world<P: Persistence>(
    persistence: &P,
) -> anyhow::Result<HashMap<ObjectId, Object>> {
    let mut objects = HashMap::new();
    for obj in persistence.list_objects(false).await? {
        objects.insert(obj.id.clone(), obj);
    }
    Ok(objects)
}

/// Resolve the player's current location from persisted state, falling back when needed.
pub fn resolve_player_location(
    player_id: &ObjectId,
    objects: &HashMap<ObjectId, Object>,
    fallback: Option<ObjectId>,
) -> Option<ObjectId> {
    let player = objects.get(player_id)?;
    if !player.is_active() {
        return fallback;
    }
    let loc_id = player.location.as_ref()?;
    let loc = objects.get(loc_id)?;
    if loc.is_active() && loc.is_location() {
        Some(loc_id.clone())
    } else {
        fallback
    }
}

/// Hydrate the shared object graph from persistence (no per-player location).
pub async fn restore_world_graph<P: Persistence>(
    persistence: &P,
) -> anyhow::Result<HashMap<ObjectId, Object>> {
    hydrate_world(persistence).await
}

/// Deprecated — use [`WorldState::restore`](crate::world::WorldState::restore) and
/// [`PlayerSession::restore`](crate::repl::PlayerSession::restore).
#[deprecated(note = "use WorldState::restore and PlayerSession::restore")]
pub async fn restore_session<P: Persistence>(
    persistence: &P,
    _player_id: ObjectId,
    _bootstrap_location: Option<ObjectId>,
) -> anyhow::Result<WorldSession> {
    let objects = hydrate_world(persistence).await?;
    Ok(WorldSession {
        objects,
        dirty: DirtyTracker::default(),
    })
}

/// Resolve start location after bootstrap when the world already exists.
pub async fn resolve_bootstrap_location<P: Persistence>(
    factory: &ObjectFactory<P>,
    player_id: &ObjectId,
    default_start: ObjectId,
) -> anyhow::Result<ObjectId> {
    if let Some(player) = factory.load_object(player_id).await? {
        if player.is_active() {
            if let Some(loc_id) = &player.location {
                if let Some(loc) = factory.load_object(loc_id).await? {
                    if loc.is_active() && loc.is_location() {
                        return Ok(loc_id.clone());
                    }
                }
            }
        }
    }
    Ok(default_start)
}

/// Persist a batch of objects (e.g. after inventory or movement changes).
pub async fn persist_objects<P: Persistence>(
    persistence: &P,
    objects: &HashMap<ObjectId, Object>,
    ids: &[ObjectId],
) -> anyhow::Result<()> {
    for id in ids {
        if let Some(obj) = objects.get(id) {
            persistence.save_object(obj).await?;
        }
    }
    Ok(())
}

/// Persist every object in the map inside one transaction when supported.
pub async fn persist_all<P: Persistence>(
    persistence: &P,
    objects: &HashMap<ObjectId, Object>,
) -> anyhow::Result<()> {
    let batch: Vec<&Object> = objects.values().collect();
    persistence.save_objects_batch(&batch).await
}
