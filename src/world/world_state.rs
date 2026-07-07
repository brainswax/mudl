//! Authoritative in-memory world graph — shared across all player connections (M5).

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, MutexGuard};

use crate::command::persist_inventory_dirty;
use crate::mudl::AnatomyRegistry;
use crate::object::{Object, ObjectId};
use crate::persistence::Persistence;
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

    /// Blocking lock for the REPL and sync command handlers (including `#[tokio::test]`).
    pub fn lock_blocking(&self) -> MutexGuard<'_, WorldState> {
        loop {
            if let Ok(guard) = self.inner.try_lock() {
                return guard;
            }
            std::thread::yield_now();
        }
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
        persist_dirty(persistence, &mut self.objects, &mut self.dirty).await
    }

    pub async fn persist<P: Persistence>(&mut self, persistence: &P) -> anyhow::Result<()> {
        persist_inventory_dirty(persistence, &mut self.objects, &mut self.dirty).await
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