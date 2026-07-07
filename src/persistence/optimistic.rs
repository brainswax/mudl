use anyhow::{anyhow, Result};

use crate::object::{Object, ObjectId};

use super::error::PersistenceError;
use super::metadata::SaveMetadata;
use super::Persistence;

/// Maximum optimistic-save retries before surfacing a conflict to the caller.
pub const DEFAULT_SAVE_RETRIES: u32 = 5;

/// Apply fresh database revision to a locally mutated object before retrying a save.
pub fn refresh_revision_from_db(local: &mut Object, fresh: &Object) {
    local.revision = fresh.revision;
    local.updated_at = fresh.updated_at.clone();
}

/// Save one object with optimistic locking and automatic conflict retry.
///
/// On conflict the in-memory object is kept (local mutations win) but its expected
/// `revision` is refreshed from the database before the next attempt.
pub async fn save_object_with_retry<P: Persistence>(
    persistence: &P,
    object: &mut Object,
    max_retries: u32,
) -> Result<SaveMetadata> {
    let mut last_conflict = None;
    for _ in 0..max_retries {
        match persistence.save_object(object).await {
            Ok(meta) => {
                meta.apply_to(object);
                return Ok(meta);
            }
            Err(err) => {
                let Some(pe) = err.downcast_ref::<PersistenceError>() else {
                    return Err(err);
                };
                if !pe.is_revision_conflict() {
                    return Err(err);
                }
                let PersistenceError::RevisionConflict { id, .. } = pe;
                let fresh = persistence
                    .load_object(id)
                    .await?
                    .ok_or_else(|| anyhow!("object {id} disappeared during save retry"))?;
                refresh_revision_from_db(object, &fresh);
                last_conflict = Some(pe.clone());
            }
        }
    }
    Err(anyhow!(
        "{}",
        last_conflict.unwrap_or(PersistenceError::RevisionConflict {
            id: object.id.clone(),
            expected: object.revision,
            actual: object.revision,
        })
    ))
}

/// Persist a batch with per-object retry; updates revisions on the provided graph.
pub async fn save_objects_batch_with_retry<P: Persistence>(
    persistence: &P,
    objects: &mut std::collections::HashMap<ObjectId, Object>,
    ids: &[ObjectId],
    max_retries: u32,
) -> Result<usize> {
    let mut saved = 0usize;
    for id in ids {
        let Some(obj) = objects.get_mut(id) else {
            continue;
        };
        save_object_with_retry(persistence, obj, max_retries).await?;
        saved += 1;
    }
    Ok(saved)
}