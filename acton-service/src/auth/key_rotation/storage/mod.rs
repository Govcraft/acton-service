//! Key rotation storage trait and backend implementations
//!
//! The [`KeyRotationStorage`] trait defines the interface for persisting signing key
//! metadata across the supported database backends. Unlike audit storage, signing keys
//! require mutable status (Active -> Draining -> Retired) but prevent deletion to
//! maintain an audit trail.
//!
//! # Available Backends
//!
//! - **PostgreSQL** (`database` feature): Uses `sqlx::PgPool`
//! - **Turso** (`turso` feature): Uses `Arc<libsql::Database>`
//! - **SurrealDB** (`surrealdb` feature): Uses `Arc<SurrealClient>` with optimistic concurrency

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use super::key_metadata::{KeyStatus, SigningKeyMetadata};
use crate::error::Error;

#[cfg(feature = "database")]
pub mod pg;

#[cfg(feature = "turso")]
pub mod turso;

#[cfg(feature = "surrealdb")]
pub mod surrealdb_impl;

/// Trait for signing key metadata persistence backends
///
/// Implementations store key metadata and support the lifecycle transitions
/// required by automated key rotation. Keys are never deleted -- retired keys
/// remain for audit purposes.
#[async_trait]
pub trait KeyRotationStorage: Send + Sync {
    /// Store a new signing key
    ///
    /// Inserts a new key record. Fails if a key with the same `kid` already exists.
    async fn store_key(&self, key: &SigningKeyMetadata) -> Result<(), Error>;

    /// Get the currently active signing key for a service
    ///
    /// Returns `None` if no active key exists (e.g., first startup before bootstrapping).
    async fn get_active_key(&self, service_name: &str)
        -> Result<Option<SigningKeyMetadata>, Error>;

    /// Look up a key by its `kid` (key identifier)
    ///
    /// Used for JWT header-based `kid` resolution.
    async fn get_key_by_kid(&self, kid: &str) -> Result<Option<SigningKeyMetadata>, Error>;

    /// Get all keys that can verify tokens (Active + Draining)
    ///
    /// Returns keys ordered by `created_at` descending (newest first).
    async fn get_verification_keys(
        &self,
        service_name: &str,
    ) -> Result<Vec<SigningKeyMetadata>, Error>;

    /// Transition a key to a new status
    ///
    /// The `timestamp` is applied to the appropriate timestamp field based on
    /// `new_status`:
    /// - `Draining`: sets `draining_since`
    /// - `Retired`: sets `retired_at`
    /// - `Active`: sets `activated_at`
    ///
    /// Returns an error if the key does not exist or the status transition
    /// was not applied (e.g., optimistic concurrency conflict).
    async fn update_key_status(
        &self,
        kid: &str,
        new_status: KeyStatus,
        timestamp: DateTime<Utc>,
    ) -> Result<(), Error>;

    /// Retire all draining keys whose drain window has expired
    ///
    /// Sets status to `Retired` and `retired_at = now` for all keys where
    /// `status = 'draining'` and `drain_expires_at <= now`.
    ///
    /// Returns the number of keys retired.
    async fn retire_expired_draining_keys(&self, now: DateTime<Utc>) -> Result<u64, Error>;

    /// Create the signing_keys table and indexes
    ///
    /// Should be called once during application startup. Implementations
    /// use `IF NOT EXISTS` semantics so this is safe to call repeatedly.
    async fn initialize(&self) -> Result<(), Error>;
}
