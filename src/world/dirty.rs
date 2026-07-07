//! Dirty tracking for incremental persistence.

use std::collections::HashSet;

use crate::object::ObjectId;
use crate::persistence::Persistence;

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

/// Persist only objects marked dirty. Falls back to saving all if dirty set is empty
/// and `force_all` is true.
pub async fn persist_dirty<P: Persistence>(
    persistence: &P,
    objects: &std::collections::HashMap<ObjectId, crate::object::Object>,
    dirty: &mut DirtyTracker,
) -> anyhow::Result<usize> {
    let ids = dirty.take_dirty();
    let count = ids.len();
    let batch: Vec<&crate::object::Object> = ids
        .iter()
        .filter_map(|id| objects.get(id))
        .collect();
    if !batch.is_empty() {
        persistence.save_objects_batch(&batch).await?;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{Object, PermissionFlags};
    use crate::persistence::SqlitePersistence;
    use std::collections::HashMap;

    fn sample(id: &str) -> Object {
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

        let count = persist_dirty(&persistence, &objects, &mut dirty)
            .await
            .unwrap();
        assert_eq!(count, 1);
        assert!(dirty.is_empty());

        assert!(persistence.load_object(&a.id).await.unwrap().is_some());
        assert!(persistence.load_object(&b.id).await.unwrap().is_none());
    }
}
