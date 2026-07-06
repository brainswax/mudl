use std::collections::HashMap;

use crate::mudl::{ItemInstanceDef, ItemPrototypeDef, LoadedWorld, WorldDef};
use crate::object::{Object, ObjectFactory, ObjectId, PermissionFlags, Property, Value};
use crate::persistence::Persistence;
use crate::world::exits::validate_world_places;
use crate::world::session::resolve_bootstrap_location;

fn location_id(def: &WorldDef) -> ObjectId {
    ObjectId::new(format!("{}:{}-001", def.obj_type, def.base_name))
}

fn find_location_def<'a>(world: &'a LoadedWorld, base_name: &str) -> Option<&'a WorldDef> {
    world
        .world_defs
        .iter()
        .find(|def| def.base_name == base_name)
}

fn prototype_display_name<'a>(
    prototype_base: &'a str,
    prototypes: &'a [ItemPrototypeDef],
) -> Option<&'a str> {
    prototypes
        .iter()
        .find(|p| p.base_name == prototype_base)
        .and_then(|p| p.name.as_deref().or(Some(p.base_name.as_str())))
}

fn instance_display_name<'a>(
    def: &'a ItemInstanceDef,
    prototypes: &'a [ItemPrototypeDef],
) -> &'a str {
    if let Some(name) = def.name.as_deref() {
        return name;
    }
    if let Some(proto) = def.prototype.as_deref() {
        if let Some(name) = prototype_display_name(proto, prototypes) {
            return name;
        }
    }
    def.base_name.as_str()
}

fn merged_aliases(def: &ItemInstanceDef, prototypes: &[ItemPrototypeDef]) -> Vec<String> {
    let mut aliases = def.aliases.clone();
    if let Some(proto) = def.prototype.as_ref() {
        if let Some(proto_def) = prototypes.iter().find(|p| &p.base_name == proto) {
            for alias in &proto_def.aliases {
                if !aliases.contains(alias) {
                    aliases.push(alias.clone());
                }
            }
        }
    }
    aliases
}

async fn spawn_prototype<P: Persistence>(
    factory: &ObjectFactory<P>,
    owner: &ObjectId,
    def: &ItemPrototypeDef,
    ids: &mut HashMap<String, ObjectId>,
) -> anyhow::Result<()> {
    if ids.contains_key(&def.base_name) {
        return Ok(());
    }
    let name = def.name.as_deref().unwrap_or(&def.base_name);
    let obj = factory
        .create_from_mudl_spec(
            &def.base_name,
            name,
            owner.clone(),
            None,
            &def.props,
            def.description.as_deref(),
            &def.aliases,
        )
        .await?;
    ids.insert(def.base_name.clone(), obj.id);
    Ok(())
}

async fn spawn_instance<P: Persistence>(
    factory: &ObjectFactory<P>,
    owner: &ObjectId,
    def: &ItemInstanceDef,
    prototype_defs: &[ItemPrototypeDef],
    prototypes: &HashMap<String, ObjectId>,
    placements: &mut HashMap<String, ObjectId>,
    objects: &mut HashMap<ObjectId, Object>,
) -> anyhow::Result<()> {
    if placements.contains_key(&def.base_name) {
        return Ok(());
    }
    if def.location.is_empty() {
        anyhow::bail!("Item instance '{}' missing location", def.base_name);
    }

    let prototype_id = def
        .prototype
        .as_ref()
        .and_then(|name| prototypes.get(name).cloned());

    let name = instance_display_name(def, prototype_defs);
    let description = def
        .description
        .as_deref()
        .or_else(|| {
            def.prototype.as_deref().and_then(|p| {
                prototype_defs
                    .iter()
                    .find(|d| d.base_name == p)
                    .and_then(|d| d.description.as_deref())
            })
        });
    let aliases = merged_aliases(def, prototype_defs);
    let mut obj = factory
        .create_from_mudl_spec(
            &def.base_name,
            name,
            owner.clone(),
            prototype_id,
            &def.props,
            description,
            &aliases,
        )
        .await?;

    let parent_id = placements
        .get(&def.location)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Unknown location '{}' for item '{}'",
                def.location,
                def.base_name
            )
        })?;

    obj.location = Some(parent_id.clone());

    let mut parent = match objects.get(parent_id) {
        Some(obj) => obj.clone(),
        None => factory
            .load_object(parent_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Placement target '{}' not loaded", def.location))?,
    };

    if parent.is_container() {
        parent.add_to_list_property("contents", obj.id.clone());
        factory.persistence().save_object(&parent).await?;
        objects.insert(parent_id.clone(), parent);
    }

    if obj.is_portal() {
        if let Some(base) = obj.portal_destination_base() {
            let dest_id = placements.get(&base).ok_or_else(|| {
                anyhow::anyhow!(
                    "Unknown portal destination '{}' for item '{}'",
                    base,
                    def.base_name
                )
            })?;
            obj.set_portal_destination(dest_id.clone());
        }
    }

    factory.persistence().save_object(&obj).await?;
    objects.insert(obj.id.clone(), obj.clone());
    placements.insert(def.base_name.clone(), obj.id);
    Ok(())
}

/// Spawn MUDL-defined item prototypes and instances into persistence.
pub async fn bootstrap_world_items<P: Persistence>(
    factory: &ObjectFactory<P>,
    owner: &ObjectId,
    world: &LoadedWorld,
    area_ids: &HashMap<String, ObjectId>,
) -> anyhow::Result<HashMap<String, ObjectId>> {
    let mut prototype_ids: HashMap<String, ObjectId> = HashMap::new();
    let mut placements: HashMap<String, ObjectId> = area_ids.clone(); // grows with item base_names
    let mut objects: HashMap<ObjectId, Object> = HashMap::new();

    for def in &world.item_prototypes {
        spawn_prototype(factory, owner, def, &mut prototype_ids).await?;
    }

    let mut pending = world.item_instances.clone();
    while !pending.is_empty() {
        let mut remaining = Vec::new();
        let mut spawned_any = false;
        for def in pending {
            if placements.contains_key(&def.location) {
                spawn_instance(
                    factory,
                    owner,
                    &def,
                    &world.item_prototypes,
                    &prototype_ids,
                    &mut placements,
                    &mut objects,
                )
                .await?;
                spawned_any = true;
            } else {
                remaining.push(def);
            }
        }
        if !spawned_any {
            let names: Vec<_> = remaining.iter().map(|d| d.base_name.as_str()).collect();
            anyhow::bail!("Unresolved item instances: {names:?}");
        }
        pending = remaining;
    }

    Ok(placements)
}

/// Bootstrap world objects from a loaded MUDL world into persistence.
pub async fn bootstrap_world<P: Persistence>(
    factory: &ObjectFactory<P>,
    owner: ObjectId,
    world: &LoadedWorld,
) -> anyhow::Result<ObjectId> {
    if let Some(start_base) = &world.starting_location {
        if let Some(def) = find_location_def(world, start_base) {
            let start_id = location_id(def);
            if factory.load_object(&start_id).await?.is_some() {
                return resolve_bootstrap_location(factory, &owner, start_id).await;
            }
        }
    }

    if world.world_defs.is_empty() {
        anyhow::bail!("No world definitions in world {}", world.name);
    }

    let mut name_to_id: HashMap<String, ObjectId> = HashMap::new();

    for def in &world.world_defs {
        let mut obj = factory
            .create(&def.obj_type, &def.base_name, owner.clone())
            .await?;
        obj.name = def.name.clone();
        if let Some(desc) = &def.description {
            obj.add_property(Property {
                name: "description".to_string(),
                value: Value::String(desc.clone()),
                permissions: PermissionFlags::EVERYONE,
                behavior: None,
            });
        }
        factory.persistence().save_object(&obj).await?;
        name_to_id.insert(def.base_name.clone(), obj.id.clone());
    }

    for def in &world.world_defs {
        if let Some(id) = name_to_id.get(&def.base_name) {
            let mut obj = if let Some(o) = factory.load_object(id).await? {
                o
            } else {
                continue;
            };
            if let Some(loc_base) = &def.location {
                let loc_id = name_to_id.get(loc_base).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Place '{}' has unknown parent location '{}'",
                        def.base_name,
                        loc_base
                    )
                })?;
                obj.location = Some(loc_id.clone());
            }
            for (dir, target_base) in &def.exits {
                let target_id = name_to_id.get(target_base).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Exit '{}' from '{}' targets unknown place '{}'",
                        dir,
                        def.base_name,
                        target_base
                    )
                })?;
                obj.add_exit(dir, target_id.clone());
            }
            factory.persistence().save_object(&obj).await?;
        }
    }

    let mut place_objects: HashMap<ObjectId, Object> = HashMap::new();
    for id in name_to_id.values() {
        if let Some(obj) = factory.load_object(id).await? {
            place_objects.insert(id.clone(), obj);
        }
    }
    validate_world_places(&place_objects).map_err(|errors| {
        anyhow::anyhow!("Invalid world exit graph: {}", errors.join("; "))
    })?;

    bootstrap_world_items(factory, &owner, world, &name_to_id).await?;

    if factory.load_object(&owner).await?.is_none() {
        let mut player = factory
            .create_player("admin", owner.clone(), &world.anatomy)
            .await?;
        player.name = "Admin".to_string();
        if let Some(start_base) = &world.starting_location {
            if let Some(start_id) = name_to_id.get(start_base) {
                player.location = Some(start_id.clone());
            }
        }
        factory.persistence().save_object(&player).await?;
    }

    let start_id = if let Some(start_base) = &world.starting_location {
        if let Some(def) = find_location_def(world, start_base) {
            name_to_id
                .get(start_base)
                .cloned()
                .unwrap_or_else(|| location_id(def))
        } else {
            name_to_id
                .values()
                .next()
                .cloned()
                .unwrap_or_else(|| ObjectId::new("area:the-void-001"))
        }
    } else {
        name_to_id
            .values()
            .next()
            .cloned()
            .unwrap_or_else(|| ObjectId::new("area:the-void-001"))
    };

    resolve_bootstrap_location(factory, &owner, start_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repl::session::Session;
    use crate::world::exits::validate_world_places;
    use crate::inventory::{
        close_container, open_container, read_item, take_item, unlock_container,
        InventoryContext, InventoryError,
    };
    use crate::mudl::load_module;
    use crate::persistence::SqlitePersistence;

    #[tokio::test]
    async fn bootstrap_wires_starting_area_exits() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence.clone());
        let owner = ObjectId::new("player:admin-001");
        let world = load_module("modules/default").unwrap().active_world().unwrap().clone();

        let start = bootstrap_world(&factory, owner.clone(), &world)
            .await
            .unwrap();

        let clearing = factory.load_object(&start).await.unwrap().unwrap();
        assert_eq!(clearing.name, "West Clearing");
        let exits = clearing.get_exits();
        assert!(exits.contains_key("north"));
        assert!(exits.contains_key("east"));

        let objects: Vec<Object> = persistence.list_objects(false).await.unwrap();
        let forest = objects
            .iter()
            .find(|o| o.name == "Forest Path")
            .expect("forest path area");
        let cottage_rear = objects
            .iter()
            .find(|o| o.name == "Behind the Cottage")
            .expect("cottage rear area");
        assert_eq!(exits.get("north").unwrap(), &forest.id);
        assert_eq!(exits.get("east").unwrap(), &cottage_rear.id);

        let forest_exits = forest.get_exits();
        assert_eq!(forest_exits.get("south").unwrap(), &start);
        assert!(forest_exits.contains_key("north"));
    }

    #[tokio::test]
    async fn bootstrap_spawns_starting_scene_items() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence.clone());
        let owner = ObjectId::new("player:admin-001");
        let world = load_module("modules/default").unwrap().active_world().unwrap().clone();

        let start = bootstrap_world(&factory, owner.clone(), &world)
            .await
            .unwrap();
        assert_eq!(start.as_str(), "area:the-void-001");

        let area = factory
            .load_object(&start)
            .await
            .unwrap()
            .expect("starting area");
        assert_eq!(area.name, "West Clearing");

        let objects: Vec<Object> = persistence.list_objects(false).await.unwrap();
        let ground: Vec<_> = objects
            .iter()
            .filter(|o| o.location.as_ref() == Some(&start) && o.is_active())
            .collect();
        assert!(
            ground.iter().any(|o| o.name == "Worn Mailbox"),
            "mailbox on ground"
        );
        assert!(
            ground.iter().any(|o| o.name == "Travel Chest"),
            "chest on ground"
        );

        let mailbox = ground
            .iter()
            .find(|o| o.name == "Worn Mailbox")
            .unwrap();
        assert!(mailbox.is_container());
        assert!(!mailbox.container_is_open(), "starting mailbox should be closed");
        assert!(mailbox.is_readable());
        assert!(mailbox.read_text().is_some());
        let mailbox_contents: Vec<_> = mailbox
            .container_contents()
            .iter()
            .filter_map(|id| objects.iter().find(|o| &o.id == id))
            .map(|o| o.name.as_str())
            .collect();
        assert_eq!(mailbox_contents.len(), 3);
        assert!(mailbox_contents.contains(&"Brass Key"));
        assert!(mailbox_contents.contains(&"Cottage Key"));
        assert!(mailbox_contents.contains(&"Folded Note"));

        let key = objects.iter().find(|o| o.name == "Brass Key").unwrap();
        assert!(key.is_key());
        assert_eq!(key.key_lock_id().as_deref(), Some("chest-lock"));

        let chest = ground
            .iter()
            .find(|o| o.name == "Travel Chest")
            .unwrap();
        assert!(!chest.container_is_open(), "starting chest should be closed");
        assert!(chest.container_is_locked(), "starting chest should be locked");
        assert_eq!(chest.container_lock_id().as_deref(), Some("chest-lock"));
        let chest_contents: Vec<_> = chest
            .container_contents()
            .iter()
            .filter_map(|id| objects.iter().find(|o| &o.id == id))
            .map(|o| o.name.as_str())
            .collect();
        assert!(chest_contents.contains(&"Chipped Blade"));
        assert!(chest_contents.contains(&"Iron Lantern"));
        assert!(chest_contents.contains(&"Trail Rations"));
        assert!(chest_contents.contains(&"Tinderbox"));
        assert!(!chest_contents.contains(&"Folded Note"));

        let note = objects
            .iter()
            .find(|o| o.name == "Folded Note")
            .unwrap();
        assert!(note.is_readable());
        assert_eq!(
            note.read_text().as_deref(),
            Some("Mind the dark below — take the lantern first.")
        );
    }

    #[tokio::test]
    async fn starting_scene_mailbox_key_chest_unlock_flow() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence.clone());
        let player_id = ObjectId::new("player:admin-001");
        let world = load_module("modules/default").unwrap().active_world().unwrap().clone();
        let anatomy = world.anatomy.clone();

        let room_id = bootstrap_world(&factory, player_id.clone(), &world)
            .await
            .unwrap();

        let mut objects: HashMap<ObjectId, Object> = persistence
            .list_objects(false)
            .await
            .unwrap()
            .into_iter()
            .map(|o| (o.id.clone(), o))
            .collect();

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        assert_eq!(
            read_item(&ctx, "mailbox").unwrap(),
            "You read the worn mailbox:\n\nThe faded lettering reads: WEST CLEARING — Edge of Nowhere. For wanderers without a return address."
        );

        let err = read_item(&ctx, "note").unwrap_err();
        assert!(err.to_string().contains("don't see"));

        let open_mailbox = open_container(&mut ctx, "mailbox").unwrap();
        assert!(
            open_mailbox.starts_with("You open the worn mailbox."),
            "{open_mailbox}"
        );
        assert!(open_mailbox.contains("brass key"));
        assert!(open_mailbox.contains("cottage key"));
        assert!(open_mailbox.contains("folded note"));

        let read_note = read_item(&ctx, "note").unwrap();
        assert!(read_note.contains("Mind the dark below"));

        let take_key = take_item(&mut ctx, "brass key").unwrap();
        assert!(take_key.contains("pick up"));
        assert!(take_key.to_lowercase().contains("brass key"));

        let err = open_container(&mut ctx, "chest").unwrap_err();
        assert_eq!(
            err,
            InventoryError::ContainerLocked("travel chest".to_string())
        );

        let unlock_msg = unlock_container(&mut ctx, "chest", Some("brass key")).unwrap();
        assert_eq!(
            unlock_msg,
            "You unlock the travel chest with the brass key."
        );

        let open_chest = open_container(&mut ctx, "chest").unwrap();
        assert!(open_chest.starts_with("You open the travel chest."));
        assert!(!open_chest.contains("folded note"));

        close_container(&mut ctx, "mailbox").unwrap();
        let mailbox = ctx
            .objects
            .values()
            .find(|o| o.name == "Worn Mailbox")
            .unwrap();
        assert!(!mailbox.container_is_open());
    }

    #[tokio::test]
    async fn bootstrap_spawns_cottage_doors_with_resolved_destinations() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence.clone());
        let owner = ObjectId::new("player:admin-001");
        let world = load_module("modules/default").unwrap().active_world().unwrap().clone();

        bootstrap_world(&factory, owner, &world).await.unwrap();

        let objects: Vec<Object> = persistence.list_objects(false).await.unwrap();
        let cottage_front = objects
            .iter()
            .find(|o| o.name == "Front of Small Cottage")
            .expect("cottage front");
        let cottage_interior = objects
            .iter()
            .find(|o| o.name == "Cottage Interior")
            .expect("cottage interior");

        let front_door = objects
            .iter()
            .find(|o| {
                o.is_door()
                    && o.location.as_ref() == Some(&cottage_front.id)
                    && o.door_direction().as_deref() == Some("in")
            })
            .expect("front door");
        assert!(!front_door.gate_is_open());
        assert!(front_door.gate_is_locked());
        assert_eq!(
            front_door.door_destination().as_ref(),
            Some(&cottage_interior.id)
        );

        let interior_door = objects
            .iter()
            .find(|o| {
                o.is_door()
                    && o.location.as_ref() == Some(&cottage_interior.id)
                    && o.door_direction().as_deref() == Some("out")
            })
            .expect("interior door");
        assert!(!interior_door.gate_is_open());
        assert!(!interior_door.gate_is_locked());
        assert_eq!(
            interior_door.door_destination().as_ref(),
            Some(&cottage_front.id)
        );

        let cottage_rear = objects
            .iter()
            .find(|o| o.name == "Behind the Cottage")
            .expect("cottage rear");
        let interior_window = objects
            .iter()
            .find(|o| {
                o.is_window()
                    && o.location.as_ref() == Some(&cottage_interior.id)
                    && o.portal_direction().as_deref() == Some("rear")
            })
            .expect("interior window");
        assert!(!interior_window.portal_passable());
        assert!(interior_window.portal_transparent());
        assert!(interior_window.portal_allows_view());
        assert_eq!(
            interior_window.portal_destination().as_ref(),
            Some(&cottage_rear.id)
        );

        let cottage_bedroom = objects
            .iter()
            .find(|o| o.name == "Bedroom")
            .expect("cottage bedroom");
        let boots = objects
            .iter()
            .find(|o| {
                o.name == "Boots of Carrying"
                    && o.location.as_ref() == Some(&cottage_bedroom.id)
            })
            .expect("boots of carrying in cottage bedroom");
        assert!(boots.is_wearable());
        assert_eq!(boots.carry_max_weight_bonus(), 25);
        assert!((boots.carry_encumbrance_factor() - 0.85).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn cottage_interior_window_shows_rear_view_on_look() {
        use crate::display::format_room_look_player;
        use crate::display::{DisplayContext, DisplayMode};

        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence.clone());
        let owner = ObjectId::new("player:admin-001");
        let world = load_module("modules/default").unwrap().active_world().unwrap().clone();

        bootstrap_world(&factory, owner, &world).await.unwrap();

        let objects: Vec<Object> = persistence.list_objects(false).await.unwrap();
        let interior_id = objects
            .iter()
            .find(|o| o.name == "Cottage Interior")
            .expect("cottage interior")
            .id
            .clone();

        let object_map: HashMap<ObjectId, Object> =
            objects.into_iter().map(|o| (o.id.clone(), o)).collect();
        let interior = object_map
            .get(&interior_id)
            .expect("interior object")
            .clone();
        let look = format_room_look_player(
            &interior,
            &DisplayContext::new(ObjectId::new("player:hero-001"), DisplayMode::Player)
                .with_objects(object_map),
        );
        assert!(look.contains("Through the rear window you see:"));
        assert!(look.contains("stacked firewood"));
    }

    #[tokio::test]
    async fn bootstrap_wires_cottage_room_hierarchy_and_exits() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence.clone());
        let owner = ObjectId::new("player:admin-001");
        let world = load_module("modules/default").unwrap().active_world().unwrap().clone();

        bootstrap_world(&factory, owner, &world).await.unwrap();

        let objects: Vec<Object> = persistence.list_objects(false).await.unwrap();
        let interior = objects
            .iter()
            .find(|o| o.name == "Cottage Interior")
            .expect("cottage interior");
        let bedroom = objects
            .iter()
            .find(|o| o.name == "Bedroom")
            .expect("bedroom");
        let pantry = objects
            .iter()
            .find(|o| o.name == "Pantry")
            .expect("pantry");

        assert!(interior.is_area());
        assert!(bedroom.is_room());
        assert!(pantry.is_room());
        assert_eq!(bedroom.location.as_ref(), Some(&interior.id));
        assert_eq!(pantry.location.as_ref(), Some(&interior.id));

        let object_map: HashMap<ObjectId, Object> =
            objects.into_iter().map(|o| (o.id.clone(), o)).collect();
        validate_world_places(&object_map).unwrap();

        let interior = object_map
            .values()
            .find(|o| o.name == "Cottage Interior")
            .unwrap();
        let exits = interior.get_exits();
        assert!(exits.contains_key("west"));
        assert!(exits.contains_key("east"));
    }

    #[tokio::test]
    async fn cottage_room_movement_and_persist_reload() {
        use crate::world::session::{hydrate_world, persist_all, resolve_player_location};

        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence.clone());
        let player_id = ObjectId::new("player:admin-001");
        let world = load_module("modules/default").unwrap().active_world().unwrap().clone();
        let anatomy = world.anatomy.clone();

        let start = bootstrap_world(&factory, player_id.clone(), &world)
            .await
            .unwrap();

        let mut objects: HashMap<ObjectId, Object> = persistence
            .list_objects(false)
            .await
            .unwrap()
            .into_iter()
            .map(|o| (o.id.clone(), o))
            .collect();

        let pantry_id = objects
            .values()
            .find(|o| o.name == "Pantry")
            .map(|o| o.id.clone())
            .expect("pantry");

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&start),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };
        open_container(&mut ctx, "mailbox").unwrap();
        take_item(&mut ctx, "cottage key").unwrap();
        drop(ctx);

        let mut session = Session::test_session(
            player_id.clone(),
            anatomy.clone(),
            objects,
            Some(start.clone()),
        );
        session.go("north").unwrap();
        session.go("north").unwrap();
        unlock_container(&mut session.inventory_context(), "door", None).unwrap();
        open_container(&mut session.inventory_context(), "door").unwrap();
        session.go("in").unwrap();
        session.go("east").unwrap();
        assert_eq!(session.current_location(), Some(&pantry_id));

        persist_all(&persistence, session.objects()).await.unwrap();

        let reloaded = hydrate_world(&persistence).await.unwrap();
        let restored = resolve_player_location(&player_id, &reloaded, Some(start));
        assert_eq!(restored.as_ref(), Some(&pantry_id));

        let pantry = reloaded.get(&pantry_id).unwrap();
        assert!(pantry.is_room());
        assert_eq!(
            pantry.parent_place(&reloaded).map(|p| p.name.as_str()),
            Some("Cottage Interior")
        );
    }

    #[tokio::test]
    async fn cottage_door_unlock_open_and_passage_flow() {
        use crate::display::format_room_look_player;
        use crate::display::{DisplayContext, DisplayMode};

        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence.clone());
        let player_id = ObjectId::new("player:admin-001");
        let world = load_module("modules/default").unwrap().active_world().unwrap().clone();
        let anatomy = world.anatomy.clone();

        let start = bootstrap_world(&factory, player_id.clone(), &world)
            .await
            .unwrap();

        let mut objects: HashMap<ObjectId, Object> = persistence
            .list_objects(false)
            .await
            .unwrap()
            .into_iter()
            .map(|o| (o.id.clone(), o))
            .collect();

        let cottage_front_id = objects
            .values()
            .find(|o| o.name == "Front of Small Cottage")
            .map(|o| o.id.clone())
            .expect("cottage front");

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&start),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        open_container(&mut ctx, "mailbox").unwrap();
        take_item(&mut ctx, "cottage key").unwrap();

        let mut session = Session::test_session(
            player_id.clone(),
            anatomy.clone(),
            ctx.objects.clone(),
            Some(start.clone()),
        );
        session.go("north").unwrap();
        session.go("north").unwrap();
        assert_eq!(session.current_location(), Some(&cottage_front_id));

        let look = format_room_look_player(
            session.object(&cottage_front_id).unwrap(),
            &DisplayContext::new(player_id.clone(), DisplayMode::Player)
                .with_objects(session.objects().clone()),
        );
        assert!(look.contains("in (locked door)"));

        let blocked = session.go("in").unwrap_err().to_string();
        assert!(blocked.contains("locked"));

        let room_id = session.current_location().cloned();
        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: room_id.as_ref(),
            objects: session.objects_mut(),
            anatomy: &anatomy,
            dirty: None,
        };
        let unlock = unlock_container(&mut ctx, "door", None).unwrap();
        assert!(unlock.contains("unlock"));
        let open = open_container(&mut ctx, "door").unwrap();
        assert!(open.contains("open"));

        drop(ctx);
        let msg = session.go("in").unwrap();
        assert!(msg.contains("main hall"));
        assert_eq!(
            session
                .object(session.current_location().unwrap())
                .map(|o| o.name.as_str()),
            Some("Cottage Interior")
        );
    }
}