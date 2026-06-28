use async_trait::async_trait;
use sqlx::SqlitePool;
use anyhow::Result;

use super::super::object::{Object, ObjectId};
use super::r#trait::Persistence;

pub struct SqlitePersistence {
    pool: SqlitePool,
}

impl SqlitePersistence {
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = SqlitePool::connect(database_url).await?;

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

        sqlx::query(
            "INSERT OR REPLACE INTO objects (id, data) VALUES (?, ?)",
        )
        .bind(id)
        .bind(data)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn load_object(&self, id: &ObjectId) -> Result<Option<Object>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT data FROM objects WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        if let Some((data,)) = row {
            let object: Object = serde_json::from_str(&data)?;
            Ok(Some(object))
        } else {
            Ok(None)
        }
    }

    async fn get_next_id_counter(&self, obj_type: &str, base_name: &str) -> Result<u32> {
        let key = format!("{}:{}", obj_type, base_name);
        let counter: Option<i64> = sqlx::query_scalar(
            "SELECT counter FROM counters WHERE type_base = ?",
        )
        .bind(&key)
        .fetch_optional(&self.pool)
        .await?;

        Ok(counter.map(|c| (c + 1) as u32).unwrap_or(1))
    }

    async fn increment_counter(&self, obj_type: &str, base_name: &str) -> Result<u32> {
        let key = format!("{}:{}", obj_type, base_name);
        let new_counter: i64 = sqlx::query_scalar(
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
