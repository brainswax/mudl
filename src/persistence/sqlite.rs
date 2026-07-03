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
}
