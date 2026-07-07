//! REPL connection facade: shared [`WorldState`] + per-player [`PlayerSession`].

use std::collections::HashMap;
use std::fmt;

use crate::display::{
    format_room_look_player, narrate_go, narrate_go_encumbered, narrate_no_exit,
    narrate_no_location, narrate_overloaded, narrate_scatter_exit, object_name,
    DisplayContext, DisplayFlags, DisplayMode, ResolveScope, TargetResolution,
};
use crate::inventory::{prepare_gate_for_passage, InventoryError};
use crate::mudl::AnatomyRegistry;
use crate::mudl::trigger_def::events::{ON_ENTER, ON_LEAVE};
use crate::object::{player_encumbrance_level_with_anatomy, EncumbranceLevel, ObjectFactory};
use crate::object::{Object, ObjectId};
use crate::persistence::Persistence;
use crate::repl::PlayerSession;
use crate::world::exits::{apply_loop_entry, apply_scatter_exit, can_traverse_exit};
use crate::world::exit_index::ExitIndex;
use crate::world::navigation::resolve_exit;
use crate::world::place_builder::{
    apply_dig_result, dig_place, link_places, unlink_exit, DigRequest, DigResult, PlaceBuildError,
};
use crate::world::portal::{
    passable_portal_for_direction, portal_passage_block, portal_permits_exit,
};
use crate::world::{execute_event, EventContext, WorldState};

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
    /// Movement or portal prep blocked with a player-facing explanation.
    Blocked(String),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoLocation => write!(f, "{}", narrate_no_location()),
            Self::LocationMissing => {
                write!(f, "The ground shifts beneath you — you are nowhere.")
            }
            Self::PlayerMissing => write!(f, "You seem to have lost yourself."),
            Self::NoExit(dir) => write!(f, "{}", narrate_no_exit(dir)),
            Self::DoorClosed(name) => write!(f, "The {name} is closed."),
            Self::DoorLocked(name) => write!(f, "The {name} is locked."),
            Self::Overloaded => write!(f, "{}", narrate_overloaded()),
            Self::Blocked(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for SessionError {}

/// One REPL connection: shared world graph plus this player's view.
///
/// For multi-user (M5), hold `Arc<RwLock<WorldState>>` separately and one
/// [`PlayerSession`] per IRC nick; this type remains a convenient single-user bundle.
#[derive(Debug)]
pub struct Session {
    pub world: WorldState,
    pub player: PlayerSession,
}

impl Session {
    /// Hydrate world from persistence and attach a player connection.
    pub async fn restore<P: Persistence>(
        persistence: &P,
        player_id: ObjectId,
        bootstrap_location: Option<ObjectId>,
        anatomy: AnatomyRegistry,
    ) -> anyhow::Result<Self> {
        let world = WorldState::restore(persistence, anatomy).await?;
        let player = PlayerSession::restore(player_id, bootstrap_location, &world);
        Ok(Self { world, player })
    }

    /// Build from an in-memory graph (tests and tooling).
    #[cfg(test)]
    pub fn test_session(
        player_id: ObjectId,
        anatomy: AnatomyRegistry,
        objects: HashMap<ObjectId, Object>,
        current_location: Option<ObjectId>,
    ) -> Self {
        Self {
            world: WorldState::with_objects(anatomy, objects),
            player: PlayerSession::test(player_id, current_location),
        }
    }

    pub fn player_id(&self) -> &ObjectId {
        self.player.player_id()
    }

    pub fn anatomy(&self) -> &AnatomyRegistry {
        self.world.anatomy()
    }

    pub fn set_anatomy(&mut self, anatomy: AnatomyRegistry) {
        self.world.set_anatomy(anatomy);
    }

    pub fn objects(&self) -> &HashMap<ObjectId, Object> {
        self.world.objects()
    }

    pub fn objects_mut(&mut self) -> &mut HashMap<ObjectId, Object> {
        self.world.objects_mut()
    }

    pub fn object(&self, id: &ObjectId) -> Option<&Object> {
        self.world.object(id)
    }

    pub fn object_mut(&mut self, id: &ObjectId) -> Option<&mut Object> {
        self.world.object_mut(id)
    }

    pub fn current_location(&self) -> Option<&ObjectId> {
        self.player.current_location()
    }

    pub fn set_current_location(&mut self, location: ObjectId) {
        self.player.set_current_location(location, &mut self.world);
    }

    pub fn dirty(&self) -> &crate::world::DirtyTracker {
        self.world.dirty()
    }

    pub fn dirty_mut(&mut self) -> &mut crate::world::DirtyTracker {
        self.world.dirty_mut()
    }

    pub fn mark_dirty(&mut self, id: &ObjectId) {
        self.world.mark_dirty(id);
    }

    pub fn upsert_object(&mut self, obj: Object) {
        self.world.upsert_object(obj);
    }

    pub fn cache_object(&mut self, obj: Object) {
        self.world.cache_object(obj);
    }

    pub async fn ensure_object<P: Persistence>(
        &mut self,
        persistence: &P,
        id: &ObjectId,
    ) -> anyhow::Result<bool> {
        self.world.ensure_object(persistence, id).await
    }

    pub fn sync_location_from_player(&mut self) {
        self.player.sync_location_from_world(&self.world);
    }

    pub fn resolve_target(&self, name: &str, scope: ResolveScope) -> TargetResolution {
        self.player.resolve_target(&self.world, name, scope)
    }

    pub fn display_context(&self, mode: DisplayMode) -> DisplayContext {
        self.player.display_context(&self.world, mode)
    }

    pub fn perceive_hidden_on_look(&mut self) -> crate::world::EventOutcome {
        let Some(room_id) = self.player.current_location().cloned() else {
            return crate::world::EventOutcome::default();
        };
        let anatomy = self.world.anatomy_arc();
        let outcome = crate::world::run_discovery_on_look(
            &room_id,
            self.player.player_id(),
            self.world.objects_mut(),
            anatomy.as_ref(),
        );
        for id in &outcome.dirty {
            self.world.mark_dirty(id);
        }
        outcome
    }

    pub fn inventory_context(&mut self) -> crate::inventory::InventoryContext<'_> {
        self.player.inventory_context(&mut self.world)
    }

    /// Move the player along an exit from the current location.
    pub fn go(&mut self, direction: &str) -> Result<String, SessionError> {
        let actor_id = self.player.actor_id().clone();
        let loc_id = self
            .player
            .current_location()
            .ok_or(SessionError::NoLocation)?
            .clone();

        let room = self
            .world
            .object(&loc_id)
            .ok_or(SessionError::LocationMissing)?
            .clone();

        let index = ExitIndex::from_place(&room);
        let (dir_label, map_target_id) = resolve_exit(&index, direction)
            .ok_or_else(|| SessionError::NoExit(direction.to_string()))?;

        if !can_traverse_exit(&room, dir_label, map_target_id, self.world.objects()) {
            return Err(SessionError::NoExit(direction.to_string()));
        }

        let mut portal_prep_lines = Vec::new();
        if let Some(portal) =
            passable_portal_for_direction(&loc_id, dir_label, self.world.objects())
        {
            if portal_permits_exit(portal, map_target_id) && portal_passage_block(portal).is_some()
            {
                let portal_id = portal.id.clone();
                let portal_name = portal.name.to_lowercase();
                let mut ctx = self.inventory_context();
                match prepare_gate_for_passage(&mut ctx, &portal_id) {
                    Ok(lines) => portal_prep_lines = lines,
                    Err(InventoryError::NoMatchingKey(_))
                    | Err(InventoryError::ContainerLocked(_)) => {
                        return Err(SessionError::DoorLocked(portal_name));
                    }
                    Err(InventoryError::ContainerClosed(_)) => {
                        return Err(SessionError::DoorClosed(portal_name));
                    }
                    Err(InventoryError::InvalidTarget(msg)) => {
                        return Err(SessionError::Blocked(msg));
                    }
                    Err(err) => return Err(SessionError::Blocked(err.to_string())),
                }
            }
        }

        let encumbrance = self
            .world
            .object(&actor_id)
            .map(|player| {
                player_encumbrance_level_with_anatomy(
                    player,
                    self.world.objects(),
                    Some(self.world.anatomy()),
                )
            })
            .ok_or(SessionError::PlayerMissing)?;
        if encumbrance == EncumbranceLevel::Overloaded {
            return Err(SessionError::Overloaded);
        }

        let looped_target = apply_loop_entry(map_target_id, self.world.objects());
        let target_id = apply_scatter_exit(
            &room,
            dir_label,
            &looped_target,
            &actor_id,
            self.world.objects(),
        );

        let anatomy = self.world.anatomy_arc();
        let leave_outcome = execute_event(
            ON_LEAVE,
            &EventContext {
                actor_id: actor_id.clone(),
                host_id: loc_id.clone(),
                room_id: Some(loc_id.clone()),
                target_id: Some(target_id.clone()),
            },
            self.world.objects_mut(),
            Some(anatomy.as_ref()),
        );

        let player_obj = self
            .world
            .object_mut(&actor_id)
            .ok_or(SessionError::PlayerMissing)?;
        player_obj.location = Some(target_id.clone());

        self.player.set_location_cache(target_id.clone());
        self.world.mark_dirty(&actor_id);

        let looped = looped_target != *map_target_id;
        let scattered = target_id != looped_target;
        let mut lines = portal_prep_lines;
        lines.extend(leave_outcome.lines);
        for id in leave_outcome.dirty {
            self.world.mark_dirty(&id);
        }
        if scattered {
            let dest_name = object_name(&target_id, self.world.objects());
            lines.push(narrate_scatter_exit(&dest_name));
        } else if !looped {
            let movement_line = match encumbrance {
                EncumbranceLevel::Encumbered => narrate_go_encumbered(dir_label),
                EncumbranceLevel::Unencumbered | EncumbranceLevel::Overloaded => {
                    narrate_go(dir_label)
                }
            };
            lines.push(movement_line);
        }
        let enter_outcome = execute_event(
            ON_ENTER,
            &EventContext {
                actor_id: actor_id.clone(),
                host_id: target_id.clone(),
                room_id: Some(target_id.clone()),
                target_id: None,
            },
            self.world.objects_mut(),
            Some(anatomy.as_ref()),
        );
        for line in enter_outcome.lines {
            lines.push(line);
        }
        for id in enter_outcome.dirty {
            self.world.mark_dirty(&id);
        }

        let behavior_outcome = crate::creature::run_creature_behaviors(
            "on_enter",
            &target_id,
            &actor_id,
            self.world.objects_mut(),
            anatomy.as_ref(),
        );
        for id in behavior_outcome.dirty {
            self.world.mark_dirty(&id);
        }
        for behavior_line in behavior_outcome.lines {
            lines.push(behavior_line);
        }
        if let Some(room) = self.world.object(&target_id) {
            let ctx = DisplayContext::new(actor_id.clone(), DisplayMode::Player)
                .with_objects(self.world.objects().clone())
                .with_anatomy(self.world.anatomy().clone())
                .with_flags(DisplayFlags::BRIEF);
            lines.push(format_room_look_player(room, &ctx));
        }
        if let Some(mut player) = self.world.object(&actor_id).cloned() {
            let mut player_dirty = false;
            if let Some(regen) = crate::creature::apply_equipment_regen_on_enter(
                &mut player,
                self.world.objects(),
                anatomy.as_ref(),
            ) {
                lines.push(regen);
                player_dirty = true;
            }
            let tick = crate::creature::tick_conditions(&mut player, anatomy.as_ref(), "on_enter");
            for line in tick.lines {
                lines.push(line);
            }
            if tick.dirty {
                player_dirty = true;
            }
            if player_dirty {
                self.world.upsert_object(player);
            }
        }
        Ok(lines.join("\n"))
    }

    pub async fn dig_place<P: Persistence>(
        &mut self,
        factory: &ObjectFactory<P>,
        request: DigRequest,
    ) -> Result<DigResult, PlaceBuildError> {
        let from_id = self
            .player
            .current_location()
            .ok_or(PlaceBuildError::NoLocation)?
            .clone();
        let from = self
            .world
            .object(&from_id)
            .ok_or_else(|| PlaceBuildError::NotFound(from_id.as_str().to_string()))?
            .clone();
        if !from.is_location() {
            return Err(PlaceBuildError::NotAPlace(from.name.clone()));
        }

        let result = dig_place(
            factory,
            &from,
            self.player.actor_id().clone(),
            request,
            self.world.objects(),
        )
        .await?;
        for id in apply_dig_result(self.world.objects_mut(), &result) {
            self.world.mark_dirty(&id);
        }
        Ok(result)
    }

    pub fn link_exit(
        &mut self,
        from_id: &ObjectId,
        direction: &str,
        target_id: &ObjectId,
        reciprocal: bool,
        return_exit: Option<&str>,
    ) -> Result<Vec<String>, PlaceBuildError> {
        let from = self
            .world
            .object(from_id)
            .ok_or_else(|| PlaceBuildError::NotFound(from_id.as_str().to_string()))?
            .clone();
        if !from.is_location() {
            return Err(PlaceBuildError::NotAPlace(from.name));
        }
        let target = self
            .world
            .object(target_id)
            .ok_or_else(|| PlaceBuildError::NotFound(target_id.as_str().to_string()))?
            .clone();
        if !target.is_location() {
            return Err(PlaceBuildError::NotAPlace(target.name));
        }

        let mut from_mut = from;
        let mut target_mut = target;
        let notes = link_places(
            &mut from_mut,
            &mut target_mut,
            direction,
            self.world.objects(),
            reciprocal,
            return_exit,
        )?;
        self.world.mark_dirty(&from_mut.id);
        self.world.mark_dirty(&target_mut.id);
        self.world.upsert_object(from_mut);
        self.world.upsert_object(target_mut);
        Ok(notes)
    }

    pub fn unlink_exit(
        &mut self,
        from_id: &ObjectId,
        direction: &str,
    ) -> Result<String, PlaceBuildError> {
        let from_name = self
            .world
            .object(from_id)
            .map(|o| o.name.clone())
            .ok_or_else(|| PlaceBuildError::NotFound(from_id.as_str().to_string()))?;
        let from = self
            .world
            .object_mut(from_id)
            .ok_or_else(|| PlaceBuildError::NotFound(from_id.as_str().to_string()))?;
        let removed = unlink_exit(from, direction)?;
        self.world.mark_dirty(from_id);
        Ok(format!(
            "Removed {} exit '{}'{}",
            from_name,
            direction,
            removed
                .and_then(|id| self.world.object(&id).map(|o| format!(" (was {})", o.name)))
                .unwrap_or_default()
        ))
    }

    pub async fn persist_changes<P: Persistence>(
        &mut self,
        persistence: &P,
    ) -> anyhow::Result<usize> {
        self.world.persist_changes(persistence).await
    }

    pub async fn persist<P: Persistence>(&mut self, persistence: &P) -> anyhow::Result<()> {
        self.world.persist(persistence).await
    }

    pub async fn persist_all<P: Persistence>(&self, persistence: &P) -> anyhow::Result<()> {
        self.world.persist_all(persistence).await
    }

    pub fn len(&self) -> usize {
        self.world.len()
    }

    pub fn is_empty(&self) -> bool {
        self.world.is_empty()
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
                        effect: None,
                    },
                    BodySlotDef {
                        name: "right_hand".to_string(),
                        capacity: 1,
                        slot_type: SlotType::Grasp,
                        hands: 1,
                        effect: None,
                    },
                ],
                max_health: 100,
                base_max_weight: Some(100),
                stats: HashMap::new(),
                skills: HashMap::new(),
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
            HashMap::from([("north".to_string(), ObjectId::new("room:north-001"))]),
        );

        let mut north = bare("room:north-001", "North Passage");
        north.set_property_string("description", "A narrow passage north.");
        north.add_exit("south", room_id.clone());
        north.set_exit_return("south", "north");
        room.set_exit_alias("n", "north");
        room.set_exit_return("north", "south");

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
    async fn world_and_player_are_separate() {
        let (_persistence, session) = sample_session().await;
        assert_eq!(session.world.len(), 3);
        assert_eq!(
            session.player.current_location().map(|id| id.as_str()),
            Some("room:void-001")
        );
        assert_eq!(session.player.actor_id().as_str(), "player:hero-001");
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
                .object(session.player_id())
                .and_then(|p| p.location.as_ref())
                .map(|id| id.as_str()),
            Some("room:north-001")
        );
        assert!(session.dirty().is_dirty(session.player_id()));
    }

    #[tokio::test]
    async fn go_blocks_when_overloaded() {
        let (_persistence, mut session) = sample_session().await;
        let player_id = session.player_id().clone();
        let mut heavy = bare("item:anvil-001", "Anvil");
        heavy.set_property_numeric("weight", 100.0);
        heavy.location = Some(player_id.clone());

        let player = session.object_mut(&player_id).unwrap();
        player.set_property_int("max_weight", 100);
        player.set_property_map(
            "body_slots",
            HashMap::from([("right_hand".to_string(), heavy.id.clone())]),
        );
        session.upsert_object(heavy);

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
        let player_id = session.player_id().clone();
        let mut heavy = bare("item:crate-001", "Crate");
        heavy.set_property_numeric("weight", 92.0);
        heavy.location = Some(player_id.clone());

        let player = session.object_mut(&player_id).unwrap();
        player.set_property_int("max_weight", 100);
        player.set_property_map(
            "body_slots",
            HashMap::from([("right_hand".to_string(), heavy.id.clone())]),
        );
        session.upsert_object(heavy);

        let msg = session.go("north").unwrap();
        assert!(msg.contains("too encumbered to move easily"));
        assert!(msg.contains("head north"));
        assert_eq!(
            session.current_location().map(|id| id.as_str()),
            Some("room:north-001")
        );
    }

    #[tokio::test]
    async fn go_unencumbered_when_wearing_carry_modifiers() {
        use crate::object::WearableSpec;

        let (_persistence, mut session) = sample_session().await;
        let player_id = session.player_id().clone();
        let mut heavy = bare("item:crate-001", "Crate");
        heavy.set_property_numeric("weight", 92.0);
        heavy.location = Some(player_id.clone());

        let mut boots = bare("item:boots-001", "Boots of Carrying");
        let mut boot_spec = WearableSpec::new("left_foot", 2.0, 2.0);
        boot_spec.mod_max_weight = Some(25);
        boot_spec.mod_encumbrance = Some(0.85);
        boots.apply_wearable_role(&boot_spec);
        boots.location = Some(player_id.clone());

        let player = session.object_mut(&player_id).unwrap();
        player.set_property_int("max_weight", 100);
        player.set_property_map(
            "body_slots",
            HashMap::from([
                ("right_hand".to_string(), heavy.id.clone()),
                ("left_foot".to_string(), boots.id.clone()),
            ]),
        );
        session.upsert_object(heavy);
        session.upsert_object(boots);

        let msg = session.go("north").unwrap();
        assert!(!msg.contains("too encumbered to move easily"));
        assert!(msg.contains("head north"));
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
            .load_object(session.player_id())
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
        assert_eq!(
            bars.location.as_ref().map(|id| id.as_str()),
            Some("room:void-001")
        );
    }

    #[tokio::test]
    async fn session_dig_creates_and_links_new_place() {
        use crate::object::ObjectFactory;
        use crate::world::place_builder::DigOptions;

        let (persistence, mut session) = sample_session().await;
        let factory = ObjectFactory::new(persistence.clone());
        let result = session
            .dig_place(
                &factory,
                DigRequest {
                    direction: "east".to_string(),
                    name: "Side Chamber".to_string(),
                    options: DigOptions {
                        place_type: Some("room".to_string()),
                        description: Some("A small side room.".to_string()),
                        reciprocal: Some(true),
                        return_exit: None,
                    },
                },
            )
            .await
            .unwrap();

        assert_eq!(result.new_place.name, "Side Chamber");
        assert!(result.new_place.is_room());
        let room = session.object(session.current_location().unwrap()).unwrap();
        assert_eq!(room.get_exits().get("east"), Some(&result.new_place.id));
    }

    #[tokio::test]
    async fn go_ignores_non_passable_window_on_shared_direction() {
        use crate::object::{PortalKind, PortalSpec};

        let player_id = ObjectId::new("player:hero-001");
        let hall_id = ObjectId::new("area:hall-001");
        let pantry_id = ObjectId::new("room:pantry-001");
        let rear_id = ObjectId::new("area:rear-001");

        let mut player = bare("player:hero-001", "Hero");
        player.location = Some(hall_id.clone());

        let mut hall = bare("area:hall-001", "Hall");
        hall.add_exit("east", pantry_id.clone());

        let mut pantry = bare("room:pantry-001", "Pantry");
        pantry.location = Some(hall_id.clone());
        pantry.add_exit("west", hall_id.clone());

        let rear = bare("area:rear-001", "Rear Yard");
        let mut window = bare("item:window-001", "Window");
        window.location = Some(hall_id.clone());
        window.apply_portal_role(&PortalSpec {
            kind: PortalKind::Window,
            direction: "east".to_string(),
            destination: "rear".to_string(),
            open: false,
            lock_id: None,
            locked: false,
            lock_consumable: false,
            passable: None,
            transparent: None,
        });
        window.set_portal_destination(rear_id);

        let mut objects = HashMap::new();
        objects.insert(player.id.clone(), player);
        objects.insert(hall.id.clone(), hall);
        objects.insert(pantry.id.clone(), pantry);
        objects.insert(rear.id.clone(), rear);
        objects.insert(window.id.clone(), window);

        let mut session = Session::test_session(player_id, human_anatomy(), objects, Some(hall_id));
        session.go("east").unwrap();
        assert_eq!(session.current_location(), Some(&pantry_id));
    }
}