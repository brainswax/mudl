pub mod r#trait;
pub mod sqlite;

pub use r#trait::Persistence;
pub use sqlite::SqlitePersistence;
