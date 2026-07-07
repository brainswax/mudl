use crate::object::Object;

/// Row metadata returned after a successful optimistic save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveMetadata {
    pub revision: u64,
    pub updated_at: String,
}

impl SaveMetadata {
    pub fn apply_to(&self, object: &mut Object) {
        object.revision = self.revision;
        object.updated_at = Some(self.updated_at.clone());
    }
}

/// Optimistic save that keeps the in-memory revision in sync with SQLite.
pub async fn save_and_sync<P: super::Persistence>(
    persistence: &P,
    object: &mut Object,
) -> anyhow::Result<SaveMetadata> {
    let meta = persistence.save_object(object).await?;
    meta.apply_to(object);
    Ok(meta)
}