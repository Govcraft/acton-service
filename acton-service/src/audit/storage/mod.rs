//! Audit event storage trait and backend implementations
//!
//! The `AuditStorage` trait defines the interface for persisting audit events.
//! Backend implementations enforce immutability (no update/delete) at the database level.
//!
//! # Available Backends
//!
//! - **PostgreSQL** (`database` feature): Uses `CREATE RULE` to prevent UPDATE/DELETE
//! - **Turso** (`turso` feature): Uses triggers to prevent UPDATE/DELETE
//! - **SurrealDB** (`surrealdb` feature): Uses `PERMISSIONS FOR update, delete NONE`

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use super::event::AuditEvent;
use crate::error::Error;

#[cfg(feature = "database")]
pub mod pg;

#[cfg(feature = "turso")]
pub mod turso;

#[cfg(feature = "surrealdb")]
pub mod surrealdb_impl;

/// Trait for audit event persistence backends
///
/// Implementations MUST enforce append-only semantics at the database level
/// (not just at the application level) to prevent tampering.
#[async_trait]
pub trait AuditStorage: Send + Sync {
    /// Append a sealed event to storage
    ///
    /// The event must have `hash`, `previous_hash`, and `sequence` already set
    /// by `AuditChain::seal()`.
    async fn append(&self, event: &AuditEvent) -> Result<(), Error>;

    /// Get the most recent event (for chain resumption on startup)
    async fn latest(&self) -> Result<Option<AuditEvent>, Error>;

    /// Query events within a time range
    async fn query_range(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<AuditEvent>, Error>;

    /// Verify chain integrity from a given sequence number
    ///
    /// Returns `Ok(None)` if the chain is intact, or `Ok(Some(sequence))` with
    /// the first broken sequence number.
    async fn verify_chain(&self, from_sequence: u64) -> Result<Option<u64>, Error>;
}
