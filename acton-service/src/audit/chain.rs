//! BLAKE3 hash chain for audit event integrity
//!
//! Provides tamper detection by computing a BLAKE3 hash over each event's content
//! plus the previous event's hash, forming an ordered chain. Any modification to
//! a past event invalidates all subsequent hashes.
//!
//! `AuditChain` is intentionally NOT `Send`/`Sync` — it is owned exclusively by
//! the `AuditAgent` actor, which processes events sequentially.

use super::event::AuditEvent;

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

    /// Compute the BLAKE3 hash for an event
    ///
    /// The hash covers: sequence, previous_hash, id, timestamp, kind, severity,
    /// service_name, method, path, status_code, and source.subject.
    /// This makes the hash deterministic and verifiable.
    fn compute_hash(&self, event: &AuditEvent) -> String {
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

/// Recompute the BLAKE3 hash for a single event (for verification)
fn recompute_hash(event: &AuditEvent) -> String {
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
}
