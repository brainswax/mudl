//! Command-layer helpers shared by REPL and future frontends.

use std::collections::HashMap;

use crate::inventory::{take_item, InventoryContext, InventoryError};
use crate::mudl::{load_module, AnatomyRegistry, LoadedUniverse};
use crate::object::{Object, ObjectFactory, ObjectId};
use crate::persistence::Persistence;
use crate::world::{bootstrap_world, bundle_module, persist_all, ModuleManifest};

/// Load the active MUDL universe from `MUDL_MODULE` / `MUDL_UNIVERSE` env or default.
pub fn load_active_universe() -> anyhow::Result<LoadedUniverse> {
    crate::mudl::load_module(crate::mudl::default_module_dir())
}

/// Bootstrap the active universe's world for a player.
pub async fn bootstrap_active_universe<P: Persistence>(
    factory: &ObjectFactory<P>,
    owner: ObjectId,
) -> anyhow::Result<(LoadedUniverse, crate::object::ObjectId)> {
    let universe = load_active_universe()?;
    let world = universe.active_world()?;
    let start = bootstrap_world(factory, owner, world).await?;
    Ok((universe, start))
}

/// Package a universe module directory for distribution.
pub fn package_module(module_dir: &str, output_dir: &str) -> anyhow::Result<ModuleManifest> {
    bundle_module(module_dir, output_dir)
}

/// Reload a universe module from disk (for hot-reload during development).
pub fn reload_universe(path: &str) -> anyhow::Result<LoadedUniverse> {
    load_module(path)
}

/// Create an object and place it at the player's current location when one is set.
pub async fn create_at_location<P: Persistence>(
    factory: &ObjectFactory<P>,
    type_name: &str,
    base_name: &str,
    owner: ObjectId,
    location: Option<&ObjectId>,
    anatomy: &AnatomyRegistry,
) -> anyhow::Result<Object> {
    let mut obj = match type_name {
        "player" => factory.create_player(base_name, owner.clone(), anatomy).await?,
        "item" => factory.create_item(base_name, owner.clone()).await?,
        "container" => {
            factory
                .create_container(base_name, owner.clone(), 10, true)
                .await?
        }
        _ => factory.create(type_name, base_name, owner).await?,
    };

    if let Some(loc_id) = location {
        obj.location = Some(loc_id.clone());
        factory.persistence().save_object(&obj).await?;
    }

    Ok(obj)
}

/// Pick up a visible item from the current location into the player's hand slots.
pub fn take_from_location(
    player_id: &ObjectId,
    location_id: Option<&ObjectId>,
    item_name: &str,
    objects: &mut HashMap<ObjectId, Object>,
    anatomy: &AnatomyRegistry,
) -> Result<String, InventoryError> {
    let mut ctx = InventoryContext {
        player_id,
        room_id: location_id,
        objects,
        anatomy,
    };
    take_item(&mut ctx, item_name)
}

/// Persist inventory-related changes (player + all touched objects).
pub async fn persist_inventory_changes<P: Persistence>(
    persistence: &P,
    objects: &HashMap<ObjectId, Object>,
) -> anyhow::Result<()> {
    persist_all(persistence, objects).await
}

/// Soft-delete an object by ID (wizard). Object remains in the database.
pub async fn soft_delete_object<P: Persistence>(
    persistence: &P,
    id: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
) -> anyhow::Result<String> {
    let mut obj = persistence
        .load_object(id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Object not found: {id}"))?;
    let name = obj.name.clone();
    obj.soft_delete();
    persistence.save_object(&obj).await?;
    objects.insert(id.clone(), obj);
    Ok(format!("Soft-deleted {name} ({id})."))
}

/// Restore a soft-deleted object by ID (wizard).
pub async fn undelete_object<P: Persistence>(
    persistence: &P,
    id: &ObjectId,
    objects: &mut HashMap<ObjectId, Object>,
) -> anyhow::Result<String> {
    let mut obj = persistence
        .load_object(id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Object not found: {id}"))?;
    if !obj.is_deleted {
        anyhow::bail!("{id} is not deleted.");
    }
    let name = obj.name.clone();
    obj.undelete();
    persistence.save_object(&obj).await?;
    objects.insert(id.clone(), obj);
    Ok(format!("Restored {name} ({id})."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::{Describable, DisplayContext, DisplayMode};
    use crate::inventory::describe_carried;
    use crate::persistence::SqlitePersistence;
    use crate::world::hydrate_world;

    async fn test_factory() -> ObjectFactory<SqlitePersistence> {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        ObjectFactory::new(persistence)
    }

    fn test_anatomy() -> AnatomyRegistry {
        load_module("modules/default")
            .unwrap()
            .active_world()
            .unwrap()
            .anatomy
            .clone()
    }

    fn make_area(id: &str, name: &str, desc: &str, owner: ObjectId) -> Object {
        let mut area = Object {
            id: ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: owner.clone(),
            permissions: crate::object::PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        };
        area.add_property(crate::object::Property {
            name: "description".to_string(),
            value: crate::object::Value::String(desc.to_string()),
            permissions: crate::object::PermissionFlags::EVERYONE,
            behavior: None,
        });
        area
    }

    #[tokio::test]
    async fn create_item_placed_at_current_location() {
        let factory = test_factory().await;
        let anatomy = test_anatomy();
        let owner = ObjectId::new("player:hero-001");
        let area_id = ObjectId::new("area:the-void-001");
        let area = make_area(
            "area:the-void-001",
            "The Void",
            "A featureless void.",
            owner.clone(),
        );

        let boots = create_at_location(
            &factory,
            "item",
            "boots",
            owner.clone(),
            Some(&area_id),
            &anatomy,
        )
        .await
        .unwrap();

        assert_eq!(boots.location.as_ref(), Some(&area_id));
        assert_eq!(boots.object_type(), "item");

        let loaded = factory.load_object(&boots.id).await.unwrap().unwrap();
        assert_eq!(loaded.location.as_ref(), Some(&area_id));

        let mut objects = HashMap::new();
        objects.insert(area_id.clone(), area);
        objects.insert(boots.id.clone(), boots);

        let ctx = DisplayContext::new(owner.clone(), DisplayMode::Player).with_objects(objects.clone());
        let output = objects.get(&area_id).unwrap().describe(&ctx);
        assert!(output.contains("boots"));
        assert!(output.contains("You see:"));
    }

    #[tokio::test]
    async fn take_item_from_area_into_hand() {
        let factory = test_factory().await;
        let anatomy = test_anatomy();
        let owner = ObjectId::new("player:hero-001");
        let area_id = ObjectId::new("area:the-void-001");

        let mut player = factory
            .create_player("hero", owner.clone(), &anatomy)
            .await
            .unwrap();
        player.location = Some(area_id.clone());

        let mut boots = create_at_location(
            &factory,
            "item",
            "boots",
            owner.clone(),
            Some(&area_id),
            &anatomy,
        )
        .await
        .unwrap();
        boots.name = "Boots".to_string();

        let area = make_area(
            "area:the-void-001",
            "The Void",
            "A featureless void.",
            owner.clone(),
        );

        let mut objects = HashMap::new();
        objects.insert(player.id.clone(), player.clone());
        objects.insert(area_id.clone(), area);
        objects.insert(boots.id.clone(), boots);

        let msg = take_from_location(&owner, Some(&area_id), "boots", &mut objects, &anatomy)
            .unwrap();
        assert_eq!(msg, "You take the Boots.");

        let player = objects.get(&owner).unwrap();
        assert!(
            player.body_slot_item("left_hand").is_some()
                || player.body_slot_item("right_hand").is_some()
        );

        let carried = describe_carried(player, &objects, &anatomy);
        assert!(carried.contains("Boots"));
    }

    #[tokio::test]
    async fn take_fails_when_item_not_visible() {
        let anatomy = test_anatomy();
        let owner = ObjectId::new("player:hero-001");
        let area_id = ObjectId::new("area:the-void-001");
        let mut objects = HashMap::new();

        let err = take_from_location(&owner, Some(&area_id), "boots", &mut objects, &anatomy)
            .unwrap_err();
        assert_eq!(err, InventoryError::NotFound("boots".to_string()));
    }

    #[tokio::test]
    async fn create_and_take_persist_through_hydrate() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence.clone());
        let anatomy = test_anatomy();
        let owner = ObjectId::new("player:hero-001");
        let area_id = ObjectId::new("area:the-void-001");

        let mut player = factory
            .create_player("hero", owner.clone(), &anatomy)
            .await
            .unwrap();
        player.location = Some(area_id.clone());
        factory.persistence().save_object(&player).await.unwrap();

        let boots = create_at_location(
            &factory,
            "item",
            "boots",
            owner.clone(),
            Some(&area_id),
            &anatomy,
        )
        .await
        .unwrap();

        let mut objects = hydrate_world(&persistence).await.unwrap();
        take_from_location(&owner, Some(&area_id), "boots", &mut objects, &anatomy).unwrap();
        persist_all(&persistence, &objects).await.unwrap();

        let restored = hydrate_world(&persistence).await.unwrap();
        let player = restored.get(&owner).unwrap();
        assert!(
            player.body_slot_item("left_hand").is_some()
                || player.body_slot_item("right_hand").is_some()
        );
        let held_id = player
            .body_slot_item("right_hand")
            .or_else(|| player.body_slot_item("left_hand"))
            .unwrap();
        let held = restored.get(&held_id).unwrap();
        assert_eq!(held.name, boots.name);
        assert_eq!(held.location.as_ref(), Some(&owner));
    }

    #[tokio::test]
    async fn soft_delete_hides_object_until_undelete() {
        let persistence = SqlitePersistence::new(":memory:").await.unwrap();
        let factory = ObjectFactory::new(persistence.clone());
        let anatomy = test_anatomy();
        let owner = ObjectId::new("player:hero-001");
        let area_id = ObjectId::new("area:the-void-001");

        let boots = create_at_location(
            &factory,
            "item",
            "boots",
            owner.clone(),
            Some(&area_id),
            &anatomy,
        )
        .await
        .unwrap();

        let mut cache = hydrate_world(&persistence).await.unwrap();
        soft_delete_object(&persistence, &boots.id, &mut cache)
            .await
            .unwrap();

        let visible = hydrate_world(&persistence).await.unwrap();
        assert!(!visible.contains_key(&boots.id));

        let loaded = persistence.load_object(&boots.id).await.unwrap().unwrap();
        assert!(loaded.is_deleted);

        undelete_object(&persistence, &boots.id, &mut cache)
            .await
            .unwrap();
        let restored = hydrate_world(&persistence).await.unwrap();
        assert!(restored.contains_key(&boots.id));
        assert!(restored.get(&boots.id).unwrap().is_active());
    }

    #[tokio::test]
    async fn take_fails_when_hands_full() {
        let factory = test_factory().await;
        let anatomy = test_anatomy();
        let owner = ObjectId::new("player:hero-001");
        let area_id = ObjectId::new("area:the-void-001");

        let mut player = factory
            .create_player("hero", owner.clone(), &anatomy)
            .await
            .unwrap();
        player.location = Some(area_id.clone());

        let mut sword = factory.create_item("sword", owner.clone()).await.unwrap();
        sword.name = "Sword".to_string();
        sword.set_property_string("hand_slot", "both");
        sword.location = Some(area_id.clone());

        let mut axe = factory.create_item("axe", owner.clone()).await.unwrap();
        axe.name = "Axe".to_string();
        axe.location = Some(area_id.clone());

        let mut objects = HashMap::new();
        objects.insert(player.id.clone(), player);
        objects.insert(sword.id.clone(), sword);
        objects.insert(axe.id.clone(), axe);

        take_from_location(&owner, Some(&area_id), "sword", &mut objects, &anatomy).unwrap();
        let err = take_from_location(&owner, Some(&area_id), "axe", &mut objects, &anatomy)
            .unwrap_err();
        assert_eq!(err, InventoryError::HandsFull);
    }
}