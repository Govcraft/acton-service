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
//! - **ClickHouse** (`clickhouse` feature): Naturally append-only (MergeTree engine)

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use super::event::AuditEvent;
use crate::error::Error;

#[cfg(any(
    feature = "database",
    feature = "turso",
    feature = "surrealdb",
    feature = "clickhouse"
))]
pub(crate) mod lazy;

#[cfg(feature = "database")]
pub mod pg;

#[cfg(feature = "turso")]
pub mod turso;

#[cfg(feature = "surrealdb")]
pub mod surrealdb_impl;

#[cfg(feature = "clickhouse")]
pub mod clickhouse_impl;

/// Returns true if the stored event-kind string looks like a framework-owned
/// kind (`auth.*`, `http.*`, `account.*`, `config.*`) that should have been
/// recognized by the parser. Used by parser catch-alls to detect likely
/// version skew between an emitter and a reader.
///
/// Only compiled alongside a storage backend — nothing parses stored rows without one.
#[cfg(any(
    feature = "database",
    feature = "turso",
    feature = "surrealdb",
    feature = "clickhouse"
))]
pub(crate) fn looks_like_framework_kind(s: &str) -> bool {
    s.starts_with("auth.")
        || s.starts_with("http.")
        || s.starts_with("account.")
        || s.starts_with("config.")
}

/// Helper for storage-backend parser catch-alls.
///
/// Strips the `custom.` prefix when present (so user-defined custom events
/// round-trip cleanly) and emits a `tracing::warn!` when the input looks
/// like a framework-owned kind that no parser arm matched — i.e. the
/// emitter is on a newer version than this reader.
///
/// Only compiled alongside a storage backend — nothing parses stored rows without one.
#[cfg(any(
    feature = "database",
    feature = "turso",
    feature = "surrealdb",
    feature = "clickhouse"
))]
pub(crate) fn parse_custom_kind(s: &str) -> String {
    if looks_like_framework_kind(s) {
        tracing::warn!(
            stored_kind = %s,
            "unrecognized framework audit event kind — falling back to Custom; likely version skew between emitter and reader"
        );
    }
    s.strip_prefix("custom.").unwrap_or(s).to_string()
}

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

    /// Query events with timestamps before the given cutoff
    ///
    /// Returns up to `limit` events ordered by sequence ASC.
    /// Used by retention cleanup to fetch events for archival before purge.
    async fn query_before(
        &self,
        _cutoff: DateTime<Utc>,
        _limit: usize,
    ) -> Result<Vec<AuditEvent>, Error> {
        Err(Error::Internal("query_before not implemented".into()))
    }

    /// Purge events with timestamps before the given cutoff
    ///
    /// Temporarily disables immutability protections, performs the delete,
    /// then reinstates protections. Returns the number of rows deleted.
    async fn purge_before(&self, _cutoff: DateTime<Utc>) -> Result<u64, Error> {
        Err(Error::Internal("purge_before not implemented".into()))
    }

    /// Confirm the backend is usable, performing any deferred setup.
    ///
    /// Backends built from an already-connected client are ready immediately.
    /// Lazily-resolved backends (those built from a pool that connects in the
    /// background) return an error
    /// until their connection pool finishes connecting; the audit agent polls
    /// this before initializing the hash chain so it resumes from the persisted
    /// sequence instead of restarting at zero.
    async fn ensure_ready(&self) -> Result<(), Error> {
        Ok(())
    }
}

#[cfg(all(
    test,
    any(
        feature = "database",
        feature = "turso",
        feature = "surrealdb",
        feature = "clickhouse"
    )
))]
mod helper_tests {
    use super::{looks_like_framework_kind, parse_custom_kind};

    #[test]
    fn framework_prefixes_detected() {
        assert!(looks_like_framework_kind("auth.token.invalid"));
        assert!(looks_like_framework_kind("http.request.denied"));
        assert!(looks_like_framework_kind("account.created"));
        assert!(looks_like_framework_kind("config.drift_detected"));
    }

    #[test]
    fn non_framework_prefixes_ignored() {
        assert!(!looks_like_framework_kind("custom.user.exported"));
        assert!(!looks_like_framework_kind("user.signed_up"));
        assert!(!looks_like_framework_kind("billing.invoice.paid"));
        assert!(!looks_like_framework_kind(""));
    }

    #[test]
    fn parse_custom_strips_custom_prefix() {
        assert_eq!(parse_custom_kind("custom.user.exported"), "user.exported");
    }

    #[test]
    fn parse_custom_preserves_unprefixed_user_strings() {
        assert_eq!(
            parse_custom_kind("billing.invoice.paid"),
            "billing.invoice.paid"
        );
    }

    #[test]
    fn parse_custom_passes_through_framework_strings_for_visibility() {
        // The warn fires (verified manually / via tracing subscribers in
        // integration tests); we assert the returned string preserves the
        // original so operators can grep for it in their event store.
        assert_eq!(
            parse_custom_kind("auth.token.invalid"),
            "auth.token.invalid"
        );
    }
}
