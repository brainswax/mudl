//! Authoritative in-memory world graph — shared across all player connections (M5).

use std::collections::HashMap;
use std::sync::Arc;

use crate::command::persist_inventory_dirty;
use crate::mudl::AnatomyRegistry;
use crate::object::{Object, ObjectId};
use crate::persistence::Persistence;
use crate::world::dirty::{persist_dirty, DirtyTracker};
use crate::world::session::{hydrate_world, persist_all};

/// Shared world state: object graph, creature definitions, and dirty persistence tracking.
///
/// IRC/gateway code should hold `Arc<RwLock<WorldState>>` and pass `&mut WorldState` into
/// per-connection [`PlayerSession`](crate::repl::PlayerSession) operations.
#[derive(Debug)]
pub struct WorldState {
    objects: HashMap<ObjectId, Object>,
    /// Immutable during play; `Arc` allows cloning a handle while mutating `objects`.
    anatomy: Arc<AnatomyRegistry>,
    dirty: DirtyTracker,
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
        })
    }

    /// Build world state from an in-memory graph (tests and tooling).
    pub fn with_objects(anatomy: AnatomyRegistry, objects: HashMap<ObjectId, Object>) -> Self {
        Self {
            objects,
            anatomy: Arc::new(anatomy),
            dirty: DirtyTracker::default(),
        }
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

    /// Split borrows for inventory/move helpers (objects + dirty mutably, anatomy immutably).
    pub fn borrow_for_inventory(
        &mut self,
    ) -> (
        &mut HashMap<ObjectId, Object>,
        &AnatomyRegistry,
        &mut DirtyTracker,
    ) {
        (
            &mut self.objects,
            self.anatomy.as_ref(),
            &mut self.dirty,
        )
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

    /// Insert or replace an object and mark it dirty.
    pub fn upsert_object(&mut self, obj: Object) {
        self.dirty.mark(&obj.id);
        self.objects.insert(obj.id.clone(), obj);
    }

    /// Insert into the graph without marking dirty (e.g. `load` from DB).
    pub fn cache_object(&mut self, obj: Object) {
        self.objects.insert(obj.id.clone(), obj);
    }

    /// Load a single object from persistence into the graph if absent.
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

    /// Persist only dirty objects (no-op count when nothing changed).
    pub async fn persist_changes<P: Persistence>(
        &mut self,
        persistence: &P,
    ) -> anyhow::Result<usize> {
        persist_dirty(persistence, &self.objects, &mut self.dirty).await
    }

    /// Persist dirty objects, or the full graph when the tracker is empty.
    pub async fn persist<P: Persistence>(&mut self, persistence: &P) -> anyhow::Result<()> {
        persist_inventory_dirty(persistence, &self.objects, &mut self.dirty).await
    }

    /// Force-save every object in the graph.
    pub async fn persist_all<P: Persistence>(&self, persistence: &P) -> anyhow::Result<()> {
        persist_all(persistence, &self.objects).await
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
}