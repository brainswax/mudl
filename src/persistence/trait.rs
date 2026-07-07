use anyhow::Result;
use async_trait::async_trait;

use crate::object::{Object, ObjectId};

use super::metadata::SaveMetadata;

#[async_trait]
pub trait Persistence: Send + Sync {
    /// Optimistic save: succeeds only when `object.revision` matches the database row.
    async fn save_object(&self, object: &Object) -> Result<SaveMetadata>;

    /// Atomically persist multiple objects (SQLite transaction when supported).
    async fn save_objects_batch(&self, objects: &[&Object]) -> Result<Vec<SaveMetadata>> {
        let mut metas = Vec::with_capacity(objects.len());
        for object in objects {
            metas.push(self.save_object(object).await?);
        }
        Ok(metas)
    }

    async fn load_object(&self, id: &ObjectId) -> Result<Option<Object>>;
    async fn get_next_id_counter(&self, obj_type: &str, base_name: &str) -> Result<u32>;
    async fn increment_counter(&self, obj_type: &str, base_name: &str) -> Result<u32>;

    /// List persisted objects. Soft-deleted objects are excluded unless `include_deleted` is true.
    async fn list_objects(&self, include_deleted: bool) -> Result<Vec<Object>> {
        let _ = include_deleted;
        Ok(Vec::new())
    }
}