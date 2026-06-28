use anyhow::Result;
use async_trait::async_trait;
use sqlx::SqlitePool;

use std::path::Path;
use std::str::FromStr;

use sqlx::sqlite::SqliteConnectOptions;

use super::super::object::{Object, ObjectId};
use super::r#trait::Persistence;

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
                data TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS counters (
                type_base TEXT PRIMARY KEY,
                counter INTEGER NOT NULL DEFAULT 0
            );
            "#,
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }
}

#[async_trait]
impl Persistence for SqlitePersistence {
    async fn save_object(&self, object: &Object) -> Result<()> {
        let id = object.id.to_string();
        let data = serde_json::to_string(object)?;

        sqlx::query("INSERT OR REPLACE INTO objects (id, data) VALUES (?, ?)")
            .bind(id)
            .bind(data)
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
}

impl SqlitePersistence {
    /// Load every persisted object (used to build display context for look/examine).
    pub async fn list_objects(&self) -> Result<Vec<Object>> {
        let rows: Vec<String> =
            sqlx::query_scalar::<_, String>("SELECT data FROM objects ORDER BY id")
                .fetch_all(&self.pool)
                .await?;

        rows.into_iter()
            .map(|data| serde_json::from_str(&data).map_err(Into::into))
            .collect()
    }
}
