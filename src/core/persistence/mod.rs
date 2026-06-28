pub mod sqlite;
pub mod r#trait;

pub use r#trait::Persistence;
pub use sqlite::SqlitePersistence;
