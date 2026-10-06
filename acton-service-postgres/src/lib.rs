//! Independent postgres connection and audit persistence implementation.
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
pub fn database_error(err: sqlx::Error) -> DatabaseError {
    use sqlx::Error as E;
    match err {
        E::RowNotFound => DatabaseError::not_found(DatabaseOperation::Query, "Row not found"),
        E::PoolTimedOut => DatabaseError::pool_exhausted("Connection pool timed out"),
        E::PoolClosed => DatabaseError::connection_failed("Connection pool is closed"),
        E::Protocol(msg) => DatabaseError::new(
            DatabaseOperation::Query,
            DatabaseErrorKind::QueryFailed,
            msg,
        ),
        E::Configuration(e) => DatabaseError::new(
            DatabaseOperation::Connect,
            DatabaseErrorKind::Configuration,
            e.to_string(),
        ),
        E::Io(e) => DatabaseError::new(
            DatabaseOperation::Connect,
            DatabaseErrorKind::ConnectionFailed,
            e.to_string(),
        ),
        E::Tls(e) => DatabaseError::new(
            DatabaseOperation::Connect,
            DatabaseErrorKind::ConnectionFailed,
            format!("TLS error: {}", e),
        ),
        E::TypeNotFound { type_name } => DatabaseError::new(
            DatabaseOperation::Query,
            DatabaseErrorKind::TypeConversion,
            format!("Type not found: {}", type_name),
        ),
        E::ColumnNotFound(col) => DatabaseError::new(
            DatabaseOperation::Query,
            DatabaseErrorKind::QueryFailed,
            format!("Column not found: {}", col),
        ),
        E::ColumnIndexOutOfBounds { index, len } => DatabaseError::new(
            DatabaseOperation::Query,
            DatabaseErrorKind::QueryFailed,
            format!("Column index {} out of bounds (len: {})", index, len),
        ),
        E::ColumnDecode { index, source } => DatabaseError::new(
            DatabaseOperation::Query,
            DatabaseErrorKind::TypeConversion,
            format!("Failed to decode column {}: {}", index, source),
        ),
        E::Decode(e) => DatabaseError::new(
            DatabaseOperation::Query,
            DatabaseErrorKind::TypeConversion,
            e.to_string(),
        ),
        E::AnyDriverError(e) => DatabaseError::new(
            DatabaseOperation::Query,
            DatabaseErrorKind::QueryFailed,
            e.to_string(),
        ),
        E::Migrate(e) => DatabaseError::new(
            DatabaseOperation::Migration,
            DatabaseErrorKind::QueryFailed,
            e.to_string(),
        ),
        E::Database(db_err) => {
            // Parse database-specific errors - combine all constraint violations
            let kind = if db_err.is_unique_violation()
                || db_err.is_foreign_key_violation()
                || db_err.is_check_violation()
            {
                DatabaseErrorKind::ConstraintViolation
            } else {
                DatabaseErrorKind::QueryFailed
            };
            DatabaseError::new(DatabaseOperation::Query, kind, db_err.to_string())
        }
        E::WorkerCrashed => DatabaseError::connection_failed("Database worker crashed"),
        _ => DatabaseError::new(
            DatabaseOperation::Query,
            DatabaseErrorKind::Other,
            err.to_string(),
        ),
    }
}
pub use acton_service_audit as audit;
pub mod database;
pub use database::*;
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
