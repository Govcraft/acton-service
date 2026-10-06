//! Independent surrealdb connection and audit persistence implementation.
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
pub fn database_error(err: surrealdb::Error) -> DatabaseError {
    let msg = err.to_string();

    let (kind, operation) = if msg.contains("already exists")
        || msg.contains("unique")
        || msg.contains("duplicate")
    {
        (
            DatabaseErrorKind::ConstraintViolation,
            DatabaseOperation::Insert,
        )
    } else if msg.contains("not found") || msg.contains("no record") {
        (DatabaseErrorKind::NotFound, DatabaseOperation::Query)
    } else if msg.contains("timeout") || msg.contains("timed out") {
        (DatabaseErrorKind::Timeout, DatabaseOperation::Query)
    } else if msg.contains("connect") || msg.contains("Connection") {
        (
            DatabaseErrorKind::ConnectionFailed,
            DatabaseOperation::Connect,
        )
    } else if msg.contains("permission") || msg.contains("not allowed") || msg.contains("denied") {
        (
            DatabaseErrorKind::PermissionDenied,
            DatabaseOperation::Query,
        )
    } else if msg.contains("auth") || msg.contains("signin") || msg.contains("credentials") {
        (
            DatabaseErrorKind::ConnectionFailed,
            DatabaseOperation::Connect,
        )
    } else if msg.contains("parse") || msg.contains("syntax") {
        (DatabaseErrorKind::QueryFailed, DatabaseOperation::Query)
    } else {
        (DatabaseErrorKind::Other, DatabaseOperation::Query)
    };

    DatabaseError::new(operation, kind, msg)
}
pub use acton_service_audit as audit;
pub mod surrealdb_backend;
pub use surrealdb_backend::*;
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
