pub mod error;
pub mod metadata;
pub mod optimistic;
pub mod sqlite;
pub mod r#trait;
pub mod writer_lock;

pub use error::PersistenceError;
pub use metadata::{save_and_sync, SaveMetadata};
pub use optimistic::{
    refresh_revision_from_db, save_object_with_retry, save_objects_batch_with_retry,
    DEFAULT_SAVE_RETRIES,
};
pub use r#trait::Persistence;
pub use sqlite::SqlitePersistence;
pub use writer_lock::{
    is_memory_database, lock_path_for_database_url, WriterGuard, WriterLockError,
    WriterLockOptions, WriterLockRecord, WriterMode,
};