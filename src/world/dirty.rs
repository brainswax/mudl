//! Dirty tracking for incremental persistence.

use std::collections::HashSet;

use crate::object::ObjectId;
use crate::persistence::{
    save_objects_batch_with_retry, Persistence, DEFAULT_SAVE_RETRIES,
};

/// Tracks which objects have been modified since the last persist.
#[derive(Debug, Clone, Default)]
pub struct DirtyTracker {
    ids: HashSet<ObjectId>,
}

impl DirtyTracker {
    pub fn mark(&mut self, id: &ObjectId) {
        self.ids.insert(id.clone());
    }

    pub fn mark_many<I: IntoIterator<Item = ObjectId>>(&mut self, ids: I) {
        self.ids.extend(ids);
    }

    pub fn is_dirty(&self, id: &ObjectId) -> bool {
        self.ids.contains(id)
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Drain all dirty IDs (clears the tracker).
    pub fn take_dirty(&mut self) -> HashSet<ObjectId> {
        std::mem::take(&mut self.ids)
    }

    pub fn clear(&mut self) {
        self.ids.clear();
    }
}

/// Persist only objects marked dirty with optimistic locking and conflict retry.
pub async fn persist_dirty<P: Persistence>(
    persistence: &P,
    objects: &mut std::collections::HashMap<ObjectId, crate::object::Object>,
    dirty: &mut DirtyTracker,
) -> anyhow::Result<usize> {
    let ids: Vec<ObjectId> = dirty.take_dirty().into_iter().collect();
    if ids.is_empty() {
        return Ok(0);
    }

    let pending = ids;
    for _ in 0..DEFAULT_SAVE_RETRIES {
        let batch: Vec<&crate::object::Object> = pending
            .iter()
            .filter_map(|id| objects.get(id))
            .collect();
        if batch.is_empty() {
            return Ok(0);
        }

        match persistence.save_objects_batch(&batch).await {
            Ok(metas) => {
                for (id, meta) in pending.iter().zip(metas) {
                    if let Some(obj) = objects.get_mut(id) {
                        meta.apply_to(obj);
                    }
                }
                return Ok(pending.len());
            }
            Err(err) => {
                let Some(pe) = err.downcast_ref::<crate::persistence::PersistenceError>() else {
                    dirty.mark_many(pending);
                    return Err(err);
                };
                if !pe.is_revision_conflict() {
                    dirty.mark_many(pending);
                    return Err(err);
                }
                let crate::persistence::PersistenceError::RevisionConflict { id, .. } = pe;
                if let Some(fresh) = persistence.load_object(id).await? {
                    if let Some(obj) = objects.get_mut(id) {
                        crate::persistence::refresh_revision_from_db(obj, &fresh);
                    }
                }
            }
        }
    }

    save_objects_batch_with_retry(persistence, objects, &pending, DEFAULT_SAVE_RETRIES).await?;
    Ok(pending.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{Object, PermissionFlags};
    use crate::persistence::SqlitePersistence;
    use std::collections::HashMap;

    fn sample(id: &str) -> Object {
        let (revision, updated_at) = crate::object::object_persistence_defaults();
        Object {
            id: ObjectId::new(id),
            name: id.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new("player:admin-001"),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
            revision,
            updated_at,
        }
    }

    #[tokio::test]
    async fn persist_dirty_only_saves_marked_objects() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let mut dirty = DirtyTracker::default();
        let mut objects = HashMap::new();

        let a = sample("item:a-001");
        let b = sample("item:b-001");
        objects.insert(a.id.clone(), a.clone());
        objects.insert(b.id.clone(), b.clone());

        dirty.mark(&a.id);

        let count = persist_dirty(&persistence, &mut objects, &mut dirty)
            .await
            .unwrap();
        assert_eq!(count, 1);
        assert!(dirty.is_empty());
        assert_eq!(objects.get(&a.id).unwrap().revision, 1);
        assert!(objects.get(&a.id).unwrap().updated_at.is_some());

        assert!(persistence.load_object(&a.id).await.unwrap().is_some());
        assert!(persistence.load_object(&b.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn persist_dirty_retries_after_revision_conflict() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let mut dirty = DirtyTracker::default();
        let mut objects = HashMap::new();

        let mut local = sample("item:a-001");
        local.name = "local".to_string();
        objects.insert(local.id.clone(), local.clone());
        dirty.mark(&local.id);

        let mut stale = sample("item:a-001");
        stale.name = "stale".to_string();
        persistence.save_object(&stale).await.unwrap();

        let count = persist_dirty(&persistence, &mut objects, &mut dirty)
            .await
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(objects.get(&local.id).unwrap().name, "local");
        assert_eq!(objects.get(&local.id).unwrap().revision, 2);

        let loaded = persistence.load_object(&local.id).await.unwrap().unwrap();
        assert_eq!(loaded.name, "local");
        assert_eq!(loaded.revision, 2);
    }
}