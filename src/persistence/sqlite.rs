use anyhow::Result;
use async_trait::async_trait;
use sqlx::SqlitePool;

use std::path::Path;
use std::str::FromStr;

use sqlx::sqlite::SqliteConnectOptions;

use super::r#trait::Persistence;
use crate::object::{Object, ObjectId};

#[derive(Clone)]
pub struct SqlitePersistence {
    pool: SqlitePool,
}

impl SqlitePersistence {
    pub async fn new(database_url: &str) -> Result<Self> {
        let is_memory = database_url == ":memory:" || database_url.ends_with(":memory:");

        let connect_url = if is_memory {
            ":memory:".to_string()
        } else if database_url.starts_with("sqlite:") {
            database_url.to_string()
        } else {
            format!("sqlite:{}", database_url)
        };

        // Ensure parent directory exists for file-based databases
        if !is_memory {
            let path_str = connect_url
                .strip_prefix("sqlite:")
                .unwrap_or(&connect_url)
                .to_string();

            if let Some(parent) = Path::new(&path_str).parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)?;
                }
            }
        }

        let options = SqliteConnectOptions::from_str(&connect_url)?.create_if_missing(true);

        let pool = SqlitePool::connect_with(options).await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS objects (
                id TEXT PRIMARY KEY,
                data TEXT NOT NULL,
                is_deleted INTEGER NOT NULL DEFAULT 0,
                deleted_at TEXT
            );
            CREATE TABLE IF NOT EXISTS counters (
                type_base TEXT PRIMARY KEY,
                counter INTEGER NOT NULL DEFAULT 0
            );
            "#,
        )
        .execute(&pool)
        .await?;

        // Migrate older databases that lack soft-delete columns.
        sqlx::query("ALTER TABLE objects ADD COLUMN is_deleted INTEGER NOT NULL DEFAULT 0")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("ALTER TABLE objects ADD COLUMN deleted_at TEXT")
            .execute(&pool)
            .await
            .ok();

        Ok(Self { pool })
    }

    fn deleted_flag(object: &Object) -> i32 {
        i32::from(object.is_deleted)
    }
}

#[async_trait]
impl Persistence for SqlitePersistence {
    async fn save_object(&self, object: &Object) -> Result<()> {
        let id = object.id.to_string();
        let data = serde_json::to_string(object)?;

        sqlx::query(
            "INSERT OR REPLACE INTO objects (id, data, is_deleted, deleted_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&data)
        .bind(Self::deleted_flag(object))
        .bind(&object.deleted_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn load_object(&self, id: &ObjectId) -> Result<Option<Object>> {
        let data: Option<String> =
            sqlx::query_scalar::<_, String>("SELECT data FROM objects WHERE id = ?")
                .bind(id.to_string())
                .fetch_optional(&self.pool)
                .await?;

        if let Some(data) = data {
            let object: Object = serde_json::from_str(&data)?;
            Ok(Some(object))
        } else {
            Ok(None)
        }
    }

    async fn get_next_id_counter(&self, obj_type: &str, base_name: &str) -> Result<u32> {
        let key = format!("{}:{}", obj_type, base_name);
        let counter: Option<i64> =
            sqlx::query_scalar::<_, i64>("SELECT counter FROM counters WHERE type_base = ?")
                .bind(&key)
                .fetch_optional(&self.pool)
                .await?;

        Ok(counter.map(|c| (c + 1) as u32).unwrap_or(1))
    }

    async fn increment_counter(&self, obj_type: &str, base_name: &str) -> Result<u32> {
        let key = format!("{}:{}", obj_type, base_name);
        let new_counter: i64 = sqlx::query_scalar::<_, i64>(
            r#"
            INSERT INTO counters (type_base, counter) VALUES (?, 1)
            ON CONFLICT(type_base) DO UPDATE SET counter = counter + 1
            RETURNING counter
            "#,
        )
        .bind(&key)
        .fetch_one(&self.pool)
        .await?;

        Ok(new_counter as u32)
    }

    async fn list_objects(&self, include_deleted: bool) -> Result<Vec<Object>> {
        let rows: Vec<String> = if include_deleted {
            sqlx::query_scalar::<_, String>("SELECT data FROM objects ORDER BY id")
                .fetch_all(&self.pool)
                .await?
        } else {
            sqlx::query_scalar::<_, String>(
                "SELECT data FROM objects WHERE is_deleted = 0 ORDER BY id",
            )
            .fetch_all(&self.pool)
            .await?
        };

        rows.into_iter()
            .map(|data| serde_json::from_str(&data).map_err(Into::into))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::PermissionFlags;

    async fn memory_persistence() -> SqlitePersistence {
        SqlitePersistence::new(":memory:").await.unwrap()
    }

    fn sample_object(id: &str, name: &str) -> Object {
        Object {
            id: ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: ObjectId::new("player:admin-001"),
            permissions: PermissionFlags::OWNER,
            properties: Default::default(),
            verbs: Default::default(),
            event_handlers: Default::default(),
            is_deleted: false,
            deleted_at: None,
        }
    }

    #[tokio::test]
    async fn save_and_reload_object_roundtrip() {
        let persistence = memory_persistence().await;
        let mut boots = sample_object("item:boots-001", "boots");
        boots.location = Some(ObjectId::new("area:the-void-001"));

        persistence.save_object(&boots).await.unwrap();
        let loaded = persistence
            .load_object(&boots.id)
            .await
            .unwrap()
            .expect("boots should exist");

        assert_eq!(loaded.name, "boots");
        assert_eq!(
            loaded.location.as_ref().map(ObjectId::as_str),
            Some("area:the-void-001")
        );
    }

    #[tokio::test]
    async fn list_objects_excludes_soft_deleted() {
        let persistence = memory_persistence().await;
        let active = sample_object("item:boots-001", "boots");
        let mut deleted = sample_object("item:trash-001", "trash");
        deleted.soft_delete();

        persistence.save_object(&active).await.unwrap();
        persistence.save_object(&deleted).await.unwrap();

        let visible = persistence.list_objects(false).await.unwrap();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].id.as_str(), "item:boots-001");

        let all = persistence.list_objects(true).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn soft_deleted_object_still_loadable_by_id() {
        let persistence = memory_persistence().await;
        let mut boots = sample_object("item:boots-001", "boots");
        boots.soft_delete();
        persistence.save_object(&boots).await.unwrap();

        let loaded = persistence
            .load_object(&boots.id)
            .await
            .unwrap()
            .expect("soft-deleted object remains in DB");
        assert!(loaded.is_deleted);
        assert!(loaded.deleted_at.is_some());
    }

    #[tokio::test]
    async fn player_inventory_state_roundtrips() {
        let persistence = memory_persistence().await;
        let area_id = ObjectId::new("area:the-void-001");
        let player_id = ObjectId::new("player:admin-001");
        let item_id = ObjectId::new("item:boots-001");

        let area = sample_object("area:the-void-001", "The Void");
        let mut player = sample_object("player:admin-001", "Admin");
        player.location = Some(area_id.clone());
        player.set_property_map(
            "body_slots",
            [("right_hand".to_string(), item_id.clone())].into(),
        );

        let mut boots = sample_object("item:boots-001", "boots");
        boots.location = Some(player_id.clone());

        persistence.save_object(&area).await.unwrap();
        persistence.save_object(&player).await.unwrap();
        persistence.save_object(&boots).await.unwrap();

        let reloaded_player = persistence.load_object(&player_id).await.unwrap().unwrap();
        assert_eq!(
            reloaded_player.body_slot_item("right_hand").as_ref(),
            Some(&item_id)
        );

        let reloaded_boots = persistence.load_object(&item_id).await.unwrap().unwrap();
        assert_eq!(reloaded_boots.location.as_ref(), Some(&player_id));
    }

    #[tokio::test]
    async fn complex_object_graph_roundtrip() {
        use crate::object::{ContainerSpec, StackableSpec};

        let persistence = memory_persistence().await;
        let room_id = ObjectId::new("room:test-001");
        let player_id = ObjectId::new("player:hero-001");

        let room = sample_object("room:test-001", "Test Room");
        let mut player = sample_object("player:hero-001", "Hero");
        player.location = Some(room_id.clone());

        let mut backpack = sample_object("item:backpack-001", "backpack");
        let backpack_id = backpack.id.clone();
        backpack.apply_container_role(&ContainerSpec {
            capacity: 5,
            max_weight: Some(100),
            max_volume: None,
            wearable: true,
            wear_slot: Some("torso".to_string()),
        });

        let mut coins = sample_object("item:coins-001", "coins");
        coins.set_property_int("weight", 1);
        coins.apply_stackable_role(&StackableSpec {
            count: 20,
            max_stack: 99,
        });
        let coins_id = coins.id.clone();
        coins.location = Some(backpack_id.clone());

        let mut bars = sample_object("item:bars-001", "gold bar");
        let bars_id = bars.id.clone();
        bars.set_property_int("weight", 5);
        bars.apply_stackable_role(&StackableSpec {
            count: 3,
            max_stack: 99,
        });
        bars.location = Some(player_id.clone());

        backpack.set_property_list("contents", vec![coins_id.clone()]);
        player.set_body_slot("torso", Some(backpack_id.clone()));
        player.set_body_slot("right_hand", Some(bars_id.clone()));

        for obj in [&room, &player, &backpack, &coins, &bars] {
            persistence.save_object(obj).await.unwrap();
        }

        let reloaded_player = persistence.load_object(&player_id).await.unwrap().unwrap();
        assert_eq!(
            reloaded_player.body_slot_item("torso").as_ref(),
            Some(&backpack_id)
        );
        assert_eq!(
            reloaded_player.body_slot_item("right_hand").as_ref(),
            Some(&bars_id)
        );

        let reloaded_backpack = persistence.load_object(&backpack_id).await.unwrap().unwrap();
        assert_eq!(reloaded_backpack.container_contents(), vec![coins_id.clone()]);

        let reloaded_coins = persistence.load_object(&coins_id).await.unwrap().unwrap();
        assert_eq!(reloaded_coins.stack_count(), 20);
        assert_eq!(reloaded_coins.location.as_ref(), Some(&backpack_id));

        let reloaded_bars = persistence.load_object(&bars_id).await.unwrap().unwrap();
        assert_eq!(reloaded_bars.stack_count(), 3);
        assert_eq!(reloaded_bars.location.as_ref(), Some(&player_id));
    }

    /// Collect every object id referenced by location, prototype, contents, and body slots.
    fn referenced_ids(obj: &Object) -> Vec<ObjectId> {
        let mut refs = Vec::new();
        if let Some(loc) = &obj.location {
            refs.push(loc.clone());
        }
        if let Some(proto) = &obj.prototype {
            refs.push(proto.clone());
        }
        refs.extend(obj.container_contents());
        refs.extend(obj.body_slots().into_values());
        for prop in obj.properties.values() {
            collect_value_refs(&prop.value, &mut refs);
        }
        refs
    }

    fn collect_value_refs(value: &crate::object::Value, out: &mut Vec<ObjectId>) {
        use crate::object::Value;
        match value {
            Value::ObjectRef(id) => out.push(id.clone()),
            Value::List(items) => items.iter().for_each(|v| collect_value_refs(v, out)),
            Value::Map(map) => map.values().for_each(|v| collect_value_refs(v, out)),
            _ => {}
        }
    }

    fn assert_graph_references_resolve(objects: &std::collections::HashMap<ObjectId, Object>) {
        for obj in objects.values() {
            for id in referenced_ids(obj) {
                assert!(
                    objects.contains_key(&id),
                    "missing referenced object {} from {}",
                    id.as_str(),
                    obj.id.as_str()
                );
            }
        }
    }

    fn assert_objects_identical(
        before: &std::collections::HashMap<ObjectId, Object>,
        after: &std::collections::HashMap<ObjectId, Object>,
    ) {
        assert_eq!(
            before.len(),
            after.len(),
            "object count changed after reload"
        );
        for (id, original) in before {
            let reloaded = after
                .get(id)
                .unwrap_or_else(|| panic!("object {} missing after reload", id.as_str()));
            assert_eq!(
                original, reloaded,
                "object {} differs after persistence roundtrip",
                id.as_str()
            );
        }
    }

    #[tokio::test]
    async fn milestone1_complex_scene_persist_reload_identical() {
        use std::collections::HashMap;

        use crate::display::short_id;
        use crate::inventory::{drop_item, put_item, take_item, wear_item, InventoryContext};
        use crate::mudl::load_module;
        use crate::object::ObjectFactory;
        use crate::world::session::{hydrate_world, persist_all};

        let persistence = memory_persistence().await;
        let factory = ObjectFactory::new(persistence.clone());
        let anatomy = load_module("modules/default")
            .unwrap()
            .active_world()
            .unwrap()
            .anatomy
            .clone();
        let owner = ObjectId::new("player:hero-001");
        let room_id = ObjectId::new("room:test-001");

        let mut player = factory
            .create_player("hero", owner.clone(), &anatomy)
            .await
            .unwrap();
        player.location = Some(room_id.clone());

        let mut room = factory.create("room", "test", owner.clone()).await.unwrap();
        room.name = "Test Room".to_string();

        let mut backpack = factory
            .create_container("backpack", owner.clone(), 5, true)
            .await
            .unwrap();
        backpack.name = "backpack".to_string();
        backpack.location = Some(room_id.clone());

        let mut bars = factory
            .create_stackable_item("gold bar", owner.clone(), None, 10)
            .await
            .unwrap();
        bars.name = "gold bar".to_string();
        bars.set_property_int("weight", 1);
        bars.location = Some(room_id.clone());
        let bars_id = bars.id.clone();
        let bars_proto = bars.prototype.clone();

        let mut coins = factory
            .create_stackable_item("coins", owner.clone(), None, 20)
            .await
            .unwrap();
        coins.name = "coins".to_string();
        coins.set_property_int("weight", 1);
        coins.location = Some(room_id.clone());

        let mut greatsword = factory
            .create_item("greatsword", owner.clone())
            .await
            .unwrap();
        greatsword.name = "Greatsword".to_string();
        greatsword.set_property_string("hand_slot", "both");
        greatsword.set_property_int("weight", 8);
        greatsword.location = Some(room_id.clone());

        let mut objects = HashMap::new();
        objects.insert(player.id.clone(), player);
        objects.insert(room_id.clone(), room);
        objects.insert(backpack.id.clone(), backpack);
        objects.insert(bars.id.clone(), bars);
        objects.insert(coins.id.clone(), coins);
        objects.insert(greatsword.id.clone(), greatsword);

        let mut ctx = InventoryContext {
            player_id: &owner,
            room_id: Some(&room_id),
            objects: &mut objects,
            anatomy: &anatomy,
            dirty: None,
        };

        // Build a realistic post-play graph: worn container, partial stacks, two-handed wield.
        take_item(&mut ctx, &format!("1 {}", short_id(&bars_id))).unwrap();
        wear_item(&mut ctx, "backpack").unwrap();
        take_item(&mut ctx, "5 coins").unwrap();
        put_item(&mut ctx, "coins", "backpack", Some(5)).unwrap();
        drop_item(&mut ctx, "gold bar").unwrap();
        take_item(&mut ctx, "greatsword").unwrap();

        let mut split_bars = factory
            .create_stackable_item("gold bar", owner.clone(), bars_proto, 3)
            .await
            .unwrap();
        split_bars.name = "gold bar".to_string();
        split_bars.set_property_int("weight", 1);
        split_bars.location = Some(room_id.clone());
        objects.insert(split_bars.id.clone(), split_bars);

        let before: HashMap<ObjectId, Object> = objects
            .iter()
            .map(|(id, obj)| (id.clone(), obj.clone()))
            .collect();

        assert_graph_references_resolve(&before);

        let player_after = before.get(&owner).unwrap();
        assert_eq!(
            player_after.body_slot_item("torso").as_ref(),
            Some(
                &before
                    .values()
                    .find(|o| o.name == "backpack")
                    .unwrap()
                    .id
            )
        );
        assert!(
            player_after.body_slot_item("left_hand").is_some()
                && player_after.body_slot_item("right_hand").is_some()
        );

        let ground_bars: Vec<_> = before
            .values()
            .filter(|o| o.name == "gold bar" && o.location.as_ref() == Some(&room_id))
            .collect();
        assert_eq!(ground_bars.len(), 2);
        let ground_counts: Vec<u32> = ground_bars.iter().map(|o| o.stack_count()).collect();
        assert!(ground_counts.contains(&3), "split ground pile preserved");
        assert!(ground_counts.contains(&10), "main pile restored after take/drop cycle");
        let total_on_ground: u32 = ground_counts.iter().sum();
        assert_eq!(total_on_ground, 13);

        let greatsword_id = before.values().find(|o| o.name == "Greatsword").unwrap().id.clone();
        assert!(
            before.get(&greatsword_id).unwrap().carried_slot().is_some(),
            "wielded item should record carried_slot"
        );

        let backpack_id = before.values().find(|o| o.name == "backpack").unwrap().id.clone();
        let stored_coins_id = before
            .get(&backpack_id)
            .unwrap()
            .container_contents()[0]
            .clone();
        assert_eq!(before.get(&stored_coins_id).unwrap().stack_count(), 5);

        persist_all(&persistence, &before).await.unwrap();

        let after = hydrate_world(&persistence).await.unwrap();
        assert_graph_references_resolve(&after);
        assert_objects_identical(&before, &after);

        let reloaded_player = after.get(&owner).unwrap();
        assert!(reloaded_player.body_slot_item("torso").is_some());
        assert!(reloaded_player.body_slot_item("left_hand").is_some());
        assert!(reloaded_player.body_slot_item("right_hand").is_some());
        assert!(!reloaded_player
            .body_slots()
            .values()
            .any(|id| !after.contains_key(id)));
    }
}
