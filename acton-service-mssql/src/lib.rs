//! Independent mssql connection and audit persistence implementation.
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
pub fn database_error(err: tiberius::error::Error) -> DatabaseError {
    DatabaseError::new(
        DatabaseOperation::Query,
        DatabaseErrorKind::QueryFailed,
        err.to_string(),
    )
}
pub use acton_service_audit as audit;
pub mod mssql;
pub use mssql::*;
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
