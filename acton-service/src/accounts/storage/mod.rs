//! Account storage trait and backend implementations
//!
//! The `AccountStorage` trait defines the interface for persisting accounts.
//!
//! # Available Backends
//!
//! - **PostgreSQL** (`database` feature): Full SQL with JSONB roles
//! - **Turso** (`turso` feature): SQLite-based with TEXT/INTEGER types
//! - **SurrealDB** (`surrealdb` feature): SurrealQL with SCHEMAFULL table

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use super::types::{Account, AccountStatus};
use crate::error::Error;

#[cfg(feature = "database")]
pub mod pg;

#[cfg(feature = "turso")]
pub mod turso;

#[cfg(feature = "surrealdb")]
pub mod surrealdb_impl;

/// Trait for account persistence backends
#[async_trait]
pub trait AccountStorage: Send + Sync {
    /// Create a new account
    async fn create(&self, account: &Account) -> Result<(), Error>;

    /// Get an account by ID
    async fn get_by_id(&self, id: &str) -> Result<Option<Account>, Error>;

    /// Get an account by email
    async fn get_by_email(&self, email: &str) -> Result<Option<Account>, Error>;

    /// Get an account by username
    async fn get_by_username(&self, username: &str) -> Result<Option<Account>, Error>;

    /// Update a full account record
    async fn update(&self, account: &Account) -> Result<(), Error>;

    /// Update only the status (and related timestamp/reason fields)
    async fn update_status(
        &self,
        id: &str,
        status: AccountStatus,
        reason: Option<&str>,
    ) -> Result<(), Error>;

    /// List accounts with optional status filter and pagination
    async fn list(
        &self,
        status_filter: Option<AccountStatus>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Account>, Error>;

    /// Count accounts with optional status filter
    async fn count(&self, status_filter: Option<AccountStatus>) -> Result<u64, Error>;

    /// Hard delete an account (GDPR)
    async fn delete(&self, id: &str) -> Result<bool, Error>;

    /// Record a successful login (updates last_login_at, resets failed_login_count)
    async fn record_login(&self, id: &str) -> Result<(), Error>;

    /// Find accounts past their expiration date
    async fn find_expired(&self, limit: usize) -> Result<Vec<Account>, Error>;

    /// Find accounts inactive since before the cutoff
    async fn find_inactive(
        &self,
        cutoff: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<Account>, Error>;
}
