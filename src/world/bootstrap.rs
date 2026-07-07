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

/// Spawn MUDL-defined creature spawners into persistence.
pub async fn bootstrap_world_spawners<P: Persistence>(
    factory: &ObjectFactory<P>,
    owner: &ObjectId,
    world: &LoadedWorld,
    target_ids: &HashMap<String, ObjectId>,
) -> anyhow::Result<()> {
    use crate::creature::{
        apply_spawner_def, behavior_templates_to_property, spawn_templates_to_property,
    };
    use std::collections::HashMap as StdHashMap;

    let template_map: StdHashMap<_, _> = world
        .spawn_template_defs
        .iter()
        .map(|t| (t.base_name.clone(), t.clone()))
        .collect();

    for def in &world.spawner_defs {
        if def.location.is_empty() && def.target.is_empty() {
            anyhow::bail!(
                "Spawner '{}' missing location or target",
                def.base_name
            );
        }
        let spawner_id = ObjectId::new(format!("spawner:{}-001", def.base_name));
        if factory.load_object(&spawner_id).await?.is_some() {
            continue;
        }

        let (location, target_id) = if !def.target.is_empty() {
            let target_id = target_ids.get(&def.target).cloned().ok_or_else(|| {
                anyhow::anyhow!(
                    "Spawner '{}' targets unknown object '{}'",
                    def.base_name,
                    def.target
                )
            })?;
            (None, Some(target_id))
        } else {
            let location = target_ids.get(&def.location).cloned().ok_or_else(|| {
                anyhow::anyhow!(
                    "Spawner '{}' targets unknown location '{}'",
                    def.base_name,
                    def.location
                )
            })?;
            (Some(location), None)
        };

        let mut spawner = Object {
            id: spawner_id,
            name: format!("{} spawner", def.base_name),
            aliases: Vec::new(),
            location,
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        apply_spawner_def(&mut spawner, def, &template_map)?;
        if let Some(target_id) = target_id {
            spawner.set_property_object_ref("spawner_target", target_id);
        }
        spawner.add_property(spawn_templates_to_property(&world.spawn_template_defs));
        spawner.add_property(behavior_templates_to_property(&world.behavior_template_defs));
        factory.persistence().save_object(&spawner).await?;
    }
    Ok(())
}

/// Spawn MUDL-defined loot spawners into persistence.
pub async fn bootstrap_world_loot_spawners<P: Persistence>(
    factory: &ObjectFactory<P>,
    owner: &ObjectId,
    world: &LoadedWorld,
    target_ids: &HashMap<String, ObjectId>,
) -> anyhow::Result<()> {
    use crate::loot::{apply_loot_spawner_def, loot_templates_to_property};
    use std::collections::HashMap as StdHashMap;

    let template_map: StdHashMap<_, _> = world
        .loot_template_defs
        .iter()
        .map(|t| (t.base_name.clone(), t.clone()))
        .collect();

    for def in &world.loot_spawner_defs {
        if def.target.is_empty() {
            anyhow::bail!("Loot spawner '{}' missing target", def.base_name);
        }
        let spawner_id = ObjectId::new(format!("loot-spawner:{}-001", def.base_name));
        if factory.load_object(&spawner_id).await?.is_some() {
            continue;
        }
        let target_id = target_ids.get(&def.target).cloned().ok_or_else(|| {
            anyhow::anyhow!(
                "Loot spawner '{}' targets unknown location or object '{}'",
                def.base_name,
                def.target
            )
        })?;

        let mut spawner = Object {
            id: spawner_id,
            name: format!("{} loot spawner", def.base_name),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: owner.clone(),
            permissions: PermissionFlags::EVERYONE,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        apply_loot_spawner_def(&mut spawner, def, &template_map)?;
        spawner.set_property_object_ref("loot_spawner_target", target_id);
        spawner.add_property(loot_templates_to_property(&world.loot_template_defs));
        factory.persistence().save_object(&spawner).await?;
    }
    Ok(())
}

/// Spawn MUDL-defined NPCs into persistence.
pub async fn bootstrap_world_npcs<P: Persistence>(
    factory: &ObjectFactory<P>,
    owner: &ObjectId,
    world: &LoadedWorld,
    area_ids: &HashMap<String, ObjectId>,
) -> anyhow::Result<()> {
    for def in &world.npc_defs {
        if def.location.is_empty() {
            anyhow::bail!("NPC '{}' missing location", def.base_name);
        }
        let location = area_ids.get(&def.location).cloned();
        let behavior_templates: std::collections::HashMap<_, _> = world
            .behavior_template_defs
            .iter()
            .map(|t| (t.base_name.clone(), t.clone()))
            .collect();
        factory
            .create_npc(
                def,
                owner.clone(),
                &world.anatomy,
                location,
                &behavior_templates,
            )
            .await?;
    }
    Ok(())
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
            if !def.scatter_to.is_empty() {
                let scatter_ids: Vec<Value> = def
                    .scatter_to
                    .iter()
                    .filter_map(|base| name_to_id.get(base).map(|id| Value::ObjectRef(id.clone())))
                    .collect();
                if !scatter_ids.is_empty() {
                    obj.add_property(Property {
                        name: "scatter_to".to_string(),
                        value: Value::List(scatter_ids),
                        permissions: PermissionFlags::EVERYONE,
                        behavior: None,
                    });
                }
                if let Some(dir) = &def.scatter_direction {
                    obj.add_property(Property {
                        name: "scatter_direction".to_string(),
                        value: Value::String(dir.clone()),
                        permissions: PermissionFlags::EVERYONE,
                        behavior: None,
                    });
                }
            }
            if let Some(loop_base) = &def.loop_to {
                if let Some(loop_id) = name_to_id.get(loop_base) {
                    obj.add_property(Property {
                        name: "loop_to".to_string(),
                        value: Value::ObjectRef(loop_id.clone()),
                        permissions: PermissionFlags::EVERYONE,
                        behavior: None,
                    });
                }
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

    let placements = bootstrap_world_items(factory, &owner, world, &name_to_id).await?;
    bootstrap_world_npcs(factory, &owner, world, &name_to_id).await?;
    bootstrap_world_spawners(factory, &owner, world, &placements).await?;

    let mut loot_targets = placements.clone();
    for def in &world.npc_defs {
        loot_targets.insert(
            def.base_name.clone(),
            ObjectId::new(format!("npc:{}-001", def.base_name)),
        );
    }
    bootstrap_world_loot_spawners(factory, &owner, world, &loot_targets).await?;

    if factory.load_object(&owner).await?.is_none() {
        let mut player = factory
            .create_player("admin", owner.clone(), &world.anatomy)
            .await?;
        player.name = "Admin".to_string();
        if let Some(start_base) = &world.starting_location {
            if let Some(start_id) = name_to_id.get(start_base) {
                player.location = Some(start_id.clone());
                player.set_property_object_ref("home_location", start_id.clone());
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
        break_item, close_container, open_container, read_item, take_item, wield_item,
        InventoryContext,
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

        let open_chest = open_container(&mut ctx, "chest").unwrap();
        assert!(open_chest.contains("unlock the travel chest with the brass key"));
        assert!(open_chest.contains("open the travel chest"));
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

        let objects: HashMap<ObjectId, Object> = persistence
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

        let mut session = Session::test_session(
            player_id.clone(),
            anatomy.clone(),
            objects,
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

        session.go("south").unwrap();
        session.go("south").unwrap();
        open_container(&mut session.inventory_context(), "mailbox").unwrap();
        take_item(&mut session.inventory_context(), "cottage key").unwrap();
        session.go("north").unwrap();
        session.go("north").unwrap();

        let msg = session.go("in").unwrap();
        assert!(msg.contains("unlock"));
        assert!(msg.contains("open"));
        assert!(msg.contains("main hall"));
        assert_eq!(
            session
                .object(session.current_location().unwrap())
                .map(|o| o.name.as_str()),
            Some("Cottage Interior")
        );
    }

    #[tokio::test]
    async fn haunted_forest_whisper_charm_and_oak_are_consumable() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence.clone());
        let player_id = ObjectId::new("player:admin-001");
        let world = load_module("modules/default").unwrap().active_world().unwrap().clone();

        bootstrap_world(&factory, player_id, &world).await.unwrap();

        let objects: Vec<Object> = persistence.list_objects(false).await.unwrap();
        let charm = objects
            .iter()
            .find(|o| o.name == "Whisper Charm" && o.id.as_str().contains("chest-whisper"))
            .or_else(|| {
                objects
                    .iter()
                    .find(|o| o.name == "Whisper Charm" && o.key_consumable())
            })
            .expect("whisper charm instance spawned");
        assert!(
            charm.key_consumable(),
            "whisper charm should mark key_consumable"
        );

        let oak = objects
            .iter()
            .find(|o| o.name == "Hollow Oak")
            .expect("hollow oak spawned");
        assert!(
            oak.lock_consumable(),
            "hollow oak should mark lock_consumable"
        );
    }

    #[tokio::test]
    async fn haunted_forest_full_adventure() {
        use crate::inventory::read_item;
        use crate::world::exits::{apply_scatter_exit, pick_scatter_destination};

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

        validate_world_places(&objects).expect("haunted map validates");

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&start),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        let boulder_hint = read_item(&ctx, "boulder").unwrap();
        assert!(boulder_hint.contains("HOLLOW OAK"));

        open_container(&mut ctx, "mailbox").unwrap();
        take_item(&mut ctx, "brass key").unwrap();
        open_container(&mut ctx, "chest").unwrap();
        take_item(&mut ctx, "whisper charm").unwrap();
        drop(ctx);

        let mut session = Session::test_session(
            player_id.clone(),
            anatomy.clone(),
            objects,
            Some(start.clone()),
        );

        session.go("north").unwrap();
        let charm_id = session
            .objects()
            .get(&player_id)
            .and_then(|player| {
                player.carried_body_items().into_iter().find(|id| {
                    session
                        .objects()
                        .get(id)
                        .is_some_and(|o| o.name == "Whisper Charm" && o.is_key())
                })
            })
            .expect("carried whisper charm");

        let entry_msg = session.go("in").unwrap();
        assert!(entry_msg.contains("unlock"));
        assert!(entry_msg.contains("open"));
        assert!(entry_msg.contains("crumbles away"));
        assert!(entry_msg.contains("cannot be secured again"));
        assert!(entry_msg.contains("Tangled Threshold") || entry_msg.contains("held breath"));
        assert!(
            session.objects().get(&charm_id).is_some_and(|o| o.is_deleted),
            "whisper charm should be consumed opening the hollow oak"
        );
        let oak = session
            .objects()
            .values()
            .find(|o| o.name == "Hollow Oak" && o.id.as_str().contains("forest-hollow"))
            .expect("forest hollow oak portal");
        assert!(!oak.gate_has_lock(), "oak lock should be spent after entry");

        let wrong_turn = session.go("east").unwrap();
        assert_eq!(
            session.object(session.current_location().unwrap()).unwrap().name,
            "Tangled Threshold",
            "wrong turns loop silently to the threshold"
        );
        assert!(
            !wrong_turn.to_lowercase().contains("wither"),
            "dead-end names should not appear on silent loop"
        );
        assert!(
            !wrong_turn.to_lowercase().contains("you go"),
            "silent loop should not narrate movement"
        );

        session.go("north").unwrap();
        let moon_read = read_item(&mut session.inventory_context(), "marker").unwrap();
        assert!(moon_read.contains("MOON"));

        session.go("east").unwrap();
        session.go("south").unwrap();
        session.go("west").unwrap();
        session.go("north").unwrap();
        assert_eq!(
            session.object(session.current_location().unwrap()).unwrap().name,
            "Pale Heart of the Wood"
        );

        let heart_location = session.current_location().unwrap().clone();
        let reward_chest_id = session
            .objects()
            .values()
            .find(|o| {
                o.name == "Rootbound Chest"
                    && o.is_container()
                    && !o.is_deleted
                    && o.location.as_ref() == Some(&heart_location)
            })
            .map(|o| o.id.clone())
            .expect("heart reward chest at Pale Heart");
        assert_eq!(
            session
                .objects()
                .get(&reward_chest_id)
                .unwrap()
                .location
                .as_ref(),
            Some(&heart_location)
        );

        let mut ctx = session.inventory_context();
        let reward_msg = open_container(&mut ctx, "reward chest").unwrap();
        drop(ctx);
        assert!(
            reward_msg.contains("find"),
            "opening reward chest should spawn loot: {reward_msg}"
        );
        let reward_chest = session.objects().get(&reward_chest_id).unwrap();
        assert!(reward_chest.gate_is_open());
        let reward_names = [
            "Trail Rations",
            "Tinderbox",
            "Iron Lantern",
            "Chipped Blade",
            "Brass Key Ring",
            "Boots of Carrying",
        ];
        let spawned: Vec<_> = reward_chest
            .container_contents()
            .iter()
            .filter_map(|id| session.objects().get(id))
            .collect();
        assert!(
            spawned
                .iter()
                .any(|item| reward_names.contains(&item.name.as_str())),
            "reward chest should hold a weighted loot drop"
        );

        let heart = session
            .object(session.current_location().unwrap())
            .unwrap()
            .clone();
        let scatter_dest = pick_scatter_destination(&heart, &player_id, session.objects())
            .expect("scatter destination");
        let main_world = ["West Clearing", "Forest Path", "Behind the Cottage"];
        assert!(
            session
                .objects()
                .get(&scatter_dest)
                .map(|o| main_world.contains(&o.name.as_str()))
                .unwrap_or(false),
            "scatter lands in main world"
        );

        let exit_msg = session.go("out").unwrap();
        assert!(exit_msg.contains("spits you out"));
        assert_eq!(session.current_location(), Some(&scatter_dest));

        let heart_exits = heart.get_exits();
        let map_target = heart_exits.get("out").unwrap();
        assert_eq!(
            apply_scatter_exit(&heart, "out", map_target, &player_id, session.objects()),
            scatter_dest
        );

        match session
            .object(session.current_location().unwrap())
            .map(|o| o.name.as_str())
        {
            Some("West Clearing") => {
                session.go("north").unwrap();
            }
            Some("Behind the Cottage") => {
                session.go("west").unwrap();
                session.go("north").unwrap();
            }
            Some("Forest Path") => {}
            Some(other) => panic!("unexpected scatter landing: {other}"),
            None => panic!("nowhere after scatter"),
        }

        session.go("in").unwrap();
        assert_eq!(
            session.object(session.current_location().unwrap()).unwrap().name,
            "Tangled Threshold",
            "haunted forest is replayable"
        );
    }

    #[tokio::test]
    async fn bootstrap_equipment_modifiers_on_starting_gear() {
        use crate::creature::creature_effective_stat;

        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence.clone());
        let player_id = ObjectId::new("player:admin-001");
        let world = load_module("modules/default").unwrap().active_world().unwrap().clone();
        let anatomy = world.anatomy.clone();

        bootstrap_world(&factory, player_id.clone(), &world)
            .await
            .unwrap();

        let mut objects: HashMap<ObjectId, Object> = persistence
            .list_objects(false)
            .await
            .unwrap()
            .into_iter()
            .map(|o| (o.id.clone(), o))
            .collect();

        let blade = objects
            .values()
            .find(|o| o.name == "Chipped Blade")
            .expect("chipped blade in travel chest");
        assert_eq!(blade.equipment_stat_mods().get("strength").copied(), Some(2));

        let vest = objects
            .values()
            .find(|o| o.name == "Leather Vest")
            .expect("leather vest in cottage bedroom");
        assert_eq!(vest.equipment_max_health_bonus(), 5);
        assert_eq!(vest.equipment_skill_mods().get("survival").copied(), Some(1));

        let lantern = objects
            .values()
            .find(|o| o.name == "Iron Lantern")
            .expect("iron lantern in travel chest");
        assert_eq!(
            lantern.equipment_grant_effects(),
            vec!["iron_lantern_aura".to_string()]
        );

        let mut session = Session::test_session(
            player_id.clone(),
            anatomy.clone(),
            objects,
            Some(ObjectId::new("area:the-void-001")),
        );
        open_container(&mut session.inventory_context(), "mailbox").unwrap();
        take_item(&mut session.inventory_context(), "brass key").unwrap();
        open_container(&mut session.inventory_context(), "chest").unwrap();
        take_item(&mut session.inventory_context(), "chipped blade").unwrap();
        wield_item(&mut session.inventory_context(), "blade").unwrap();

        let player = session.object(&player_id).unwrap();
        assert_eq!(
            creature_effective_stat(player, "strength", session.objects(), &anatomy),
            12
        );
    }

    #[tokio::test]
    async fn bootstrap_attaches_behavior_templates_to_npcs() {
        use crate::creature::read_creature_behaviors;

        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence.clone());
        let owner = ObjectId::new("player:admin-001");
        let world = load_module("modules/default").unwrap().active_world().unwrap().clone();

        bootstrap_world(&factory, owner, &world).await.unwrap();

        let npc = factory
            .load_object(&ObjectId::new("npc:path-watcher-001"))
            .await
            .unwrap()
            .expect("path watcher");
        let entries = read_creature_behaviors(&npc);
        assert!(
            entries.iter().any(|e| e.template_name.as_deref() == Some("guard")),
            "path watcher should have guard behavior template"
        );
        assert!(
            entries.iter().any(|e| {
                e.entry_type == "script"
                    && e.text.as_deref()
                        == Some("The trees seem to lean closer when you pass.")
            }),
            "path watcher should keep inline on_enter script"
        );
    }

    #[tokio::test]
    async fn milestone3_creature_systems_initial() {
        use crate::creature::{
            apply_effect, creature_health, creature_is_defeated, creature_skill, creature_stat,
            damage_creature, run_creature_behaviors,
        };
        use crate::display::{
            creature::format_examine_creature_player, format_examine_self, Describable,
            DisplayMode,
        };

        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence.clone());
        let player_id = ObjectId::new("player:admin-001");
        let world = load_module("modules/default").unwrap().active_world().unwrap().clone();

        let start = bootstrap_world(&factory, player_id.clone(), &world)
            .await
            .unwrap();

        let player = factory.load_object(&player_id).await.unwrap().unwrap();
        assert_eq!(creature_health(&player), 100);
        assert_eq!(creature_stat(&player, "strength"), 10);
        assert_eq!(creature_skill(&player, "survival"), 0);
        assert_eq!(player.get_int_property("max_weight"), Some(100));

        let npc_id = ObjectId::new("npc:path-watcher-001");
        let npc = factory.load_object(&npc_id).await.unwrap().expect("path watcher");
        assert_eq!(npc.name, "Path Watcher");
        assert_eq!(npc.location.as_ref().map(|id| id.as_str()), Some("area:forest-path-001"));
        assert_eq!(creature_health(&npc), 100);

        let objects: HashMap<ObjectId, Object> = persistence
            .list_objects(false)
            .await
            .unwrap()
            .into_iter()
            .map(|o| (o.id.clone(), o))
            .collect();

        let self_examine = format_examine_self(&player, &objects, &world.anatomy);
        assert!(self_examine.contains("You feel fit."));
        assert!(self_examine.contains("Strength 10"));

        let npc_examine = format_examine_creature_player(&npc, &world.anatomy);
        assert!(npc_examine.contains("looks fit"));
        assert!(npc_examine.contains("Strength 10"));

        let forest_path = ObjectId::new("area:forest-path-001");
        let mut behavior_objects = objects.clone();
        let outcome = run_creature_behaviors(
            "on_enter",
            &forest_path,
            &player_id,
            &mut behavior_objects,
        );
        assert!(outcome.lines.len() >= 2, "guard + script should both fire");
        assert!(outcome
            .lines
            .iter()
            .any(|l| l.contains("trees seem to lean closer")));
        assert!(outcome.lines.iter().any(|l| l.contains("Halt")));

        let mut session = Session::test_session(
            player_id.clone(),
            world.anatomy.clone(),
            objects,
            Some(start),
        );
        let move_out = session.go("north").unwrap();
        assert!(move_out.contains("trees seem to lean closer"));

        let damage_msg = {
            let mut ctx = session.inventory_context();
            damage_creature(
                ctx.player_id,
                ctx.room_id,
                ctx.objects,
                ctx.anatomy,
                ctx.dirty,
                "path watcher",
                40,
            )
            .unwrap()
        };
        assert!(damage_msg.contains("damage") || damage_msg.contains("stagger"));
        let watcher = session.object(&npc_id).unwrap();
        assert_eq!(creature_health(watcher), 60);

        {
            let mut ctx = session.inventory_context();
            damage_creature(
                ctx.player_id,
                ctx.room_id,
                ctx.objects,
                ctx.anatomy,
                ctx.dirty,
                "self",
                90,
            )
            .unwrap();
        }
        let wounded = session.object(&player_id).unwrap();
        assert_eq!(creature_health(wounded), 10);
        assert!(!creature_is_defeated(wounded));

        let display_ctx = session.display_context(DisplayMode::Player);
        let watcher_describe = session.object(&npc_id).unwrap().describe(&display_ctx);
        assert!(watcher_describe.contains("wounded") || watcher_describe.contains("health"));

        let mut encumbered_player = session
            .object(&player_id)
            .expect("player")
            .clone();
        apply_effect(&mut encumbered_player, "weary", &world.anatomy);
        session.objects_mut().insert(player_id.clone(), encumbered_player);
        let weary_player = session.object(&player_id).unwrap();
        assert_eq!(
            crate::creature::effect_encumbrance_factor(weary_player),
            1.1
        );
    }

    #[tokio::test]
    async fn loot_spawner_adds_bonus_to_travel_chest_on_open() {
        use crate::loot::loot_spawners_for_target;

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

        let chest_id = objects
            .values()
            .find(|o| o.id.as_str().contains("scene-chest"))
            .map(|o| o.id.clone())
            .expect("travel chest");

        assert_eq!(
            loot_spawners_for_target(&chest_id, &objects).len(),
            1,
            "travel chest should have a loot spawner attached"
        );

        let mut ctx = InventoryContext {
            player_id: &player_id,
            room_id: Some(&start),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        open_container(&mut ctx, "mailbox").unwrap();
        take_item(&mut ctx, "brass key").unwrap();
        let before = ctx
            .objects
            .get(&chest_id)
            .unwrap()
            .container_contents()
            .len();

        let open_msg = open_container(&mut ctx, "chest").unwrap();
        assert!(open_msg.contains("bonus rations") || open_msg.contains("find"));
        let after = ctx
            .objects
            .get(&chest_id)
            .unwrap()
            .container_contents()
            .len();
        assert!(after > before, "opening chest should spawn bonus loot");

        let again = open_container(&mut ctx, "chest").unwrap();
        assert!(
            !again.contains("find"),
            "once=true loot spawner should not repeat"
        );
    }

    #[tokio::test]
    async fn breakable_pot_disables_spawner_and_drops_loot() {
        use crate::creature::{is_spawner, spawners_for_target};

        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence.clone());
        let player_id = ObjectId::new("player:admin-001");
        let world = load_module("modules/default").unwrap().active_world().unwrap().clone();

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

        let moon = ObjectId::new("area:haunted-moon-001");
        let pot_id = objects
            .values()
            .find(|o| o.id.as_str().contains("haunted-moon-pot"))
            .map(|o| o.id.clone())
            .expect("clay pot instance in haunted moon");
        assert_eq!(
            objects.get(&pot_id).unwrap().location.as_ref(),
            Some(&moon)
        );

        let pot_spawners: Vec<_> = objects
            .values()
            .filter(|o| is_spawner(o))
            .filter(|o| o.id.as_str().contains("pot-mist"))
            .collect();
        assert_eq!(pot_spawners.len(), 1, "pot mist spawner should bootstrap");
        assert_eq!(
            spawners_for_target(&pot_id, &objects).len(),
            1,
            "pot should have an attached creature spawner (pot_id={})",
            pot_id.as_str()
        );

        let mut session = Session::test_session(
            player_id.clone(),
            world.anatomy.clone(),
            objects,
            Some(start),
        );

        let mut ctx = session.inventory_context();
        open_container(&mut ctx, "mailbox").unwrap();
        take_item(&mut ctx, "brass key").unwrap();
        open_container(&mut ctx, "chest").unwrap();
        take_item(&mut ctx, "whisper charm").unwrap();
        drop(ctx);

        session.go("north").unwrap();
        session.go("in").unwrap();
        session.go("north").unwrap();
        assert_eq!(
            session.object(session.current_location().unwrap()).unwrap().name,
            "Moonlit Glade"
        );

        let wisp_count_before = session
            .objects()
            .values()
            .filter(|o| {
                o.object_type() == "npc"
                    && o.is_active()
                    && o.location.as_ref() == Some(&moon)
                    && o.name == "Mist Wisp"
            })
            .count();
        assert!(wisp_count_before >= 1, "pot spawner should seed a mist wisp on enter");

        let mut ctx = session.inventory_context();
        let break_msg = break_item(&mut ctx, "pot").unwrap();
        drop(ctx);

        assert!(break_msg.contains("smash the clay pot"));
        assert!(break_msg.contains("find") || break_msg.contains("rations"));
        assert!(
            session.objects().get(&pot_id).is_some_and(|o| o.is_deleted),
            "broken pot should be removed from play"
        );
        assert!(
            spawners_for_target(&pot_id, session.objects()).is_empty(),
            "pot creature spawner should be destroyed"
        );

        let moon_rations = session
            .objects()
            .values()
            .filter(|o| {
                o.is_active()
                    && o.name == "Trail Rations"
                    && o.location.as_ref() == Some(&moon)
            })
            .count();
        assert!(moon_rations >= 1, "breaking pot should spill loot into the glade");

        session.go("south").unwrap();
        session.go("north").unwrap();
        let wisp_count_after = session
            .objects()
            .values()
            .filter(|o| {
                o.object_type() == "npc"
                    && o.is_active()
                    && o.location.as_ref() == Some(&moon)
                    && o.name == "Mist Wisp"
                    && o.get_object_ref_property("spawned_by")
                        .is_some_and(|id| id.as_str().contains("pot-mist"))
            })
            .count();
        assert_eq!(
            wisp_count_after, 0,
            "pot spawner should not respawn wisps after the pot is broken"
        );
    }

    #[tokio::test]
    async fn creature_spawner_attached_to_haunted_moon() {
        use crate::creature::spawners_in_room;

        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence.clone());
        let player_id = ObjectId::new("player:admin-001");
        let world = load_module("modules/default").unwrap().active_world().unwrap().clone();

        bootstrap_world(&factory, player_id.clone(), &world)
            .await
            .unwrap();

        let objects: HashMap<ObjectId, Object> = persistence
            .list_objects(false)
            .await
            .unwrap()
            .into_iter()
            .map(|o| (o.id.clone(), o))
            .collect();

        let moon = ObjectId::new("area:haunted-moon-001");
        let clearing = ObjectId::new("area:the-void-001");
        assert!(
            spawners_in_room(&clearing, &objects).is_empty(),
            "starting clearing has no spawners"
        );
        assert_eq!(spawners_in_room(&moon, &objects).len(), 2);

        let mut session = Session::test_session(
            player_id.clone(),
            world.anatomy.clone(),
            objects,
            Some(clearing),
        );

        open_container(&mut session.inventory_context(), "mailbox").unwrap();
        take_item(&mut session.inventory_context(), "brass key").unwrap();
        open_container(&mut session.inventory_context(), "chest").unwrap();
        take_item(&mut session.inventory_context(), "whisper charm").unwrap();

        session.go("north").unwrap();
        session.go("in").unwrap();
        session.go("north").unwrap();

        let moon_npcs: Vec<_> = session
            .objects()
            .values()
            .filter(|o| {
                o.object_type() == "npc"
                    && o.location.as_ref() == Some(&moon)
                    && o.get_property("spawned_by").is_some()
            })
            .collect();
        assert!(
            !moon_npcs.is_empty(),
            "haunted-moon spawner should create a creature on enter"
        );
        assert!(
            moon_npcs
                .iter()
                .any(|o| o.name == "Mist Wisp" || o.name == "Pale Lurker")
        );
    }

    #[tokio::test]
    async fn path_watcher_kill_loot_spawner_bootstraps() {
        use crate::creature::attack_creature;
        use crate::loot::loot_spawners_for_target;

        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence.clone());
        let player_id = ObjectId::new("player:admin-001");
        let world = load_module("modules/default").unwrap().active_world().unwrap().clone();

        let start = bootstrap_world(&factory, player_id.clone(), &world)
            .await
            .unwrap();

        let npc_id = ObjectId::new("npc:path-watcher-001");
        let mut objects: HashMap<ObjectId, Object> = persistence
            .list_objects(false)
            .await
            .unwrap()
            .into_iter()
            .map(|o| (o.id.clone(), o))
            .collect();

        assert_eq!(
            loot_spawners_for_target(&npc_id, &objects).len(),
            1,
            "path watcher should have on_kill loot spawner"
        );

        let forest_path = ObjectId::new("area:forest-path-001");
        let mut session = Session::test_session(
            player_id.clone(),
            world.anatomy.clone(),
            objects,
            Some(forest_path.clone()),
        );
        session.objects_mut().get_mut(&player_id).unwrap().location =
            Some(forest_path.clone());
        session
            .objects_mut()
            .get_mut(&npc_id)
            .unwrap()
            .set_property_int("health", 1);

        let mut ctx = session.inventory_context();
        let outcome = attack_creature(
            ctx.player_id,
            ctx.room_id,
            ctx.objects,
            ctx.anatomy,
            ctx.dirty,
            "path watcher",
        )
        .unwrap();
        assert!(outcome.lines.iter().any(|l| l.contains("corpse")));
        assert!(
            outcome.lines.iter().any(|l| l.contains("rations")),
            "on_kill loot should drop trail rations: {:?}",
            outcome.lines
        );
        assert!(session.object(&npc_id).unwrap().is_deleted);
        let _ = start;
    }
}