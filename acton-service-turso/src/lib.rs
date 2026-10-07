//! Independent turso connection and audit persistence implementation.
pub use acton_service_core::config;
/// Shared errors with adapter-owned driver conversions.
pub mod error {
    pub use acton_service_core::error::{
        DatabaseError, DatabaseErrorKind, DatabaseOperation, Result,
    };
    pub use acton_service_core::StorageError as Error;
}
pub use error::{DatabaseError, DatabaseErrorKind, DatabaseOperation};
/// Classify a driver failure without depending on the service facade.
pub fn database_error(err: libsql::Error) -> DatabaseError {
    let msg = err.to_string();

    // Parse libsql error messages to determine kind and operation
    // Combine all constraint violations into a single branch
    let (kind, operation) = if msg.contains("UNIQUE constraint failed")
        || msg.contains("FOREIGN KEY constraint failed")
        || msg.contains("NOT NULL constraint failed")
        || msg.contains("CHECK constraint failed")
    {
        (
            DatabaseErrorKind::ConstraintViolation,
            DatabaseOperation::Insert,
        )
    } else if msg.contains("no such table") || msg.contains("no such column") {
        (DatabaseErrorKind::QueryFailed, DatabaseOperation::Query)
    } else if msg.contains("timeout") || msg.contains("timed out") {
        (DatabaseErrorKind::Timeout, DatabaseOperation::Query)
    } else if msg.contains("connection") || msg.contains("Connection") {
        (
            DatabaseErrorKind::ConnectionFailed,
            DatabaseOperation::Connect,
        )
    } else if msg.contains("permission denied") || msg.contains("Permission denied") {
        (
            DatabaseErrorKind::PermissionDenied,
            DatabaseOperation::Query,
        )
    } else if msg.contains("sync") || msg.contains("Sync") {
        (DatabaseErrorKind::SyncFailed, DatabaseOperation::Sync)
    } else {
        (DatabaseErrorKind::Other, DatabaseOperation::Query)
    };

    DatabaseError::new(operation, kind, msg)
}
pub use acton_service_audit as audit;
pub mod turso;
pub use turso::*;
/// Append-only audit persistence implemented by this driver.
pub mod storage {
    pub use acton_service_audit::storage::*;
    /// Deferred backend schema initialization contracts.
    pub mod lazy {
        pub use acton_service_audit::storage::lazy::*;
    }
    mod backend;
    pub use backend::*;
}
