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
    feature = "clickhouse",
    feature = "mssql"
))]
pub(crate) mod lazy;

#[cfg(feature = "database")]
pub mod pg;

#[cfg(feature = "mssql")]
pub mod mssql;

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
    feature = "clickhouse",
    feature = "mssql"
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
    feature = "clickhouse",
    feature = "mssql"
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

/// Verify a requested range fetched together with its immediate predecessor.
/// A stored predecessor anchors local consistency, not externally trusted history.
#[cfg(any(
    test,
    feature = "database",
    feature = "turso",
    feature = "surrealdb",
    feature = "clickhouse",
    feature = "mssql"
))]
fn verify_stored_chain(events: &[AuditEvent], from_sequence: u64) -> Result<Option<u64>, Error> {
    let start = from_sequence.max(1);
    let first_requested = events
        .iter()
        .find(|event| event.sequence >= start)
        .ok_or_else(|| {
            Error::Internal(format!(
                "Audit verification unavailable: empty range starting at sequence {start}"
            ))
        })?;
    if first_requested.sequence != start {
        return Err(Error::Internal(format!(
            "Audit verification incomplete: requested sequence {start} is unavailable"
        )));
    }

    let (previous_sequence, previous_hash) = if start == 1 {
        (0, None)
    } else {
        let predecessor = events.first().filter(|event| event.sequence == start - 1)
            .ok_or_else(|| Error::Internal(format!(
                "Audit verification incomplete: predecessor anchor at sequence {} is unavailable; a trusted retention checkpoint is required",
                start - 1
            )))?;
        // Include the anchor's content hash in verification, but make no claim
        // about its linkage to history preceding the fetched range.
        let previous_hash = if predecessor.sequence == 1 {
            None
        } else {
            predecessor.previous_hash.as_deref()
        };
        (predecessor.sequence - 1, previous_hash)
    };

    Ok(
        super::chain::verify_chain_with_anchor(events, previous_sequence, previous_hash)
            .err()
            .map(|error| error.sequence),
    )
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
    /// The range starts at `from_sequence` (0 is an alias for genesis, 1).
    /// Built-in adapters fetch the immediate predecessor in the same ordered
    /// query and check its content hash, then every sequence, hash, and link in
    /// the requested range. `Ok(None)` means local consistency against that
    /// stored anchor; it does not prove completeness against an independently
    /// trusted chain head or integrity of history before the anchor.
    ///
    /// `Ok(Some(sequence))` identifies the first broken event, including a
    /// corrupt predecessor. An empty range, missing requested start, or missing
    /// predecessor returns `Err`, never an affirmative integrity result. After
    /// prefix retention, verification at the retained boundary is incomplete
    /// without a trusted checkpoint; these adapters do not store checkpoints.
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

#[cfg(test)]
mod verification_tests {
    use super::verify_stored_chain;
    use crate::audit::{AuditChain, AuditEvent, AuditEventKind, AuditSeverity};

    fn events() -> Vec<AuditEvent> {
        let mut chain = AuditChain::new("verification-test".into());
        (0..4)
            .map(|_| {
                chain.seal(AuditEvent::new(
                    AuditEventKind::HttpRequest,
                    AuditSeverity::Informational,
                    "verification-test".into(),
                ))
            })
            .collect()
    }

    #[test]
    fn full_chain_and_each_anchored_suffix_are_valid() {
        let events = events();
        assert_eq!(verify_stored_chain(&events, 0).unwrap(), None);
        assert_eq!(verify_stored_chain(&events, 1).unwrap(), None);
        for start in 2..=4 {
            assert_eq!(
                verify_stored_chain(&events[start as usize - 2..], start).unwrap(),
                None
            );
        }
    }

    #[test]
    fn empty_ranges_and_predecessor_only_are_unavailable() {
        assert!(verify_stored_chain(&[], 0)
            .unwrap_err()
            .to_string()
            .contains("empty range"));
        let events = events();
        assert!(verify_stored_chain(&events[3..], 5)
            .unwrap_err()
            .to_string()
            .contains("empty range"));
    }

    #[test]
    fn purged_prefix_requires_an_anchor() {
        let events = events();
        let retained = &events[2..];
        assert!(verify_stored_chain(retained, 3)
            .unwrap_err()
            .to_string()
            .contains("predecessor anchor"));
        assert!(verify_stored_chain(retained, 1)
            .unwrap_err()
            .to_string()
            .contains("requested sequence 1"));
        assert_eq!(verify_stored_chain(retained, 4).unwrap(), None);
    }

    #[test]
    fn modified_payload_hash_and_links_report_the_first_broken_event() {
        for index in 0..4 {
            let mut payload = events();
            payload[index].path = Some("/tampered".into());
            assert_eq!(
                verify_stored_chain(&payload, 2).unwrap(),
                Some(index as u64 + 1)
            );
            let mut hash = events();
            hash[index].hash = Some("tampered".into());
            assert_eq!(
                verify_stored_chain(&hash, 2).unwrap(),
                Some(index as u64 + 1)
            );
        }
        let mut events = events();
        events[1].previous_hash = Some("wrong-anchor".into());
        assert_eq!(verify_stored_chain(&events, 2).unwrap(), Some(2));
    }

    #[test]
    fn genesis_anchor_cannot_claim_a_predecessor() {
        let mut chain = AuditChain::resume("verification-test".into(), "forged".into(), 0);
        let forged: Vec<_> = events()
            .into_iter()
            .take(2)
            .map(|event| chain.seal(event))
            .collect();
        assert_eq!(verify_stored_chain(&forged, 2).unwrap(), Some(1));
    }

    #[test]
    fn replaced_anchor_and_resealed_gap_are_detected() {
        let mut events = events();
        let mut unrelated = AuditChain::new("another-service".into());
        events[0] = unrelated.seal(events[0].clone());
        assert_eq!(verify_stored_chain(&events, 2).unwrap(), Some(2));

        let mut events = self::events();
        let mut gap = AuditChain::resume(
            "verification-test".into(),
            events[1].hash.clone().unwrap(),
            3,
        );
        events[2] = gap.seal(events[2].clone());
        assert_eq!(verify_stored_chain(&events[..3], 2).unwrap(), Some(4));
    }
}
