//! Driver-independent configuration and storage errors.
pub mod config;
pub mod error;
pub use error::{DatabaseError, DatabaseErrorKind, DatabaseOperation, StorageError};
