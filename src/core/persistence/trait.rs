use anyhow::Result;
use async_trait::async_trait;

use super::super::object::{Object, ObjectId};

#[async_trait]
pub trait Persistence: Send + Sync {
    async fn save_object(&self, object: &Object) -> Result<()>;
    async fn load_object(&self, id: &ObjectId) -> Result<Option<Object>>;
    async fn get_next_id_counter(&self, obj_type: &str, base_name: &str) -> Result<u32>;
    async fn increment_counter(&self, obj_type: &str, base_name: &str) -> Result<u32>;
}
