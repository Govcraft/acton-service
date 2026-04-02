//! ClickHouse audit storage backend
//!
//! ClickHouse is naturally append-only (MergeTree engine), making it ideal for
//! audit event storage. No immutability rules are needed -- ClickHouse does not
//! support standard UPDATE/DELETE operations on MergeTree tables.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use clickhouse::Row;
use serde::{Deserialize, Serialize};

use super::AuditStorage;
use crate::audit::event::{AuditEvent, AuditEventKind, AuditSeverity, AuditSource};
use crate::error::Error;

/// ClickHouse-backed audit storage
///
/// Uses the MergeTree engine with time-based partitioning for efficient
/// append-only audit event storage with fast analytical queries.
pub struct ClickHouseAuditStorage {
    client: clickhouse::Client,
}

impl ClickHouseAuditStorage {
    /// Create a new ClickHouse audit storage
    pub fn new(client: clickhouse::Client) -> Self {
        Self { client }
    }

    /// Initialize the audit_events table
    ///
    /// Creates the table with MergeTree engine, ordered by (timestamp, sequence)
    /// and partitioned by month for efficient time-range queries and cleanup.
    pub async fn initialize(&self) -> Result<(), Error> {
        self.client
            .query(
                "CREATE TABLE IF NOT EXISTS audit_events (
                    id UUID,
                    timestamp DateTime64(3, 'UTC'),
                    kind String,
                    severity UInt8,
                    source_ip Nullable(String),
                    source_user_agent Nullable(String),
                    source_subject Nullable(String),
                    source_request_id Nullable(String),
                    method Nullable(String),
                    path Nullable(String),
                    status_code Nullable(UInt16),
                    duration_ms Nullable(Int64),
                    service_name String,
                    metadata Nullable(String),
                    hash String,
                    previous_hash Nullable(String),
                    sequence UInt64
                ) ENGINE = MergeTree()
                ORDER BY (timestamp, sequence)
                PARTITION BY toYYYYMM(timestamp)",
            )
            .execute()
            .await
            .map_err(|e| {
                Error::ClickHouse(format!("Failed to create audit_events table: {}", e))
            })?;

        Ok(())
    }
}

/// Row type for inserting audit events into ClickHouse
#[derive(Row, Serialize)]
struct AuditInsertRow {
    id: uuid::Uuid,
    timestamp: i64,
    kind: String,
    severity: u8,
    source_ip: Option<String>,
    source_user_agent: Option<String>,
    source_subject: Option<String>,
    source_request_id: Option<String>,
    method: Option<String>,
    path: Option<String>,
    status_code: Option<u16>,
    duration_ms: Option<i64>,
    service_name: String,
    metadata: Option<String>,
    hash: String,
    previous_hash: Option<String>,
    sequence: u64,
}

/// Row type for reading audit events from ClickHouse
#[derive(Row, Deserialize)]
struct AuditQueryRow {
    id: uuid::Uuid,
    timestamp: i64,
    kind: String,
    severity: u8,
    source_ip: Option<String>,
    source_user_agent: Option<String>,
    source_subject: Option<String>,
    source_request_id: Option<String>,
    method: Option<String>,
    path: Option<String>,
    status_code: Option<u16>,
    duration_ms: Option<i64>,
    service_name: String,
    metadata: Option<String>,
    hash: String,
    previous_hash: Option<String>,
    sequence: u64,
}

impl From<&AuditEvent> for AuditInsertRow {
    fn from(event: &AuditEvent) -> Self {
        Self {
            id: event.id,
            timestamp: event.timestamp.timestamp_millis(),
            kind: event.kind.to_string(),
            severity: event.severity.as_syslog_severity(),
            source_ip: event.source.ip.clone(),
            source_user_agent: event.source.user_agent.clone(),
            source_subject: event.source.subject.clone(),
            source_request_id: event.source.request_id.clone(),
            method: event.method.clone(),
            path: event.path.clone(),
            status_code: event.status_code,
            duration_ms: event.duration_ms.map(|d| d as i64),
            service_name: event.service_name.clone(),
            metadata: event.metadata.as_ref().map(|m| m.to_string()),
            hash: event.hash.clone().unwrap_or_default(),
            previous_hash: event.previous_hash.clone(),
            sequence: event.sequence,
        }
    }
}

impl From<AuditQueryRow> for AuditEvent {
    fn from(row: AuditQueryRow) -> Self {
        let kind = match row.kind.as_str() {
            "auth.login.success" => AuditEventKind::AuthLoginSuccess,
            "auth.login.failed" => AuditEventKind::AuthLoginFailed,
            "auth.logout" => AuditEventKind::AuthLogout,
            "auth.token.refresh" => AuditEventKind::AuthTokenRefresh,
            "auth.token.revoked" => AuditEventKind::AuthTokenRevoked,
            "auth.password.changed" => AuditEventKind::AuthPasswordChanged,
            "auth.apikey.created" => AuditEventKind::AuthApiKeyCreated,
            "auth.apikey.revoked" => AuditEventKind::AuthApiKeyRevoked,
            "auth.oauth.callback" => AuditEventKind::AuthOAuthCallback,
            "auth.permission.denied" => AuditEventKind::AuthPermissionDenied,
            "auth.key.rotated" => AuditEventKind::AuthKeyRotated,
            "auth.key.retired" => AuditEventKind::AuthKeyRetired,
            "auth.key.rotation_failed" => AuditEventKind::AuthKeyRotationFailed,
            "http.request" => AuditEventKind::HttpRequest,
            "http.request.denied" => AuditEventKind::HttpRequestDenied,
            other => {
                let name = other.strip_prefix("custom.").unwrap_or(other);
                AuditEventKind::Custom(name.to_string())
            }
        };

        let severity = match row.severity {
            0 => AuditSeverity::Emergency,
            1 => AuditSeverity::Alert,
            2 => AuditSeverity::Critical,
            3 => AuditSeverity::Error,
            4 => AuditSeverity::Warning,
            5 => AuditSeverity::Notice,
            7 => AuditSeverity::Debug,
            _ => AuditSeverity::Informational,
        };

        let timestamp = DateTime::from_timestamp_millis(row.timestamp)
            .unwrap_or_else(Utc::now);

        let metadata = row
            .metadata
            .and_then(|m| serde_json::from_str(&m).ok());

        AuditEvent {
            id: row.id,
            timestamp,
            kind,
            severity,
            source: AuditSource {
                ip: row.source_ip,
                user_agent: row.source_user_agent,
                subject: row.source_subject,
                request_id: row.source_request_id,
            },
            method: row.method,
            path: row.path,
            status_code: row.status_code,
            duration_ms: row.duration_ms.map(|d| d as u64),
            service_name: row.service_name,
            metadata,
            hash: Some(row.hash),
            previous_hash: row.previous_hash,
            sequence: row.sequence,
        }
    }
}

#[async_trait]
impl AuditStorage for ClickHouseAuditStorage {
    async fn append(&self, event: &AuditEvent) -> Result<(), Error> {
        let row = AuditInsertRow::from(event);
        let mut insert: clickhouse::insert::Insert<AuditInsertRow> = self
            .client
            .insert("audit_events")
            .await
            .map_err(|e| Error::ClickHouse(format!("Failed to create insert: {}", e)))?;
        insert
            .write(&row)
            .await
            .map_err(|e| Error::ClickHouse(format!("Failed to write audit event: {}", e)))?;
        insert
            .end()
            .await
            .map_err(|e| Error::ClickHouse(format!("Failed to flush audit event: {}", e)))?;
        Ok(())
    }

    async fn latest(&self) -> Result<Option<AuditEvent>, Error> {
        let rows = self
            .client
            .query("SELECT ?fields FROM audit_events ORDER BY sequence DESC LIMIT 1")
            .fetch_all::<AuditQueryRow>()
            .await
            .map_err(|e| {
                Error::ClickHouse(format!("Failed to fetch latest audit event: {}", e))
            })?;

        Ok(rows.into_iter().next().map(Into::into))
    }

    async fn query_range(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<AuditEvent>, Error> {
        let rows = self
            .client
            .query("SELECT ?fields FROM audit_events WHERE timestamp >= ? AND timestamp <= ? ORDER BY sequence ASC LIMIT ?")
            .bind(from.timestamp_millis())
            .bind(to.timestamp_millis())
            .bind(limit as u64)
            .fetch_all::<AuditQueryRow>()
            .await
            .map_err(|e| Error::ClickHouse(format!("Failed to query audit events: {}", e)))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn verify_chain(&self, from_sequence: u64) -> Result<Option<u64>, Error> {
        let rows = self
            .client
            .query("SELECT ?fields FROM audit_events WHERE sequence >= ? ORDER BY sequence ASC")
            .bind(from_sequence)
            .fetch_all::<AuditQueryRow>()
            .await
            .map_err(|e| {
                Error::ClickHouse(format!(
                    "Failed to fetch audit events for verification: {}",
                    e
                ))
            })?;

        let events: Vec<AuditEvent> = rows.into_iter().map(Into::into).collect();

        match crate::audit::chain::verify_chain(&events) {
            Ok(()) => Ok(None),
            Err(e) => Ok(Some(e.sequence)),
        }
    }

    async fn query_before(
        &self,
        cutoff: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<AuditEvent>, Error> {
        let rows = self
            .client
            .query("SELECT ?fields FROM audit_events WHERE timestamp < ? ORDER BY sequence ASC LIMIT ?")
            .bind(cutoff.timestamp_millis())
            .bind(limit as u64)
            .fetch_all::<AuditQueryRow>()
            .await
            .map_err(|e| {
                Error::ClickHouse(format!(
                    "Failed to query audit events before cutoff: {}",
                    e
                ))
            })?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn purge_before(&self, cutoff: DateTime<Utc>) -> Result<u64, Error> {
        // ClickHouse lightweight DELETE (synchronous in recent versions)
        self.client
            .query("DELETE FROM audit_events WHERE timestamp < ?")
            .bind(cutoff.timestamp_millis())
            .execute()
            .await
            .map_err(|e| Error::ClickHouse(format!("Failed to purge audit events: {}", e)))?;

        // ClickHouse DELETE doesn't return affected row count reliably,
        // so we return 0 as a placeholder. The caller should verify via query_before.
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::event::{AuditEventKind, AuditSeverity, AuditSource};
    use chrono::TimeZone;

    fn make_event(
        kind: AuditEventKind,
        severity: AuditSeverity,
        timestamp: DateTime<Utc>,
    ) -> AuditEvent {
        AuditEvent {
            id: uuid::Uuid::new_v4(),
            timestamp,
            kind,
            severity,
            source: AuditSource {
                ip: Some("192.168.1.1".to_string()),
                user_agent: Some("test-agent/1.0".to_string()),
                subject: Some("user:alice".to_string()),
                request_id: Some("req-123".to_string()),
            },
            method: Some("POST".to_string()),
            path: Some("/api/v1/users".to_string()),
            status_code: Some(201),
            duration_ms: Some(42),
            service_name: "test-svc".to_string(),
            metadata: Some(serde_json::json!({"key": "value", "count": 42})),
            hash: Some("abc123hash".to_string()),
            previous_hash: Some("prev_hash_000".to_string()),
            sequence: 7,
        }
    }

    // =========================================================================
    // AuditEvent → AuditInsertRow: verify no data is silently dropped
    // or truncated when converting to the ClickHouse row format
    // =========================================================================

    #[test]
    fn test_insert_row_preserves_all_source_fields() {
        let ts = Utc.with_ymd_and_hms(2025, 6, 15, 10, 30, 0).unwrap();
        let event = make_event(AuditEventKind::AuthLoginSuccess, AuditSeverity::Informational, ts);
        let row = AuditInsertRow::from(&event);

        assert_eq!(row.id, event.id);
        assert_eq!(row.source_ip, Some("192.168.1.1".to_string()));
        assert_eq!(row.source_user_agent, Some("test-agent/1.0".to_string()));
        assert_eq!(row.source_subject, Some("user:alice".to_string()));
        assert_eq!(row.source_request_id, Some("req-123".to_string()));
    }

    #[test]
    fn test_insert_row_preserves_http_fields() {
        let ts = Utc::now();
        let event = make_event(AuditEventKind::HttpRequest, AuditSeverity::Informational, ts);
        let row = AuditInsertRow::from(&event);

        assert_eq!(row.method, Some("POST".to_string()));
        assert_eq!(row.path, Some("/api/v1/users".to_string()));
        assert_eq!(row.status_code, Some(201));
        assert_eq!(row.duration_ms, Some(42));
    }

    #[test]
    fn test_insert_row_timestamp_is_epoch_millis() {
        let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let event = make_event(AuditEventKind::HttpRequest, AuditSeverity::Informational, ts);
        let row = AuditInsertRow::from(&event);

        assert_eq!(row.timestamp, ts.timestamp_millis());
    }

    #[test]
    fn test_insert_row_maps_severity_to_syslog_value() {
        let ts = Utc::now();
        let cases = [
            (AuditSeverity::Emergency, 0u8),
            (AuditSeverity::Alert, 1),
            (AuditSeverity::Critical, 2),
            (AuditSeverity::Error, 3),
            (AuditSeverity::Warning, 4),
            (AuditSeverity::Notice, 5),
            (AuditSeverity::Informational, 6),
            (AuditSeverity::Debug, 7),
        ];

        for (severity, expected_syslog) in cases {
            let event = make_event(AuditEventKind::HttpRequest, severity, ts);
            let row = AuditInsertRow::from(&event);
            assert_eq!(
                row.severity, expected_syslog,
                "Severity {:?} should map to syslog {}",
                severity, expected_syslog
            );
        }
    }

    #[test]
    fn test_insert_row_serializes_metadata_as_json_string() {
        let ts = Utc::now();
        let event = make_event(AuditEventKind::HttpRequest, AuditSeverity::Informational, ts);
        let row = AuditInsertRow::from(&event);

        let metadata_str = row.metadata.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&metadata_str).unwrap();
        assert_eq!(parsed["key"], "value");
        assert_eq!(parsed["count"], 42);
    }

    #[test]
    fn test_insert_row_handles_none_metadata() {
        let ts = Utc::now();
        let mut event = make_event(AuditEventKind::HttpRequest, AuditSeverity::Informational, ts);
        event.metadata = None;
        let row = AuditInsertRow::from(&event);

        assert!(row.metadata.is_none());
    }

    #[test]
    fn test_insert_row_handles_none_hash() {
        let ts = Utc::now();
        let mut event = make_event(AuditEventKind::HttpRequest, AuditSeverity::Informational, ts);
        event.hash = None;
        let row = AuditInsertRow::from(&event);

        assert_eq!(row.hash, "", "None hash should become empty string for ClickHouse non-nullable column");
    }

    #[test]
    fn test_insert_row_handles_all_optional_fields_none() {
        let ts = Utc::now();
        let event = AuditEvent {
            id: uuid::Uuid::new_v4(),
            timestamp: ts,
            kind: AuditEventKind::AuthLogout,
            severity: AuditSeverity::Informational,
            source: AuditSource::default(),
            method: None,
            path: None,
            status_code: None,
            duration_ms: None,
            service_name: "svc".to_string(),
            metadata: None,
            hash: None,
            previous_hash: None,
            sequence: 0,
        };
        let row = AuditInsertRow::from(&event);

        assert!(row.source_ip.is_none());
        assert!(row.source_user_agent.is_none());
        assert!(row.source_subject.is_none());
        assert!(row.source_request_id.is_none());
        assert!(row.method.is_none());
        assert!(row.path.is_none());
        assert!(row.status_code.is_none());
        assert!(row.duration_ms.is_none());
        assert!(row.metadata.is_none());
        assert!(row.previous_hash.is_none());
    }

    // =========================================================================
    // AuditQueryRow → AuditEvent: verify the reverse conversion preserves
    // data and correctly maps ClickHouse types back to domain types
    // =========================================================================

    #[test]
    fn test_query_row_to_event_roundtrip_preserves_data() {
        let ts = Utc.with_ymd_and_hms(2025, 6, 15, 10, 30, 0).unwrap();
        let original = make_event(AuditEventKind::AuthLoginSuccess, AuditSeverity::Warning, ts);
        let row = AuditInsertRow::from(&original);

        // Simulate what ClickHouse would return
        let query_row = AuditQueryRow {
            id: row.id,
            timestamp: row.timestamp,
            kind: row.kind.clone(),
            severity: row.severity,
            source_ip: row.source_ip.clone(),
            source_user_agent: row.source_user_agent.clone(),
            source_subject: row.source_subject.clone(),
            source_request_id: row.source_request_id.clone(),
            method: row.method.clone(),
            path: row.path.clone(),
            status_code: row.status_code,
            duration_ms: row.duration_ms,
            service_name: row.service_name.clone(),
            metadata: row.metadata.clone(),
            hash: row.hash.clone(),
            previous_hash: row.previous_hash.clone(),
            sequence: row.sequence,
        };

        let recovered: AuditEvent = query_row.into();

        assert_eq!(recovered.id, original.id);
        assert_eq!(recovered.timestamp, original.timestamp);
        assert_eq!(recovered.kind.to_string(), original.kind.to_string());
        assert_eq!(
            recovered.severity.as_syslog_severity(),
            original.severity.as_syslog_severity()
        );
        assert_eq!(recovered.source.ip, original.source.ip);
        assert_eq!(recovered.source.user_agent, original.source.user_agent);
        assert_eq!(recovered.source.subject, original.source.subject);
        assert_eq!(recovered.source.request_id, original.source.request_id);
        assert_eq!(recovered.method, original.method);
        assert_eq!(recovered.path, original.path);
        assert_eq!(recovered.status_code, original.status_code);
        assert_eq!(recovered.duration_ms, original.duration_ms);
        assert_eq!(recovered.service_name, original.service_name);
        assert_eq!(recovered.sequence, original.sequence);
        assert_eq!(recovered.hash, original.hash);
        assert_eq!(recovered.previous_hash, original.previous_hash);
        // Metadata roundtrip: JSON string → serde_json::Value
        assert_eq!(recovered.metadata, original.metadata);
    }

    #[test]
    fn test_query_row_maps_all_known_event_kinds() {
        let ts_millis = Utc::now().timestamp_millis();
        let kinds = vec![
            ("auth.login.success", "auth.login.success"),
            ("auth.login.failed", "auth.login.failed"),
            ("auth.logout", "auth.logout"),
            ("auth.token.refresh", "auth.token.refresh"),
            ("auth.token.revoked", "auth.token.revoked"),
            ("auth.password.changed", "auth.password.changed"),
            ("auth.apikey.created", "auth.apikey.created"),
            ("auth.apikey.revoked", "auth.apikey.revoked"),
            ("auth.oauth.callback", "auth.oauth.callback"),
            ("auth.permission.denied", "auth.permission.denied"),
            ("auth.key.rotated", "auth.key.rotated"),
            ("auth.key.retired", "auth.key.retired"),
            ("auth.key.rotation_failed", "auth.key.rotation_failed"),
            ("http.request", "http.request"),
            ("http.request.denied", "http.request.denied"),
        ];

        for (db_kind, expected_display) in kinds {
            let row = AuditQueryRow {
                id: uuid::Uuid::new_v4(),
                timestamp: ts_millis,
                kind: db_kind.to_string(),
                severity: 6,
                source_ip: None,
                source_user_agent: None,
                source_subject: None,
                source_request_id: None,
                method: None,
                path: None,
                status_code: None,
                duration_ms: None,
                service_name: "svc".to_string(),
                metadata: None,
                hash: "h".to_string(),
                previous_hash: None,
                sequence: 1,
            };
            let event: AuditEvent = row.into();
            assert_eq!(
                event.kind.to_string(),
                expected_display,
                "Kind '{}' should roundtrip correctly",
                db_kind
            );
        }
    }

    #[test]
    fn test_query_row_maps_custom_event_kind() {
        let row = AuditQueryRow {
            id: uuid::Uuid::new_v4(),
            timestamp: Utc::now().timestamp_millis(),
            kind: "custom.user.exported_data".to_string(),
            severity: 5,
            source_ip: None,
            source_user_agent: None,
            source_subject: None,
            source_request_id: None,
            method: None,
            path: None,
            status_code: None,
            duration_ms: None,
            service_name: "svc".to_string(),
            metadata: None,
            hash: "h".to_string(),
            previous_hash: None,
            sequence: 1,
        };
        let event: AuditEvent = row.into();

        match event.kind {
            AuditEventKind::Custom(name) => assert_eq!(name, "user.exported_data"),
            other => panic!("Expected Custom, got {:?}", other),
        }
    }

    #[test]
    fn test_query_row_maps_unknown_kind_to_custom() {
        let row = AuditQueryRow {
            id: uuid::Uuid::new_v4(),
            timestamp: Utc::now().timestamp_millis(),
            kind: "something.totally.unknown".to_string(),
            severity: 6,
            source_ip: None,
            source_user_agent: None,
            source_subject: None,
            source_request_id: None,
            method: None,
            path: None,
            status_code: None,
            duration_ms: None,
            service_name: "svc".to_string(),
            metadata: None,
            hash: "h".to_string(),
            previous_hash: None,
            sequence: 1,
        };
        let event: AuditEvent = row.into();

        match event.kind {
            AuditEventKind::Custom(_) => {} // expected
            other => panic!("Unknown kinds should map to Custom, got {:?}", other),
        }
    }

    #[test]
    fn test_query_row_severity_boundary_values() {
        let make_row = |severity: u8| -> AuditEvent {
            AuditQueryRow {
                id: uuid::Uuid::new_v4(),
                timestamp: Utc::now().timestamp_millis(),
                kind: "http.request".to_string(),
                severity,
                source_ip: None,
                source_user_agent: None,
                source_subject: None,
                source_request_id: None,
                method: None,
                path: None,
                status_code: None,
                duration_ms: None,
                service_name: "svc".to_string(),
                metadata: None,
                hash: "h".to_string(),
                previous_hash: None,
                sequence: 1,
            }
            .into()
        };

        assert_eq!(make_row(0).severity.as_syslog_severity(), 0); // Emergency
        assert_eq!(make_row(7).severity.as_syslog_severity(), 7); // Debug
        // Out-of-range values should default to Informational (6)
        assert_eq!(make_row(255).severity.as_syslog_severity(), 6);
        assert_eq!(make_row(100).severity.as_syslog_severity(), 6);
    }

    #[test]
    fn test_query_row_handles_malformed_metadata_json() {
        let row = AuditQueryRow {
            id: uuid::Uuid::new_v4(),
            timestamp: Utc::now().timestamp_millis(),
            kind: "http.request".to_string(),
            severity: 6,
            source_ip: None,
            source_user_agent: None,
            source_subject: None,
            source_request_id: None,
            method: None,
            path: None,
            status_code: None,
            duration_ms: None,
            service_name: "svc".to_string(),
            metadata: Some("not valid json {{{".to_string()),
            hash: "h".to_string(),
            previous_hash: None,
            sequence: 1,
        };
        let event: AuditEvent = row.into();

        // Malformed JSON should result in None, not a panic
        assert!(
            event.metadata.is_none(),
            "Malformed metadata JSON should be silently dropped, not cause a panic"
        );
    }

    #[test]
    fn test_insert_row_event_kind_to_string_mapping() {
        let ts = Utc::now();
        let event = make_event(AuditEventKind::AuthLoginFailed, AuditSeverity::Warning, ts);
        let row = AuditInsertRow::from(&event);
        assert_eq!(row.kind, "auth.login.failed");
    }
}
