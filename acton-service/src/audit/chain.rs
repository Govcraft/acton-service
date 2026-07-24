//! BLAKE3 hash chain for audit event integrity
//!
//! Provides tamper detection by computing a BLAKE3 hash over each event's content
//! plus the previous event's hash, forming an ordered chain. Any modification to
//! a past event invalidates all subsequent hashes.
//!
//! # Hash versions
//!
//! The hash string is self-describing: a `v2:` prefix marks the current
//! scheme; an unprefixed hash is the legacy v1 scheme. The version travels
//! with the hash itself — through storage columns, syslog export, and
//! archives — so mixed-version chains verify without any schema change, and
//! [`verify_chain`] dispatches per event.
//!
//! - **v1** (legacy): covered sequence, previous hash, id, timestamp, kind,
//!   severity, service name, method, path, status code, and subject —
//!   concatenated without framing. Metadata, source IP, user agent, request
//!   ID, and duration were *not* covered.
//! - **v2**: covers every event field, each framed with a presence tag and a
//!   little-endian length prefix so field boundaries are unambiguous.
//!   Metadata is hashed as canonical JSON (recursively key-sorted), which
//!   stays byte-stable across serializers that preserve insertion order and
//!   across stores that re-normalize JSON.
//!
//! A forged downgrade (rewriting a v2 event and stamping it with an
//! unprefixed v1 hash) is caught by the next event's `previous_hash`, which
//! stores the original prefixed string. The final event of a chain has no
//! successor, so its version — like its existence — is only as trustworthy
//! as the chain tail ever is; anchoring the head externally is what bounds
//! truncation, not the hash scheme.
//!
//! `AuditChain` is intentionally NOT `Send`/`Sync` — it is owned exclusively by
//! the `AuditAgent` actor, which processes events sequentially.

use super::event::AuditEvent;

/// Prefix marking a hash computed under the v2 (full-coverage) scheme.
const HASH_V2_PREFIX: &str = "v2:";

/// BLAKE3 hash chain state
///
/// Maintains the running chain state (previous hash + sequence number).
/// Owned by `AuditAgent` — not thread-safe by design, since actor message
/// processing is inherently sequential.
pub struct AuditChain {
    previous_hash: Option<String>,
    sequence: u64,
    service_name: String,
}

impl AuditChain {
    /// Create a new chain starting from genesis (no previous hash)
    pub fn new(service_name: String) -> Self {
        Self {
            previous_hash: None,
            sequence: 0,
            service_name,
        }
    }

    /// Resume an existing chain from the last known state
    ///
    /// Used when the `AuditAgent` starts up and loads the latest event
    /// from storage to continue the chain.
    pub fn resume(service_name: String, previous_hash: String, sequence: u64) -> Self {
        Self {
            previous_hash: Some(previous_hash),
            sequence,
            service_name,
        }
    }

    /// Seal an event by computing its BLAKE3 hash and advancing the chain
    ///
    /// Sets the event's `hash`, `previous_hash`, `sequence`, and `service_name` fields.
    /// Returns the event with chain fields populated.
    pub fn seal(&mut self, mut event: AuditEvent) -> AuditEvent {
        self.sequence += 1;
        event.sequence = self.sequence;
        event.previous_hash = self.previous_hash.clone();
        event.service_name = self.service_name.clone();

        // Compute BLAKE3 hash over canonical fields
        let hash = self.compute_hash(&event);
        event.hash = Some(hash.clone());
        self.previous_hash = Some(hash);

        event
    }

    /// Current sequence number
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Current chain tip hash
    pub fn previous_hash(&self) -> Option<&str> {
        self.previous_hash.as_deref()
    }

    /// Compute the current (v2) hash for an event.
    fn compute_hash(&self, event: &AuditEvent) -> String {
        compute_hash_v2(event)
    }
}

/// Serialize a JSON value with recursively sorted object keys.
///
/// This is the canonical form the v2 hash consumes for `metadata`, and the
/// form the syslog exporter emits, so the two always agree. Plain
/// `serde_json::to_string` is not stable enough for either job: with the
/// `preserve_order` feature (which any crate in a shared workspace can switch
/// on), objects serialize in insertion order, and JSON stores like Postgres
/// `jsonb` re-normalize key order on round-trip.
pub(crate) fn canonical_json(value: &serde_json::Value) -> String {
    fn write_value(value: &serde_json::Value, out: &mut String) {
        match value {
            serde_json::Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort_unstable();
                out.push('{');
                for (index, key) in keys.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    // Key and scalar serialization cannot fail: a JSON string
                    // or scalar has no fallible Serialize path.
                    out.push_str(&serde_json::to_string(key).expect("JSON string serializes"));
                    out.push(':');
                    write_value(&map[key.as_str()], out);
                }
                out.push('}');
            }
            serde_json::Value::Array(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_value(item, out);
                }
                out.push(']');
            }
            scalar => {
                out.push_str(&serde_json::to_string(scalar).expect("JSON scalar serializes"));
            }
        }
    }

    let mut out = String::new();
    write_value(value, &mut out);
    out
}

/// Compute the v2 (full-coverage) hash for an event, `v2:`-prefixed.
///
/// Every field is framed as a presence tag (`0`/`1`) followed, when present,
/// by a little-endian `u64` byte length and the bytes themselves, in this
/// fixed order: sequence, previous_hash, id, timestamp (RFC 3339), kind,
/// severity, service_name, method, path, status_code, subject, ip,
/// user_agent, request_id, duration_ms, metadata (canonical JSON). The
/// framing makes field boundaries unambiguous, unlike the v1 concatenation.
fn compute_hash_v2(event: &AuditEvent) -> String {
    let mut hasher = blake3::Hasher::new();

    let mut frame = |bytes: Option<&[u8]>| match bytes {
        Some(bytes) => {
            hasher.update(&[1]);
            hasher.update((bytes.len() as u64).to_le_bytes().as_ref());
            hasher.update(bytes);
        }
        None => {
            hasher.update(&[0]);
        }
    };

    frame(Some(event.sequence.to_le_bytes().as_ref()));
    frame(event.previous_hash.as_deref().map(str::as_bytes));
    frame(Some(event.id.as_bytes()));
    frame(Some(event.timestamp.to_rfc3339().as_bytes()));
    frame(Some(event.kind.to_string().as_bytes()));
    frame(Some(&[event.severity.as_syslog_severity()]));
    frame(Some(event.service_name.as_bytes()));
    frame(event.method.as_deref().map(str::as_bytes));
    frame(event.path.as_deref().map(str::as_bytes));
    let status_bytes = event.status_code.map(u16::to_le_bytes);
    frame(status_bytes.as_ref().map(<[u8; 2]>::as_slice));
    frame(event.source.subject.as_deref().map(str::as_bytes));
    frame(event.source.ip.as_deref().map(str::as_bytes));
    frame(event.source.user_agent.as_deref().map(str::as_bytes));
    frame(event.source.request_id.as_deref().map(str::as_bytes));
    let duration_bytes = event.duration_ms.map(u64::to_le_bytes);
    frame(duration_bytes.as_ref().map(<[u8; 8]>::as_slice));
    let metadata_canonical = event.metadata.as_ref().map(canonical_json);
    frame(metadata_canonical.as_deref().map(str::as_bytes));

    format!("{HASH_V2_PREFIX}{}", hasher.finalize().to_hex())
}

/// Verify a chain of events is intact
///
/// Recomputes hashes for the given events (which must be in sequence order)
/// and checks they match. Returns `Ok(())` if the chain is valid, or
/// `Err(ChainVerificationError)` with the sequence number of the first
/// broken link.
pub fn verify_chain(events: &[AuditEvent]) -> Result<(), ChainVerificationError> {
    if events.is_empty() {
        return Ok(());
    }

    let mut expected_prev: Option<String> = None;

    for event in events {
        // Check previous_hash linkage
        if event.previous_hash != expected_prev {
            return Err(ChainVerificationError {
                sequence: event.sequence,
                expected_previous_hash: expected_prev,
                actual_previous_hash: event.previous_hash.clone(),
            });
        }

        // Recompute hash and verify
        let recomputed = recompute_hash(event);
        if event.hash.as_deref() != Some(recomputed.as_str()) {
            return Err(ChainVerificationError {
                sequence: event.sequence,
                expected_previous_hash: expected_prev,
                actual_previous_hash: event.previous_hash.clone(),
            });
        }

        expected_prev = event.hash.clone();
    }

    Ok(())
}

/// Recompute the hash for a single event (for verification), dispatching on
/// the version prefix carried by the event's own hash string. An event with
/// no hash at all recomputes under the current scheme, which cannot match —
/// verification then fails, as it should.
fn recompute_hash(event: &AuditEvent) -> String {
    match event.hash.as_deref() {
        Some(hash) if hash.starts_with(HASH_V2_PREFIX) => compute_hash_v2(event),
        Some(_) => compute_hash_v1(event),
        None => compute_hash_v2(event),
    }
}

/// The legacy (v1) hash: unframed concatenation of a subset of fields.
/// Kept only so chains sealed before the v2 scheme still verify; nothing
/// seals with it anymore.
fn compute_hash_v1(event: &AuditEvent) -> String {
    let mut hasher = blake3::Hasher::new();

    hasher.update(event.sequence.to_le_bytes().as_ref());

    if let Some(ref prev) = event.previous_hash {
        hasher.update(prev.as_bytes());
    }

    hasher.update(event.id.as_bytes());
    hasher.update(event.timestamp.to_rfc3339().as_bytes());
    hasher.update(event.kind.to_string().as_bytes());
    hasher.update(&[event.severity.as_syslog_severity()]);
    hasher.update(event.service_name.as_bytes());

    if let Some(ref method) = event.method {
        hasher.update(method.as_bytes());
    }
    if let Some(ref path) = event.path {
        hasher.update(path.as_bytes());
    }
    if let Some(code) = event.status_code {
        hasher.update(code.to_le_bytes().as_ref());
    }
    if let Some(ref subject) = event.source.subject {
        hasher.update(subject.as_bytes());
    }

    hasher.finalize().to_hex().to_string()
}

/// Error returned when chain verification detects a broken link
#[derive(Debug)]
pub struct ChainVerificationError {
    /// Sequence number where the chain is broken
    pub sequence: u64,
    /// What the previous hash should have been
    pub expected_previous_hash: Option<String>,
    /// What the previous hash actually was
    pub actual_previous_hash: Option<String>,
}

impl std::fmt::Display for ChainVerificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Audit chain broken at sequence {}: expected previous_hash {:?}, got {:?}",
            self.sequence, self.expected_previous_hash, self.actual_previous_hash
        )
    }
}

impl std::error::Error for ChainVerificationError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::event::{AuditEventKind, AuditSeverity};

    fn make_event(kind: AuditEventKind) -> AuditEvent {
        AuditEvent::new(
            kind,
            AuditSeverity::Informational,
            "test-service".to_string(),
        )
    }

    #[test]
    fn test_chain_seal_sets_fields() {
        let mut chain = AuditChain::new("test-service".to_string());
        let event = make_event(AuditEventKind::AuthLoginSuccess);

        let sealed = chain.seal(event);
        assert_eq!(sealed.sequence, 1);
        assert!(sealed.hash.is_some());
        assert!(sealed.previous_hash.is_none()); // First event has no previous
    }

    #[test]
    fn test_chain_links_events() {
        let mut chain = AuditChain::new("test-service".to_string());

        let e1 = chain.seal(make_event(AuditEventKind::AuthLoginSuccess));
        let e2 = chain.seal(make_event(AuditEventKind::HttpRequest));

        assert_eq!(e1.sequence, 1);
        assert_eq!(e2.sequence, 2);
        assert_eq!(e2.previous_hash, e1.hash);
    }

    #[test]
    fn test_chain_deterministic_hash() {
        let mut chain1 = AuditChain::new("test-service".to_string());
        let mut chain2 = AuditChain::new("test-service".to_string());

        // Use the same event for both chains
        let event = make_event(AuditEventKind::AuthLoginSuccess);
        let event_clone = event.clone();

        let sealed1 = chain1.seal(event);
        let sealed2 = chain2.seal(event_clone);

        // Same input produces same hash
        assert_eq!(sealed1.hash, sealed2.hash);
    }

    #[test]
    fn test_chain_resume() {
        let mut chain = AuditChain::new("test-service".to_string());
        let e1 = chain.seal(make_event(AuditEventKind::AuthLoginSuccess));
        let prev_hash = e1.hash.clone().unwrap();

        // Resume from the last event
        let mut resumed = AuditChain::resume("test-service".to_string(), prev_hash.clone(), 1);
        let e2 = resumed.seal(make_event(AuditEventKind::HttpRequest));

        assert_eq!(e2.sequence, 2);
        assert_eq!(e2.previous_hash, Some(prev_hash));
    }

    #[test]
    fn test_verify_chain_valid() {
        let mut chain = AuditChain::new("test-service".to_string());
        let events: Vec<AuditEvent> = (0..5)
            .map(|_| chain.seal(make_event(AuditEventKind::HttpRequest)))
            .collect();

        assert!(verify_chain(&events).is_ok());
    }

    #[test]
    fn test_verify_chain_tampered() {
        let mut chain = AuditChain::new("test-service".to_string());
        let mut events: Vec<AuditEvent> = (0..5)
            .map(|_| chain.seal(make_event(AuditEventKind::HttpRequest)))
            .collect();

        // Tamper with the third event's hash
        events[2].hash = Some("tampered".to_string());

        // Event 3 itself will fail verification (hash mismatch),
        // and event 4's previous_hash won't match event 3's tampered hash
        let result = verify_chain(&events);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_chain_empty() {
        assert!(verify_chain(&[]).is_ok());
    }

    #[test]
    fn test_verify_chain_single_event() {
        let mut chain = AuditChain::new("test-service".to_string());
        let event = chain.seal(make_event(AuditEventKind::AuthLoginSuccess));
        assert!(verify_chain(&[event]).is_ok());
    }

    #[test]
    fn test_chain_sequence_monotonic() {
        let mut chain = AuditChain::new("test-service".to_string());
        let mut prev_seq = 0;
        for _ in 0..10 {
            let event = chain.seal(make_event(AuditEventKind::HttpRequest));
            assert!(event.sequence > prev_seq);
            prev_seq = event.sequence;
        }
    }

    /// Builds an event carrying every field the v1 hash ignored.
    fn make_rich_event() -> AuditEvent {
        let mut event = make_event(AuditEventKind::HttpRequest).with_source(
            crate::audit::event::AuditSource {
                ip: Some("198.51.100.42".to_string()),
                user_agent: Some("curl/8.0".to_string()),
                subject: Some("operator-1".to_string()),
                request_id: Some("req_abc".to_string()),
            },
        );
        event = event.with_http("POST".to_string(), "/admin/x".to_string(), Some(200), Some(12));
        event.metadata = Some(serde_json::json!({"roles": ["auditor"], "action": "readX"}));
        event
    }

    #[test]
    fn sealed_hashes_carry_the_v2_prefix() {
        let mut chain = AuditChain::new("test-service".to_string());
        let sealed = chain.seal(make_rich_event());
        assert!(sealed.hash.as_deref().unwrap().starts_with("v2:"));
    }

    /// The v1 blind spots — metadata, source IP, user agent, request ID, and
    /// duration — must each invalidate the v2 hash when tampered with.
    #[test]
    fn v2_hash_covers_the_fields_v1_ignored() {
        let mut chain = AuditChain::new("test-service".to_string());
        let sealed = chain.seal(make_rich_event());

        let tampers: [fn(&mut AuditEvent); 5] = [
            |e| e.metadata = Some(serde_json::json!({"roles": ["provisioner"]})),
            |e| e.source.ip = Some("203.0.113.99".to_string()),
            |e| e.source.user_agent = Some("evil/1.0".to_string()),
            |e| e.source.request_id = Some("req_forged".to_string()),
            |e| e.duration_ms = Some(9999),
        ];

        for tamper in tampers {
            let mut tampered = sealed.clone();
            tamper(&mut tampered);
            assert!(
                verify_chain(std::slice::from_ref(&tampered)).is_err(),
                "a tampered field the v1 hash ignored must break v2 verification"
            );
        }

        assert!(verify_chain(std::slice::from_ref(&sealed)).is_ok());
    }

    /// A chain spanning the scheme change — legacy v1 events followed by v2
    /// events — verifies end to end, with each hash judged under its own
    /// scheme.
    #[test]
    fn mixed_v1_and_v2_chains_verify() {
        // Hand-seal a legacy event exactly as the v1 chain did.
        let mut legacy = make_event(AuditEventKind::AuthLoginSuccess);
        legacy.sequence = 1;
        legacy.previous_hash = None;
        let legacy_hash = compute_hash_v1(&legacy);
        assert!(!legacy_hash.starts_with("v2:"));
        legacy.hash = Some(legacy_hash.clone());

        // Continue the chain under the current scheme.
        let mut chain = AuditChain::resume("test-service".to_string(), legacy_hash, 1);
        let successor = chain.seal(make_rich_event());
        assert!(successor.hash.as_deref().unwrap().starts_with("v2:"));

        assert!(verify_chain(&[legacy, successor]).is_ok());
    }

    /// Rewriting a v2 event and stamping it with an unprefixed v1 hash must
    /// not slip past verification when the event has a successor: the
    /// successor's `previous_hash` pins the original v2 string.
    #[test]
    fn downgrade_forgery_is_caught_by_the_successor_link() {
        let mut chain = AuditChain::new("test-service".to_string());
        let first = chain.seal(make_rich_event());
        let second = chain.seal(make_event(AuditEventKind::HttpRequest));

        let mut forged = first.clone();
        forged.metadata = Some(serde_json::json!({"roles": ["provisioner"]}));
        forged.hash = Some(compute_hash_v1(&forged));

        assert!(verify_chain(&[forged, second]).is_err());
    }

    #[test]
    fn canonical_json_sorts_keys_recursively() {
        let value = serde_json::json!({
            "zeta": {"b": 2, "a": 1},
            "alpha": [{"y": true, "x": false}],
        });
        assert_eq!(
            canonical_json(&value),
            r#"{"alpha":[{"x":false,"y":true}],"zeta":{"a":1,"b":2}}"#
        );
    }
}
