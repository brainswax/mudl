pub mod error;
pub mod metadata;
pub mod optimistic;
pub mod sqlite;
pub mod r#trait;

pub use error::PersistenceError;
pub use metadata::{save_and_sync, SaveMetadata};
pub use optimistic::{
    refresh_revision_from_db, save_object_with_retry, save_objects_batch_with_retry,
    DEFAULT_SAVE_RETRIES,
};
pub use r#trait::Persistence;
pub use sqlite::SqlitePersistence;