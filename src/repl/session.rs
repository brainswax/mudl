//! Per-player REPL session: world graph, location, anatomy, and dirty persistence.

use std::collections::HashMap;
use std::fmt;

use crate::command::persist_inventory_dirty;
use crate::display::{
    format_room_look_player, narrate_go, narrate_go_encumbered, narrate_no_exit,
    narrate_no_location, narrate_overloaded, resolve_object, DisplayContext, DisplayFlags,
    DisplayMode, ResolveScope, TargetResolution,
};
use crate::object::{player_encumbrance_level, EncumbranceLevel};
use crate::world::portal::{
    portal_for_direction, portal_passage_block, portal_permits_exit, PortalBlock,
};
use crate::world::navigation::{normalize_direction, resolve_exit};
use crate::inventory::InventoryContext;
use crate::mudl::AnatomyRegistry;
use crate::object::{Object, ObjectId};
use crate::persistence::Persistence;
use crate::world::{
    persist_all, persist_dirty, resolve_player_location, restore_session, DirtyTracker,
};

/// Errors from session-level navigation and state operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    NoLocation,
    LocationMissing,
    PlayerMissing,
    NoExit(String),
    DoorClosed(String),
    DoorLocked(String),
    Overloaded,
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoLocation => write!(f, "{}", narrate_no_location()),
            Self::LocationMissing => {
                write!(f, "The ground shifts beneath you — you are nowhere.")
            }
            Self::PlayerMissing => write!(f, "You seem to have lost yourself."),
            Self::NoExit(dir) => {
                let friendly = normalize_direction(dir).unwrap_or(dir.as_str());
                write!(f, "{}", narrate_no_exit(friendly))
            }
            Self::DoorClosed(name) => write!(f, "The {name} is closed."),
            Self::DoorLocked(name) => write!(f, "The {name} is locked."),
            Self::Overloaded => write!(f, "{}", narrate_overloaded()),
        }
    }
}

impl std::error::Error for SessionError {}

/// One player's interactive session over the object graph.
///
/// Holds the authoritative in-memory world slice for this connection. Persistence
/// is applied incrementally via [`DirtyTracker`](crate::world::DirtyTracker).
#[derive(Debug)]
pub struct Session {
    pub player_id: ObjectId,
    anatomy: AnatomyRegistry,
    objects: HashMap<ObjectId, Object>,
    current_location: Option<ObjectId>,
    dirty: DirtyTracker,
}

impl Session {
    /// Hydrate from persistence and resolve the player's current location.
    /// Build a session from an in-memory object graph (tests and tooling).
    #[cfg(test)]
    pub fn test_session(
        player_id: ObjectId,
        anatomy: AnatomyRegistry,
        objects: HashMap<ObjectId, Object>,
        current_location: Option<ObjectId>,
    ) -> Self {
        Self {
            player_id,
            anatomy,
            objects,
            current_location,
            dirty: DirtyTracker::default(),
        }
    }

    pub async fn restore<P: Persistence>(
        persistence: &P,
        player_id: ObjectId,
        bootstrap_location: Option<ObjectId>,
        anatomy: AnatomyRegistry,
    ) -> anyhow::Result<Self> {
        let world = restore_session(persistence, player_id.clone(), bootstrap_location).await?;
        Ok(Self {
            player_id,
            anatomy,
            objects: world.objects,
            current_location: world.current_location,
            dirty: world.dirty,
        })
    }

    pub fn player_id(&self) -> &ObjectId {
        &self.player_id
    }

    pub fn anatomy(&self) -> &AnatomyRegistry {
        &self.anatomy
    }

    pub fn set_anatomy(&mut self, anatomy: AnatomyRegistry) {
        self.anatomy = anatomy;
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

    pub fn current_location(&self) -> Option<&ObjectId> {
        self.current_location.as_ref()
    }

    pub fn set_current_location(&mut self, location: ObjectId) {
        self.current_location = Some(location);
        self.dirty.mark(&self.player_id);
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

    /// Insert into the session graph without marking dirty (e.g. `load` from DB).
    pub fn cache_object(&mut self, obj: Object) {
        self.objects.insert(obj.id.clone(), obj);
    }

    /// Load a single object from persistence into the session if absent.
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

    /// Re-resolve current location from the player object's persisted `location`.
    pub fn sync_location_from_player(&mut self) {
        self.current_location =
            resolve_player_location(&self.player_id, &self.objects, self.current_location.clone());
    }

    /// Resolve a command target against this session's object graph.
    pub fn resolve_target(&self, name: &str, scope: ResolveScope) -> TargetResolution {
        resolve_object(
            name,
            &self.player_id,
            self.current_location.as_ref(),
            &self.objects,
            scope,
        )
    }

    /// Build display context using the session's object map (clone for `'static` helpers).
    pub fn display_context(&self, mode: DisplayMode) -> DisplayContext {
        DisplayContext::new(self.player_id.clone(), mode)
            .with_objects(self.objects.clone())
            .with_anatomy(self.anatomy.clone())
    }

    /// Mutable inventory command context wired to session dirty tracking.
    pub fn inventory_context(&mut self) -> InventoryContext<'_> {
        InventoryContext {
            player_id: &self.player_id,
            room_id: self.current_location.as_ref(),
            objects: &mut self.objects,
            anatomy: &self.anatomy,
            dirty: Some(&mut self.dirty),
        }
    }

    /// Move the player along an exit from the current location.
    ///
    /// Returns movement narration plus a brief look at the arrival room (description,
    /// exits, visible items).
    pub fn go(&mut self, direction: &str) -> Result<String, SessionError> {
        let loc_id = self
            .current_location
            .as_ref()
            .ok_or(SessionError::NoLocation)?
            .clone();

        let exits = self
            .objects
            .get(&loc_id)
            .ok_or(SessionError::LocationMissing)?
            .get_exits();

        let (dir_label, target_id) = resolve_exit(&exits, direction)
            .ok_or_else(|| SessionError::NoExit(direction.to_string()))?;

        if let Some(portal) = portal_for_direction(&loc_id, dir_label, &self.objects) {
            if !portal_permits_exit(portal, &target_id) {
                return Err(SessionError::NoExit(direction.to_string()));
            }
            let name = portal.name.to_lowercase();
            match portal_passage_block(portal) {
                Some(PortalBlock::Closed) => return Err(SessionError::DoorClosed(name)),
                Some(PortalBlock::Locked) => return Err(SessionError::DoorLocked(name)),
                None => {}
            }
        }

        let encumbrance = self
            .objects
            .get(&self.player_id)
            .map(|player| player_encumbrance_level(player, &self.objects))
            .ok_or(SessionError::PlayerMissing)?;
        if encumbrance == EncumbranceLevel::Overloaded {
            return Err(SessionError::Overloaded);
        }

        let player = self
            .objects
            .get_mut(&self.player_id)
            .ok_or(SessionError::PlayerMissing)?;
        player.location = Some(target_id.clone());
        self.dirty.mark(&self.player_id);

        self.current_location = Some(target_id.clone());

        let movement_line = match encumbrance {
            EncumbranceLevel::Encumbered => narrate_go_encumbered(dir_label),
            EncumbranceLevel::Unencumbered | EncumbranceLevel::Overloaded => {
                narrate_go(dir_label)
            }
        };
        let mut lines = vec![movement_line];
        if let Some(room) = self.objects.get(&target_id) {
            let ctx = DisplayContext::new(self.player_id.clone(), DisplayMode::Player)
                .with_objects(self.objects.clone())
                .with_anatomy(self.anatomy.clone())
                .with_flags(DisplayFlags::BRIEF);
            lines.push(format_room_look_player(room, &ctx));
        }
        Ok(lines.join("\n"))
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

    /// Force-save every object in the session.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{drop_item, take_item};
    use crate::object::{PermissionFlags, StackableSpec};
    use crate::persistence::SqlitePersistence;


    fn bare(id: &str, name: &str) -> Object {
        Object {
            id: ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new("player:hero-001"),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        }
    }

    fn human_anatomy() -> AnatomyRegistry {
        use crate::mudl::{BodySlotDef, CreatureDef, SlotType};
        let mut anatomy = AnatomyRegistry::default();
        anatomy.creatures.insert(
            "human".to_string(),
            CreatureDef {
                name: "human".to_string(),
                slots: vec![
                    BodySlotDef {
                        name: "left_hand".to_string(),
                        capacity: 1,
                        slot_type: SlotType::Grasp,
                        hands: 1,
                    },
                    BodySlotDef {
                        name: "right_hand".to_string(),
                        capacity: 1,
                        slot_type: SlotType::Grasp,
                        hands: 1,
                    },
                ],
            },
        );
        anatomy
    }

    async fn sample_session() -> (SqlitePersistence, Session) {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let player_id = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("room:void-001");

        let mut player = bare("player:hero-001", "Hero");
        player.set_property_string("body_plan", "human");
        player.location = Some(room_id.clone());

        let mut room = bare("room:void-001", "The Void");
        room.set_property_map(
            "exits",
            HashMap::from([(
                "north".to_string(),
                ObjectId::new("room:north-001"),
            )]),
        );

        let mut north = bare("room:north-001", "North Passage");
        north.set_property_string("description", "A narrow passage north.");
        north.add_exit("south", room_id.clone());

        persistence.save_object(&player).await.unwrap();
        persistence.save_object(&room).await.unwrap();
        persistence.save_object(&north).await.unwrap();

        let session = Session::restore(&persistence, player_id, Some(room_id), human_anatomy())
            .await
            .unwrap();
        (persistence, session)
    }

    #[tokio::test]
    async fn restore_hydrates_objects_and_location() {
        let (_persistence, session) = sample_session().await;
        assert_eq!(session.len(), 3);
        assert_eq!(
            session.current_location().map(|id| id.as_str()),
            Some("room:void-001")
        );
        assert!(session.object(&ObjectId::new("player:hero-001")).is_some());
    }

    #[tokio::test]
    async fn go_updates_player_and_location() {
        let (_persistence, mut session) = sample_session().await;
        let msg = session.go("north").unwrap();
        assert!(msg.contains("north"));
        assert!(msg.contains("narrow passage"));
        assert!(msg.contains("Obvious exits"));
        assert_eq!(
            session.current_location().map(|id| id.as_str()),
            Some("room:north-001")
        );
        assert_eq!(
            session
                .object(&session.player_id)
                .and_then(|p| p.location.as_ref())
                .map(|id| id.as_str()),
            Some("room:north-001")
        );
        assert!(session.dirty().is_dirty(&session.player_id));
    }

    #[tokio::test]
    async fn go_blocks_when_overloaded() {
        let (_persistence, mut session) = sample_session().await;
        let player_id = session.player_id.clone();
        let mut heavy = bare("item:anvil-001", "Anvil");
        heavy.set_property_numeric("weight", 100.0);
        heavy.location = Some(player_id.clone());

        let player = session.object_mut(&player_id).unwrap();
        player.set_property_int("max_weight", 100);
        player.set_property_map(
            "body_slots",
            HashMap::from([("right_hand".to_string(), heavy.id.clone())]),
        );
        session.objects.insert(heavy.id.clone(), heavy);

        let err = session.go("north").unwrap_err();
        assert_eq!(err, SessionError::Overloaded);
        assert!(err.to_string().contains("too overloaded"));
        assert_eq!(
            session.current_location().map(|id| id.as_str()),
            Some("room:void-001")
        );
    }

    #[tokio::test]
    async fn go_warns_when_encumbered_but_allows_movement() {
        let (_persistence, mut session) = sample_session().await;
        let player_id = session.player_id.clone();
        let mut heavy = bare("item:crate-001", "Crate");
        heavy.set_property_numeric("weight", 92.0);
        heavy.location = Some(player_id.clone());

        let player = session.object_mut(&player_id).unwrap();
        player.set_property_int("max_weight", 100);
        player.set_property_map(
            "body_slots",
            HashMap::from([("right_hand".to_string(), heavy.id.clone())]),
        );
        session.objects.insert(heavy.id.clone(), heavy);

        let msg = session.go("north").unwrap();
        assert!(msg.contains("too encumbered to move easily"));
        assert!(msg.contains("head north"));
        assert_eq!(
            session.current_location().map(|id| id.as_str()),
            Some("room:north-001")
        );
    }

    #[tokio::test]
    async fn go_accepts_direction_aliases() {
        let (_persistence, mut session) = sample_session().await;
        let msg = session.go("n").unwrap();
        assert!(msg.contains("north"));
        assert_eq!(
            session.current_location().map(|id| id.as_str()),
            Some("room:north-001")
        );
    }

    #[tokio::test]
    async fn persist_changes_saves_only_dirty_objects() {
        let (persistence, mut session) = sample_session().await;
        session.mark_dirty(&ObjectId::new("player:hero-001"));

        let count = session.persist_changes(&persistence).await.unwrap();
        assert_eq!(count, 1);
        assert!(session.dirty().is_empty());

        let player = persistence
            .load_object(&ObjectId::new("player:hero-001"))
            .await
            .unwrap()
            .expect("player persisted");
        assert_eq!(player.name, "Hero");
    }

    #[tokio::test]
    async fn inventory_moves_mark_dirty_for_incremental_persist() {
        let (persistence, mut session) = sample_session().await;
        let room_id = session.current_location().unwrap().clone();

        let mut sword = bare("item:sword-001", "Rusty Sword");
        sword.location = Some(room_id);
        session.upsert_object(sword);

        let mut ctx = session.inventory_context();
        take_item(&mut ctx, "rusty").unwrap();

        assert!(session.dirty().len() >= 2);

        session.persist_changes(&persistence).await.unwrap();
        let player = persistence
            .load_object(&session.player_id)
            .await
            .unwrap()
            .unwrap();
        assert!(player.body_slot_item("right_hand").is_some());
    }

    #[tokio::test]
    async fn drop_marks_dirty_and_persists() {
        let (persistence, mut session) = sample_session().await;
        let room_id = session.current_location().unwrap().clone();

        let mut bars = bare("item:gold-bar-001", "gold bar");
        bars.apply_stackable_role(&StackableSpec {
            count: 3,
            max_stack: 99,
        });
        bars.location = Some(room_id.clone());
        let bars_id = bars.id.clone();
        session.upsert_object(bars);

        let mut ctx = session.inventory_context();
        take_item(&mut ctx, "gold").unwrap();
        session.dirty_mut().clear();

        let mut ctx = session.inventory_context();
        drop_item(&mut ctx, "gold").unwrap();
        assert!(!session.dirty().is_empty());

        session.persist_changes(&persistence).await.unwrap();
        let bars = persistence.load_object(&bars_id).await.unwrap().unwrap();
        assert_eq!(bars.location.as_ref().map(|id| id.as_str()), Some("room:void-001"));
    }
}