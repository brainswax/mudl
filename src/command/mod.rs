//! Command-layer helpers shared by REPL and future frontends.

use std::collections::HashMap;

use crate::inventory::{take_item, InventoryContext, InventoryError};
use crate::mudl::{load_module, AnatomyRegistry, LoadedUniverse, MudlRoleProps};
use crate::object::{ContainerSpec, Object, ObjectFactory, ObjectId, WearableSpec};
use crate::persistence::Persistence;
use crate::world::{
    bootstrap_world, bundle_module, persist_all, persist_dirty, DirtyTracker, ModuleManifest,
};

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

/// Parse `create <type> <name...>` supporting multi-word and quoted display names.
pub fn parse_create_args(parts: &[&str], input: &str) -> anyhow::Result<(String, String)> {
    if parts.len() < 3 {
        anyhow::bail!("Usage: create <type> <name...>");
    }
    let type_name = parts[1].to_ascii_lowercase();
    let rest = input
        .trim()
        .strip_prefix("create")
        .ok_or_else(|| anyhow::anyhow!("Usage: create <type> <name...>"))?
        .trim_start();
    let name_part = if let Some(stripped) = rest.strip_prefix(parts[1]) {
        stripped.trim_start().to_string()
    } else {
        parts[2..].join(" ")
    };
    let display_name = parse_display_name(&name_part)?;
    if display_name.is_empty() {
        anyhow::bail!("Usage: create <type> <name...>");
    }
    Ok((type_name, display_name))
}

fn parse_display_name(raw: &str) -> anyhow::Result<String> {
    let trimmed = raw.trim();
    if trimmed.len() >= 2 {
        if let Some(inner) = trimmed.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            return Ok(inner.to_string());
        }
        if let Some(inner) = trimmed
            .strip_prefix('\'')
            .and_then(|s| s.strip_suffix('\''))
        {
            return Ok(inner.to_string());
        }
    }
    Ok(trimmed.to_string())
}

/// Optional wizard overrides for `@create` (capacity, weight limits, stack count, etc.).
#[derive(Debug, Clone, Default)]
pub struct CreateOptions {
    pub capacity: Option<u32>,
    pub max_weight: Option<i64>,
    pub max_volume: Option<i64>,
    pub wearable: Option<bool>,
    pub wear_slot: Option<String>,
    pub stack_count: Option<u32>,
    pub prototype: Option<ObjectId>,
    pub mudl_props: MudlRoleProps,
}

/// Parse `key=value` tokens from `@create` trailing arguments.
pub fn parse_create_options(tokens: &[&str]) -> CreateOptions {
    let mut opts = CreateOptions::default();
    for token in tokens {
        if let Some((key, value)) = token.split_once('=') {
            match key {
                "capacity" => opts.capacity = value.parse().ok(),
                "max_weight" | "weight_limit" => opts.max_weight = value.parse().ok(),
                "max_volume" | "volume_limit" => opts.max_volume = value.parse().ok(),
                "wearable" => opts.wearable = Some(value == "true"),
                "wear_slot" => opts.wear_slot = Some(value.to_string()),
                "count" | "stack_count" => opts.stack_count = value.parse().ok(),
                "prototype" => opts.prototype = Some(ObjectId::new(value)),
                _ => {
                    opts.mudl_props = MudlRoleProps::from_pairs(&[(key, value.trim_matches('"'))]);
                }
            }
        }
    }
    opts
}

/// Create an object and place it at the player's current location when one is set.
pub async fn create_at_location<P: Persistence>(
    factory: &ObjectFactory<P>,
    type_name: &str,
    display_name: &str,
    owner: ObjectId,
    location: Option<&ObjectId>,
    anatomy: &AnatomyRegistry,
) -> anyhow::Result<Object> {
    create_at_location_with_options(
        factory,
        type_name,
        display_name,
        owner,
        location,
        anatomy,
        CreateOptions::default(),
    )
    .await
}

/// Create with wizard options (`@create container "Bag" capacity=10`).
pub async fn create_at_location_with_options<P: Persistence>(
    factory: &ObjectFactory<P>,
    type_name: &str,
    display_name: &str,
    owner: ObjectId,
    location: Option<&ObjectId>,
    anatomy: &AnatomyRegistry,
    options: CreateOptions,
) -> anyhow::Result<Object> {
    let mut obj = match type_name {
        "player" => {
            factory
                .create_player(display_name, owner.clone(), anatomy)
                .await?
        }
        "item" => factory.create_item(display_name, owner.clone()).await?,
        "container" => {
            factory
                .create_container_with_spec(
                    display_name,
                    owner.clone(),
                    ContainerSpec {
                        capacity: options.capacity.unwrap_or(10),
                        max_weight: options.max_weight,
                        max_volume: options.max_volume,
                        wearable: options.wearable.unwrap_or(true),
                        wear_slot: options.wear_slot.clone(),
                    },
                    options.prototype.clone(),
                )
                .await?
        }
        "wearable" => {
            factory
                .create_wearable(
                    display_name,
                    owner.clone(),
                    WearableSpec {
                        wear_slot: options
                            .wear_slot
                            .clone()
                            .unwrap_or_else(|| "torso".to_string()),
                        weight: options.max_weight.unwrap_or(1),
                        volume: options.max_volume.unwrap_or(1),
                    },
                    options.prototype.clone(),
                )
                .await?
        }
        "stackable" => {
            factory
                .create_stackable_item(
                    display_name,
                    owner.clone(),
                    options.prototype.clone(),
                    options.stack_count.unwrap_or(1),
                )
                .await?
        }
        other => {
            let slug = crate::object::slugify_display_name(display_name);
            let mut obj = factory
                .create_named(other, &slug, display_name, owner)
                .await?;
            if let Some(proto) = &options.prototype {
                obj.prototype = Some(proto.clone());
            }
            if options.mudl_props != MudlRoleProps::default() {
                options.mudl_props.apply_to(&mut obj);
                factory.persistence().save_object(&obj).await?;
            }
            obj
        }
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

/// Persist only dirty objects; falls back to full persist when tracker is empty.
pub async fn persist_inventory_dirty<P: Persistence>(
    persistence: &P,
    objects: &HashMap<ObjectId, Object>,
    dirty: &mut DirtyTracker,
) -> anyhow::Result<()> {
    if dirty.is_empty() {
        persist_all(persistence, objects).await?;
    } else {
        persist_dirty(persistence, objects, dirty).await?;
    }
    Ok(())
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
    Ok(crate::display::narrate_soft_delete(&name))
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
    Ok(crate::display::narrate_restore(&name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::{narrate_create, Describable, DisplayContext, DisplayMode};
    use crate::inventory::describe_carried;
    use crate::object::slugify_display_name;
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

    #[test]
    fn parse_create_args_multi_word_name() {
        let input = "create sword Rusty Sword";
        let parts: Vec<&str> = input.split_whitespace().collect();
        let (ty, name) = parse_create_args(&parts, input).unwrap();
        assert_eq!(ty, "sword");
        assert_eq!(name, "Rusty Sword");
    }

    #[test]
    fn parse_create_args_quoted_name() {
        let input = r#"create sword "Rusty Sword""#;
        let parts: Vec<&str> = input.split_whitespace().collect();
        let (ty, name) = parse_create_args(&parts, input).unwrap();
        assert_eq!(ty, "sword");
        assert_eq!(name, "Rusty Sword");
    }

    #[test]
    fn slugify_produces_lowercase_hyphenated_id_base() {
        assert_eq!(slugify_display_name("Rusty Sword"), "rusty-sword");
        assert_eq!(
            slugify_display_name("  Big   Red  Boots  "),
            "big-red-boots"
        );
    }

    #[tokio::test]
    async fn create_multi_word_name_generates_lowercase_id() {
        let factory = test_factory().await;
        let anatomy = test_anatomy();
        let owner = ObjectId::new("player:hero-001");
        let area_id = ObjectId::new("area:the-void-001");

        let sword = create_at_location(
            &factory,
            "sword",
            "Rusty Sword",
            owner,
            Some(&area_id),
            &anatomy,
        )
        .await
        .unwrap();

        assert_eq!(sword.name, "Rusty Sword");
        assert_eq!(sword.id.as_str(), "sword:rusty-sword-001");
        assert_eq!(sword.location.as_ref(), Some(&area_id));
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

        let ctx =
            DisplayContext::new(owner.clone(), DisplayMode::Player).with_objects(objects.clone());
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

        let msg =
            take_from_location(&owner, Some(&area_id), "boots", &mut objects, &anatomy).unwrap();
        assert_eq!(msg, "You pick up the Boots.");

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
    async fn immersive_create_take_look_self_flow() {
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

        let mut player = factory
            .create_player("hero", owner.clone(), &anatomy)
            .await
            .unwrap();
        player.location = Some(area_id.clone());

        let sword = create_at_location(
            &factory,
            "sword",
            "Rusty Sword",
            owner.clone(),
            Some(&area_id),
            &anatomy,
        )
        .await
        .unwrap();

        let create_msg = narrate_create(&sword, Some(&area));
        assert!(create_msg.contains("Rusty Sword"));
        assert!(create_msg.contains("The Void"));
        assert!(!create_msg.contains(':'));

        let mut objects = HashMap::new();
        objects.insert(player.id.clone(), player.clone());
        objects.insert(area_id.clone(), area);
        objects.insert(sword.id.clone(), sword.clone());

        let take_msg = take_from_location(
            &owner,
            Some(&area_id),
            "rusty sword",
            &mut objects,
            &anatomy,
        )
        .unwrap();
        assert!(take_msg.contains("Rusty Sword"));
        assert!(!take_msg.contains(':'));

        let player = objects.get(&owner).unwrap();
        let ctx = DisplayContext::new(owner.clone(), DisplayMode::Player)
            .with_objects(objects.clone())
            .with_anatomy(anatomy.clone());
        let look_self = player.describe(&ctx);
        assert!(look_self.contains("Rusty Sword"));
        assert!(!look_self.contains("sword:rusty-sword"));
        assert!(!look_self.contains("player:hero"));

        let inventory = describe_carried(player, &objects, &anatomy);
        assert!(inventory.contains("Rusty Sword"));
        assert!(!inventory.contains(':'));
    }

    #[tokio::test]
    async fn wizard_create_container_with_capacity() {
        let factory = test_factory().await;
        let anatomy = test_anatomy();
        let owner = ObjectId::new("player:hero-001");
        let area_id = ObjectId::new("area:the-void-001");

        let mut opts = CreateOptions::default();
        opts.capacity = Some(3);
        opts.max_weight = Some(25);

        let bag = create_at_location_with_options(
            &factory,
            "container",
            "Leather Bag",
            owner,
            Some(&area_id),
            &anatomy,
            opts,
        )
        .await
        .unwrap();

        assert!(bag.is_container());
        assert_eq!(bag.container_capacity(), 3);
        assert_eq!(bag.container_max_weight(), Some(25));
    }

    #[tokio::test]
    async fn wizard_create_stackable_item() {
        let factory = test_factory().await;
        let anatomy = test_anatomy();
        let owner = ObjectId::new("player:hero-001");

        let mut opts = CreateOptions::default();
        opts.stack_count = Some(42);

        let coins = create_at_location_with_options(
            &factory,
            "stackable",
            "Gold Coin",
            owner,
            None,
            &anatomy,
            opts,
        )
        .await
        .unwrap();

        assert!(coins.is_stackable());
        assert_eq!(coins.stack_count(), 42);
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
        let err =
            take_from_location(&owner, Some(&area_id), "axe", &mut objects, &anatomy).unwrap_err();
        assert_eq!(err, InventoryError::HandsFull);
    }
}
