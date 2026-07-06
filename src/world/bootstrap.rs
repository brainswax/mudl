use std::collections::HashMap;

use crate::mudl::{ItemInstanceDef, ItemPrototypeDef, LoadedWorld, WorldDef};
use crate::object::{Object, ObjectFactory, ObjectId, PermissionFlags, Property, Value};
use crate::persistence::Persistence;
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
                if let Some(loc_id) = name_to_id.get(loc_base) {
                    obj.location = Some(loc_id.clone());
                }
            }
            for (dir, target_base) in &def.exits {
                if let Some(target_id) = name_to_id.get(target_base) {
                    obj.add_exit(dir, target_id.clone());
                }
            }
            factory.persistence().save_object(&obj).await?;
        }
    }

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
    use crate::inventory::{
        close_container, open_container, put_item, read_item, take_item, InventoryContext,
    };
    use crate::mudl::load_module;
    use crate::persistence::SqlitePersistence;

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
        assert_eq!(mailbox_contents, vec!["Folded Note"]);

        let chest = ground
            .iter()
            .find(|o| o.name == "Travel Chest")
            .unwrap();
        assert!(!chest.container_is_open(), "starting chest should be closed");
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
            Some("Supplies within the chest — mind the dark.")
        );
    }

    #[tokio::test]
    async fn starting_scene_mailbox_open_read_take_put_flow() {
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

        let open_msg = open_container(&mut ctx, "mailbox").unwrap();
        assert_eq!(
            open_msg,
            "You open the worn mailbox. Inside you see a folded note."
        );

        let read_note = read_item(&ctx, "note").unwrap();
        assert!(read_note.contains("Supplies within the chest"));

        let take_msg = take_item(&mut ctx, "note").unwrap();
        assert!(take_msg.contains("pick up"));

        open_container(&mut ctx, "chest").unwrap();
        let put_msg = put_item(&mut ctx, "note", "chest", None).unwrap();
        assert!(put_msg.contains("in the travel chest"));
        assert!(put_msg.contains("Folded Note") || put_msg.contains("folded note"));

        close_container(&mut ctx, "mailbox").unwrap();
        let mailbox = ctx
            .objects
            .values()
            .find(|o| o.name == "Worn Mailbox")
            .unwrap();
        assert!(!mailbox.container_is_open());
    }
}