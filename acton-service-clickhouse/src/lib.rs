//! Independent clickhouse connection and audit persistence implementation.
pub use acton_service_core::config;
/// Shared errors with adapter-owned driver conversions.
pub mod error {
    pub use acton_service_core::error::{
        DatabaseError, DatabaseErrorKind, DatabaseOperation, Result,
    };
    pub use acton_service_core::StorageError as Error;
}
pub use acton_service_audit as audit;
pub mod clickhouse_backend;
pub use clickhouse_backend::*;
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
