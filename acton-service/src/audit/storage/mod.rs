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

use super::event::{AuditEvent, AuditEventKind, AuditSeverity};
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

/// Result of bounded verification against the locally stored predecessor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditVerification {
    /// All requested events and their predecessor are locally consistent.
    Consistent,
    /// The first event whose content, sequence, or link is broken.
    Broken { sequence: u64 },
    /// A requested endpoint or predecessor is unavailable.
    Incomplete,
}

/// Stable audit sequence ordering, independent of clock adjustments.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AuditOrder {
    /// Highest sequence first.
    #[default]
    NewestFirst,
    /// Lowest sequence first.
    OldestFirst,
}

/// Bounded investigation filters. All specified filters must match.
#[derive(Debug, Clone)]
pub struct AuditQuery {
    /// Inclusive earliest timestamp.
    pub from: Option<DateTime<Utc>>,
    /// Inclusive latest timestamp.
    pub to: Option<DateTime<Utc>>,
    /// Exact event kind.
    pub kind: Option<AuditEventKind>,
    /// Exact severity.
    pub severity: Option<AuditSeverity>,
    /// Exact source subject.
    pub subject: Option<String>,
    /// Exact source subject or metadata user or actor.
    pub actor: Option<String>,
    /// Exact source request identifier.
    pub request_id: Option<String>,
    /// Exact service name.
    pub service_name: Option<String>,
    /// Optional exact wire kinds whose metadata may participate in filters.
    /// Source subject matching remains available for every kind.
    pub metadata_kinds: Option<Vec<String>>,
    /// Exact metadata schema.
    pub schema: Option<String>,
    /// Exact metadata entity_id.
    pub entity_id: Option<String>,
    /// Exact metadata tenant_id.
    pub tenant_id: Option<String>,
    /// Exact HTTP status.
    pub status_code: Option<u16>,
    /// Exclusive sequence cursor in the requested direction.
    pub cursor: Option<u64>,
    /// Inclusive snapshot sequence ceiling.
    pub through_sequence: Option<u64>,
    /// Sequence ordering.
    pub order: AuditOrder,
    /// Maximum returned events, from 1 through 1000.
    pub limit: usize,
}
impl Default for AuditQuery {
    fn default() -> Self {
        Self {
            from: None,
            to: None,
            kind: None,
            severity: None,
            subject: None,
            actor: None,
            request_id: None,
            service_name: None,
            metadata_kinds: None,
            schema: None,
            entity_id: None,
            tenant_id: None,
            status_code: None,
            cursor: None,
            through_sequence: None,
            order: AuditOrder::NewestFirst,
            limit: 100,
        }
    }
}
impl AuditQuery {
    /// Reject invalid bounds before touching storage.
    pub fn validate(&self) -> Result<(), Error> {
        if !(1..=1000).contains(&self.limit)
            || self.from.zip(self.to).is_some_and(|(a, b)| a > b)
            || self.cursor.is_some_and(|v| v > i64::MAX as u64)
            || self.through_sequence.is_some_and(|v| v > i64::MAX as u64)
        {
            return Err(Error::Internal(
                "Invalid audit query bounds (limit must be 1..=1000)".into(),
            ));
        }
        Ok(())
    }

    /// Test the exact filters against a decoded event without changing its contents.
    pub fn matches(&self, e: &AuditEvent) -> bool {
        let metadata_allowed = self
            .metadata_kinds
            .as_ref()
            .is_none_or(|kinds| kinds.contains(&e.kind.to_string()));
        let meta = |key: &str| {
            if !metadata_allowed {
                return None;
            }
            e.metadata
                .as_ref()
                .and_then(|m| m.get(key))
                .and_then(serde_json::Value::as_str)
        };
        self.from.is_none_or(|v| e.timestamp >= v)
            && self.to.is_none_or(|v| e.timestamp <= v)
            && self.kind.as_ref().is_none_or(|v| e.kind == *v)
            && self.severity.is_none_or(|v| e.severity == v)
            && self
                .subject
                .as_deref()
                .is_none_or(|v| e.source.subject.as_deref() == Some(v))
            && self.actor.as_deref().is_none_or(|v| {
                e.source.subject.as_deref() == Some(v)
                    || meta("user") == Some(v)
                    || meta("actor") == Some(v)
            })
            && self
                .request_id
                .as_deref()
                .is_none_or(|v| e.source.request_id.as_deref() == Some(v))
            && self
                .service_name
                .as_ref()
                .is_none_or(|v| e.service_name == *v)
            && self
                .schema
                .as_deref()
                .is_none_or(|v| meta("schema") == Some(v))
            && self
                .entity_id
                .as_deref()
                .is_none_or(|v| meta("entity_id") == Some(v))
            && self
                .tenant_id
                .as_deref()
                .is_none_or(|v| meta("tenant_id") == Some(v))
            && self.status_code.is_none_or(|v| e.status_code == Some(v))
            && self.through_sequence.is_none_or(|v| e.sequence <= v)
            && self.cursor.is_none_or(|v| match self.order {
                AuditOrder::NewestFirst => e.sequence < v,
                AuditOrder::OldestFirst => e.sequence > v,
            })
    }
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

    /// Return a bounded, filtered page in stable sequence order.
    /// Custom adapters explicitly report unsupported; no unbounded fallback occurs.
    /// SurrealDB caps examination at 10000 candidates and errors if exhausted.
    async fn query_filtered(&self, _query: &AuditQuery) -> Result<Vec<AuditEvent>, Error> {
        Err(Error::Internal(
            "Filtered audit queries are unsupported by this storage".into(),
        ))
    }

    /// Get the most recent event (for chain resumption on startup)
    async fn latest(&self) -> Result<Option<AuditEvent>, Error>;

    /// Query events within a time range
    async fn query_range(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<AuditEvent>, Error>;

    /// Query an inclusive sequence interval in ascending order, capped by `limit`.
    /// Unsupported custom adapters return an error instead of silently scanning.
    async fn query_sequence(
        &self,
        _from: u64,
        _to: u64,
        _limit: usize,
    ) -> Result<Vec<AuditEvent>, Error> {
        Err(Error::Internal(
            "Sequence audit queries are unsupported by this storage".into(),
        ))
    }

    /// Return retained first and last sequence numbers, or `None` for empty storage.
    /// Concurrent retention may move the lower bound after this observation.
    async fn sequence_bounds(&self) -> Result<Option<(u64, u64)>, Error> {
        let Some(last) = self.latest().await? else {
            return Ok(None);
        };
        Ok(self
            .query_sequence(1, last.sequence, 1)
            .await?
            .first()
            .map(|first| (first.sequence, last.sequence)))
    }

    /// Verify an inclusive interval using at most 10,001 stored events.
    /// Zero starts at genesis. The immediate predecessor is checked as a local
    /// anchor; consistency does not authenticate earlier history or completeness
    /// against an external head. Missing endpoints/anchor return `Incomplete`.
    async fn verify_chain_range(&self, from: u64, to: u64) -> Result<AuditVerification, Error> {
        let from = from.max(1);
        let count = to
            .checked_sub(from)
            .and_then(|n| n.checked_add(1))
            .filter(|count| *count <= 10_000)
            .ok_or_else(|| {
                Error::Internal("Audit verification requires 1 to 10000 events".into())
            })?;
        let events = self
            .query_sequence(from.saturating_sub(1).max(1), to, count as usize + 1)
            .await?;
        if events.last().map(|event| event.sequence) != Some(to) {
            return Ok(AuditVerification::Incomplete);
        }
        match verify_stored_chain(&events, from) {
            Ok(None) => Ok(AuditVerification::Consistent),
            Ok(Some(sequence)) => Ok(AuditVerification::Broken { sequence }),
            Err(_) => Ok(AuditVerification::Incomplete),
        }
    }

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

#[cfg(test)]
mod bounded_tests {
    use super::*;
    use crate::audit::{AuditChain, AuditEventKind, AuditSeverity};

    struct Memory(Vec<AuditEvent>);

    #[async_trait]
    impl AuditStorage for Memory {
        async fn append(&self, _: &AuditEvent) -> Result<(), Error> {
            Ok(())
        }
        async fn latest(&self) -> Result<Option<AuditEvent>, Error> {
            Ok(self.0.last().cloned())
        }
        async fn query_range(
            &self,
            _: DateTime<Utc>,
            _: DateTime<Utc>,
            _: usize,
        ) -> Result<Vec<AuditEvent>, Error> {
            unreachable!()
        }
        async fn verify_chain(&self, _: u64) -> Result<Option<u64>, Error> {
            unreachable!()
        }
        async fn query_sequence(
            &self,
            from: u64,
            to: u64,
            limit: usize,
        ) -> Result<Vec<AuditEvent>, Error> {
            Ok(self
                .0
                .iter()
                .filter(|event| (from..=to).contains(&event.sequence))
                .take(limit)
                .cloned()
                .collect())
        }
    }

    fn memory() -> Memory {
        let mut chain = AuditChain::new("bounded-test".into());
        Memory(
            (0..5)
                .map(|_| {
                    chain.seal(AuditEvent::new(
                        AuditEventKind::HttpRequest,
                        AuditSeverity::Informational,
                        "bounded-test".into(),
                    ))
                })
                .collect(),
        )
    }

    #[tokio::test]
    async fn custom_storage_reports_filtered_query_unsupported() {
        assert!(memory()
            .query_filtered(&AuditQuery::default())
            .await
            .unwrap_err()
            .to_string()
            .contains("unsupported"));
    }

    #[tokio::test]
    async fn bounded_checks_ignore_later_corruption_but_check_predecessor() {
        let mut storage = memory();
        storage.0[4].path = Some("tampered".into());
        assert_eq!(
            storage.verify_chain_range(2, 4).await.unwrap(),
            AuditVerification::Consistent
        );
        assert_eq!(
            storage.verify_chain_range(2, 5).await.unwrap(),
            AuditVerification::Broken { sequence: 5 }
        );
        storage.0[0].hash = None;
        assert_eq!(
            storage.verify_chain_range(2, 4).await.unwrap(),
            AuditVerification::Broken { sequence: 1 }
        );
    }

    #[tokio::test]
    async fn missing_endpoints_and_retained_anchor_are_incomplete() {
        let mut storage = memory();
        assert_eq!(
            storage.verify_chain_range(0, 3).await.unwrap(),
            AuditVerification::Consistent
        );
        assert_eq!(
            storage.verify_chain_range(3, 6).await.unwrap(),
            AuditVerification::Incomplete
        );
        assert_eq!(
            storage.verify_chain_range(6, 7).await.unwrap(),
            AuditVerification::Incomplete
        );
        storage.0.drain(..2);
        assert_eq!(storage.sequence_bounds().await.unwrap(), Some((3, 5)));
        assert_eq!(
            storage.verify_chain_range(3, 5).await.unwrap(),
            AuditVerification::Incomplete
        );
        assert_eq!(
            storage.verify_chain_range(4, 5).await.unwrap(),
            AuditVerification::Consistent
        );
        storage.0.clear();
        assert_eq!(storage.sequence_bounds().await.unwrap(), None);
        assert_eq!(
            storage.verify_chain_range(1, 1).await.unwrap(),
            AuditVerification::Incomplete
        );
    }

    #[tokio::test]
    async fn internal_gap_is_broken_and_invalid_ranges_are_rejected() {
        let mut storage = memory();
        storage.0.remove(2);
        assert_eq!(
            storage.verify_chain_range(2, 5).await.unwrap(),
            AuditVerification::Broken { sequence: 4 }
        );
        assert!(storage.verify_chain_range(3, 2).await.is_err());
        assert!(storage.verify_chain_range(1, 10_001).await.is_err());
        assert!(storage.verify_chain_range(1, u64::MAX).await.is_err());
    }
    #[tokio::test]
    async fn application_state_preserves_shared_storage_identity() {
        let storage: std::sync::Arc<dyn AuditStorage> = std::sync::Arc::new(memory());
        let state = crate::state::AppState::<()>::builder()
            .without_tracing()
            .audit_storage(storage.clone())
            .build()
            .await
            .unwrap();
        assert!(std::sync::Arc::ptr_eq(
            state.audit_storage().unwrap(),
            &storage
        ));
        assert!(std::sync::Arc::ptr_eq(
            state.clone().audit_storage().unwrap(),
            &storage
        ));
        assert!(crate::state::AppState::<()>::default()
            .audit_storage()
            .is_none());
    }
}

#[cfg(test)]
mod query_tests {
    use super::*;
    #[test]
    fn query_rejects_invalid_ranges_and_preserves_exact_filter_semantics() {
        let mut q = AuditQuery::default();
        assert!(q.validate().is_ok());
        for limit in [0, 1001, usize::MAX] {
            q.limit = limit;
            assert!(q.validate().is_err());
        }
        q = AuditQuery {
            cursor: Some(u64::MAX),
            ..AuditQuery::default()
        };
        assert!(q.validate().is_err());
        let mut e = AuditEvent::new(
            AuditEventKind::AuthTokenValidated,
            AuditSeverity::Notice,
            "test".into(),
        );
        e.sequence = 5;
        e.metadata =
            Some(serde_json::json!({"user":"user_a","schema":"case","entity_id":"case_a"}));
        q = AuditQuery {
            actor: Some("user_a".into()),
            schema: Some("case".into()),
            entity_id: Some("case_a".into()),
            cursor: Some(6),
            through_sequence: Some(5),
            ..AuditQuery::default()
        };
        assert!(q.matches(&e));
        q.cursor = Some(5);
        assert!(!q.matches(&e));
        q.order = AuditOrder::OldestFirst;
        q.cursor = Some(4);
        assert!(q.matches(&e));
        q.through_sequence = Some(4);
        assert!(!q.matches(&e));
        q.through_sequence = None;
        q.subject = Some("user_a".into());
        assert!(!q.matches(&e));
        e.source.subject = Some("user_a".into());
        assert!(q.matches(&e));
        q.metadata_kinds = Some(vec![]);
        assert!(!q.matches(&e)); // Resource metadata cannot match disallowed kinds.
        q.schema = None;
        q.entity_id = None;
        assert!(q.matches(&e)); // The source subject remains available.
        e.source.subject = None;
        assert!(!q.matches(&e));
        q.metadata_kinds = Some(vec!["auth.token.validated".into()]);
        q.subject = None;
        assert!(q.matches(&e));
        q.actor = Some("USER_A".into());
        assert!(!q.matches(&e));
    }
}
