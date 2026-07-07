use crate::object::ObjectId;

/// Persistence-layer errors (optimistic locking, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersistenceError {
    /// Another writer committed before this save (`revision` mismatch).
    RevisionConflict {
        id: ObjectId,
        expected: u64,
        actual: u64,
    },
}

impl PersistenceError {
    pub fn is_revision_conflict(&self) -> bool {
        matches!(self, Self::RevisionConflict { .. })
    }
}

impl std::fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RevisionConflict {
                id,
                expected,
                actual,
            } => write!(
                f,
                "revision conflict for {id}: expected {expected}, database has {actual}"
            ),
        }
    }
}

impl std::error::Error for PersistenceError {}