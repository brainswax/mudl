//! Authoritative in-memory world graph — shared across all player connections (M5).
//!
//! ## Lock discipline
//!
//! - One [`SharedWorld`] mutex serializes in-memory mutations (world-level lock; per-room
//!   mutex deferred until IRC load warrants finer granularity).
//! - [`DispatchStack`] lives on [`WorldState`] so re-entrant event depth/cycle checks are
//!   per-world, not thread-local.
//! - Do **not** call [`SharedWorld::lock`] / [`SharedWorld::persist_changes`] from inside a
//!   [`SharedWorld::lock_blocking`] closure on the same task — the mutex is not re-entrant.
//! - Persistence helpers release the mutex before SQLite I/O so other connections can proceed.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, MutexGuard};

use crate::mudl::AnatomyRegistry;
use crate::object::{Object, ObjectId};
use crate::persistence::{
    refresh_revision_from_db, Persistence, PersistenceError, SaveMetadata, DEFAULT_SAVE_RETRIES,
};
use crate::world::dirty::{persist_dirty, DirtyTracker};
use crate::world::dispatch_guard::DispatchStack;
use crate::world::events::{execute_event as dispatch_execute_event, EventContext, EventOutcome};
use crate::world::session::{hydrate_world, persist_all};

/// Shared world state: object graph, creature definitions, dispatch guard, and dirty tracking.
#[derive(Debug)]
pub struct WorldState {
    objects: HashMap<ObjectId, Object>,
    /// Immutable during play; `Arc` allows cloning a handle while mutating `objects`.
    anatomy: Arc<AnatomyRegistry>,
    dirty: DirtyTracker,
    dispatch: DispatchStack,
}

/// Mutable borrows of world fields used by inventory, movement, and events.
pub struct WorldMutation<'a> {
    pub objects: &'a mut HashMap<ObjectId, Object>,
    pub anatomy: &'a AnatomyRegistry,
    pub dirty: &'a mut DirtyTracker,
    pub dispatch: &'a mut DispatchStack,
}

/// Thread-safe handle to a world — IRC holds `Arc<SharedWorld>`; REPL locks per command.
#[derive(Clone)]
pub struct SharedWorld {
    inner: Arc<Mutex<WorldState>>,
}

impl SharedWorld {
    pub fn new(state: WorldState) -> Self {
        Self {
            inner: Arc::new(Mutex::new(state)),
        }
    }

    pub fn from_arc(inner: Arc<Mutex<WorldState>>) -> Self {
        Self { inner }
    }

    pub fn arc(&self) -> Arc<Mutex<WorldState>> {
        Arc::clone(&self.inner)
    }

    /// Async lock for IRC / gateway handlers.
    pub async fn lock(&self) -> MutexGuard<'_, WorldState> {
        self.inner.lock().await
    }

    /// Blocking lock for sync REPL command handlers.
    ///
    /// Uses `try_lock` + yield (not `blocking_lock`) so callers inside a Tokio runtime —
    /// including `#[tokio::test]` and `#[tokio::main]` — do not block the executor thread.
    /// IRC paths should prefer [`Self::lock`] via [`Session::with_locked_async`].
    pub fn lock_blocking(&self) -> MutexGuard<'_, WorldState> {
        let mut spins = 0u32;
        loop {
            if let Ok(guard) = self.inner.try_lock() {
                return guard;
            }
            spins = spins.saturating_add(1);
            if spins % 64 == 0 {
                std::thread::yield_now();
            } else {
                std::hint::spin_loop();
            }
        }
    }

    /// Load an object into the graph if absent. Mutex is released during the DB read.
    pub async fn ensure_object<P: Persistence>(
        &self,
        persistence: &P,
        id: &ObjectId,
    ) -> anyhow::Result<bool> {
        let already_loaded = {
            let guard = self.lock().await;
            guard.object(id).is_some()
        };
        if already_loaded {
            return Ok(true);
        }
        let loaded = persistence.load_object(id).await?;
        if let Some(obj) = loaded {
            let mut guard = self.lock().await;
            if guard.object(id).is_none() {
                guard.cache_object(obj);
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Persist dirty objects inside a single SQLite transaction when supported.
    ///
    /// The world mutex is not held during `save_objects_batch` I/O.
    pub async fn persist_changes<P: Persistence>(&self, persistence: &P) -> anyhow::Result<usize> {
        let pending: Vec<ObjectId> = {
            let mut guard = self.lock().await;
            guard.dirty_mut().take_dirty().into_iter().collect()
        };
        if pending.is_empty() {
            return Ok(0);
        }

        for _ in 0..DEFAULT_SAVE_RETRIES {
            let snapshots: Vec<Object> = {
                let guard = self.lock().await;
                pending
                    .iter()
                    .filter_map(|id| guard.object(id).cloned())
                    .collect()
            };
            if snapshots.is_empty() {
                return Ok(0);
            }
            let refs: Vec<&Object> = snapshots.iter().collect();

            match persistence.save_objects_batch(&refs).await {
                Ok(metas) => {
                    let mut guard = self.lock().await;
                    for (id, meta) in pending.iter().zip(metas) {
                        guard.apply_save_metadata(id, &meta);
                    }
                    return Ok(pending.len());
                }
                Err(err) => {
                    let Some(pe) = err.downcast_ref::<PersistenceError>() else {
                        let mut guard = self.lock().await;
                        guard.dirty_mut().mark_many(pending.iter().cloned());
                        return Err(err);
                    };
                    if !pe.is_revision_conflict() {
                        let mut guard = self.lock().await;
                        guard.dirty_mut().mark_many(pending.iter().cloned());
                        return Err(err);
                    }
                    let PersistenceError::RevisionConflict { id, .. } = pe;
                    if let Some(fresh) = persistence.load_object(id).await? {
                        let mut guard = self.lock().await;
                        if let Some(obj) = guard.object_mut(id) {
                            refresh_revision_from_db(obj, &fresh);
                        }
                    }
                }
            }
        }

        let mut guard = self.lock().await;
        guard.dirty_mut().mark_many(pending.iter().cloned());
        guard.flush_dirty(persistence).await
    }

    /// Persist dirty objects, or the full graph when the dirty set is empty.
    pub async fn persist<P: Persistence>(&self, persistence: &P) -> anyhow::Result<()> {
        let dirty_empty = {
            let guard = self.lock().await;
            guard.dirty().is_empty()
        };
        if dirty_empty {
            self.persist_all(persistence).await
        } else {
            self.persist_changes(persistence).await?;
            Ok(())
        }
    }

    /// Persist every object in one transactional batch (mutex released during I/O).
    pub async fn persist_all<P: Persistence>(&self, persistence: &P) -> anyhow::Result<()> {
        let (ids, snapshots) = {
            let guard = self.lock().await;
            let ids: Vec<ObjectId> = guard.objects().keys().cloned().collect();
            let snapshots: Vec<Object> = ids
                .iter()
                .filter_map(|id| guard.object(id).cloned())
                .collect();
            (ids, snapshots)
        };
        if ids.is_empty() {
            return Ok(());
        }
        let refs: Vec<&Object> = snapshots.iter().collect();
        let metas = persistence.save_objects_batch(&refs).await?;
        let mut guard = self.lock().await;
        for (id, meta) in ids.iter().zip(metas) {
            guard.apply_save_metadata(id, &meta);
        }
        guard.dirty_mut().clear();
        Ok(())
    }
}

impl WorldState {
    /// Hydrate all active objects from persistence.
    pub async fn restore<P: Persistence>(
        persistence: &P,
        anatomy: AnatomyRegistry,
    ) -> anyhow::Result<Self> {
        let objects = hydrate_world(persistence).await?;
        Ok(Self {
            objects,
            anatomy: Arc::new(anatomy),
            dirty: DirtyTracker::default(),
            dispatch: DispatchStack::default(),
        })
    }

    /// Build world state from an in-memory graph (tests and tooling).
    pub fn with_objects(anatomy: AnatomyRegistry, objects: HashMap<ObjectId, Object>) -> Self {
        Self {
            objects,
            anatomy: Arc::new(anatomy),
            dirty: DirtyTracker::default(),
            dispatch: DispatchStack::default(),
        }
    }

    pub fn into_shared(self) -> SharedWorld {
        SharedWorld::new(self)
    }

    pub fn anatomy(&self) -> &AnatomyRegistry {
        self.anatomy.as_ref()
    }

    pub fn anatomy_arc(&self) -> Arc<AnatomyRegistry> {
        Arc::clone(&self.anatomy)
    }

    pub fn set_anatomy(&mut self, anatomy: AnatomyRegistry) {
        self.anatomy = Arc::new(anatomy);
    }

    pub fn dispatch(&self) -> &DispatchStack {
        &self.dispatch
    }

    pub fn dispatch_mut(&mut self) -> &mut DispatchStack {
        &mut self.dispatch
    }

    /// Split borrows for inventory/move/event helpers.
    pub fn borrow_mutation(&mut self) -> WorldMutation<'_> {
        WorldMutation {
            objects: &mut self.objects,
            anatomy: self.anatomy.as_ref(),
            dirty: &mut self.dirty,
            dispatch: &mut self.dispatch,
        }
    }

    pub fn objects(&self) -> &HashMap<ObjectId, Object> {
        &self.objects
    }

    pub fn objects_mut(&mut self) -> &mut HashMap<ObjectId, Object> {
        &mut self.objects
    }

    pub fn object(&self, id: &ObjectId) -> Option<&Object> {
        self.objects.get(id)
    }

    pub fn object_mut(&mut self, id: &ObjectId) -> Option<&mut Object> {
        self.dirty.mark(id);
        self.objects.get_mut(id)
    }

    pub fn dirty(&self) -> &DirtyTracker {
        &self.dirty
    }

    pub fn dirty_mut(&mut self) -> &mut DirtyTracker {
        &mut self.dirty
    }

    pub fn mark_dirty(&mut self, id: &ObjectId) {
        self.dirty.mark(id);
    }

    /// Update revision metadata after a successful save without re-marking dirty.
    pub fn apply_save_metadata(&mut self, id: &ObjectId, meta: &SaveMetadata) {
        if let Some(obj) = self.objects.get_mut(id) {
            meta.apply_to(obj);
        }
    }

    pub fn upsert_object(&mut self, obj: Object) {
        self.dirty.mark(&obj.id);
        self.objects.insert(obj.id.clone(), obj);
    }

    pub fn cache_object(&mut self, obj: Object) {
        self.objects.insert(obj.id.clone(), obj);
    }

    pub async fn ensure_object<P: Persistence>(
        &mut self,
        persistence: &P,
        id: &ObjectId,
    ) -> anyhow::Result<bool> {
        if self.objects.contains_key(id) {
            return Ok(true);
        }
        if let Some(obj) = persistence.load_object(id).await? {
            self.objects.insert(id.clone(), obj);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Run the event bus using this world's dispatch stack and object graph.
    pub fn execute_event(&mut self, event_name: &str, ctx: &EventContext) -> EventOutcome {
        dispatch_execute_event(
            &mut self.dispatch,
            event_name,
            ctx,
            &mut self.objects,
            Some(self.anatomy.as_ref()),
        )
    }

    pub async fn persist_changes<P: Persistence>(
        &mut self,
        persistence: &P,
    ) -> anyhow::Result<usize> {
        self.flush_dirty(persistence).await
    }

    pub async fn flush_dirty<P: Persistence>(&mut self, persistence: &P) -> anyhow::Result<usize> {
        persist_dirty(persistence, &mut self.objects, &mut self.dirty).await
    }

    pub async fn persist<P: Persistence>(&mut self, persistence: &P) -> anyhow::Result<()> {
        if self.dirty.is_empty() {
            persist_all(persistence, &mut self.objects).await?;
        } else {
            self.flush_dirty(persistence).await?;
        }
        Ok(())
    }

    pub async fn persist_all<P: Persistence>(&mut self, persistence: &P) -> anyhow::Result<()> {
        persist_all(persistence, &mut self.objects).await
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::PermissionFlags;
    use crate::persistence::SqlitePersistence;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

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
    async fn shared_world_mutex_serializes_async_writers() {
        let world = WorldState::with_objects(AnatomyRegistry::default(), HashMap::new());
        let shared = Arc::new(world.into_shared());
        let shared2 = Arc::clone(&shared);

        let (started_tx, started_rx) = tokio::sync::oneshot::channel();

        let writer = tokio::spawn(async move {
            let mut guard = shared.lock().await;
            started_tx.send(()).ok();
            tokio::time::sleep(Duration::from_millis(60)).await;
            guard.mark_dirty(&ObjectId::new("item:a-001"));
        });

        started_rx.await.unwrap();
        let wait_start = Instant::now();
        let _reader = shared2.lock().await;
        assert!(
            wait_start.elapsed() >= Duration::from_millis(50),
            "second lock should wait for the first writer"
        );
        writer.await.unwrap();
    }

    #[tokio::test]
    async fn persist_changes_releases_lock_during_batch_save() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let mut objects = HashMap::new();
        let a = sample("item:a-001");
        let b = sample("item:b-001");
        objects.insert(a.id.clone(), a);
        objects.insert(b.id.clone(), b);

        let mut world = WorldState::with_objects(AnatomyRegistry::default(), objects);
        world.dirty_mut().mark(&ObjectId::new("item:a-001"));
        world.dirty_mut().mark(&ObjectId::new("item:b-001"));
        let shared = Arc::new(world.into_shared());

        let concurrent_reads = Arc::new(AtomicUsize::new(0));
        let reads = Arc::clone(&concurrent_reads);
        let shared_for_reads = Arc::clone(&shared);

        let reader = tokio::spawn(async move {
            for _ in 0..20 {
                if shared_for_reads.lock().await.object(&ObjectId::new("item:a-001")).is_some() {
                    reads.fetch_add(1, Ordering::SeqCst);
                }
                tokio::task::yield_now().await;
            }
        });

        let saved = shared.persist_changes(&persistence).await.unwrap();
        assert_eq!(saved, 2);
        reader.await.unwrap();
        assert!(
            concurrent_reads.load(Ordering::SeqCst) > 0,
            "reads should succeed while persist runs (lock released during I/O)"
        );

        let guard = shared.lock().await;
        assert_eq!(guard.object(&ObjectId::new("item:a-001")).unwrap().revision, 1);
        assert_eq!(guard.object(&ObjectId::new("item:b-001")).unwrap().revision, 1);
        assert!(guard.dirty().is_empty());
    }

    #[test]
    fn dispatch_stack_lives_on_world_state_across_locks() {
        let shared =
            WorldState::with_objects(AnatomyRegistry::default(), HashMap::new()).into_shared();
        let host = ObjectId::new("room:a-001");
        {
            let mut guard = shared.lock_blocking();
            guard.dispatch_mut().test_seed(host.clone(), "on_enter");
        }
        let mut guard = shared.lock_blocking();
        let err = guard
            .dispatch_mut()
            .enter(&host, "on_enter")
            .err()
            .unwrap();
        assert!(matches!(
            err,
            crate::world::DispatchError::CycleDetected { .. }
        ));
    }
}