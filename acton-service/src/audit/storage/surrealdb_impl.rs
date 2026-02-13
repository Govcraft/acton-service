//! SurrealDB audit storage backend
//!
//! Enforces immutability using `PERMISSIONS FOR update, delete NONE` on the audit_events table.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::AuditStorage;
use crate::audit::event::{AuditEvent, AuditEventKind, AuditSeverity, AuditSource};
use crate::error::Error;
use crate::surrealdb_backend::SurrealClient;

/// SurrealDB-backed audit storage
pub struct SurrealAuditStorage {
    client: Arc<SurrealClient>,
}

impl SurrealAuditStorage {
    /// Create a new SurrealDB audit storage
    pub fn new(client: Arc<SurrealClient>) -> Self {
        Self { client }
    }

    /// Initialize the audit_events table with immutability permissions
    pub async fn initialize(&self) -> Result<(), Error> {
        self.client
            .query(
                r#"
                DEFINE TABLE IF NOT EXISTS audit_events SCHEMAFUL
                    PERMISSIONS
                        FOR select FULL
                        FOR create FULL
                        FOR update NONE
                        FOR delete NONE;

                DEFINE FIELD IF NOT EXISTS id ON audit_events TYPE string;
                DEFINE FIELD IF NOT EXISTS timestamp ON audit_events TYPE string;
                DEFINE FIELD IF NOT EXISTS kind ON audit_events TYPE string;
                DEFINE FIELD IF NOT EXISTS severity ON audit_events TYPE int;
                DEFINE FIELD IF NOT EXISTS source_ip ON audit_events TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS source_user_agent ON audit_events TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS source_subject ON audit_events TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS source_request_id ON audit_events TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS method ON audit_events TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS path ON audit_events TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS status_code ON audit_events TYPE option<int>;
                DEFINE FIELD IF NOT EXISTS duration_ms ON audit_events TYPE option<int>;
                DEFINE FIELD IF NOT EXISTS service_name ON audit_events TYPE string;
                DEFINE FIELD IF NOT EXISTS metadata ON audit_events TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS hash ON audit_events TYPE string;
                DEFINE FIELD IF NOT EXISTS previous_hash ON audit_events TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS sequence ON audit_events TYPE int;

                DEFINE INDEX IF NOT EXISTS idx_audit_sequence ON audit_events FIELDS sequence UNIQUE;
                DEFINE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_events FIELDS timestamp;
                "#,
            )
            .await
            .map_err(|e| Error::Internal(format!("Failed to initialize audit schema: {}", e)))?;

        Ok(())
    }
}

/// Serializable record for SurrealDB insert
#[derive(Serialize)]
struct AuditRecord {
    id: String,
    timestamp: String,
    kind: String,
    severity: i64,
    source_ip: Option<String>,
    source_user_agent: Option<String>,
    source_subject: Option<String>,
    source_request_id: Option<String>,
    method: Option<String>,
    path: Option<String>,
    status_code: Option<i64>,
    duration_ms: Option<i64>,
    service_name: String,
    metadata: Option<String>,
    hash: String,
    previous_hash: Option<String>,
    sequence: i64,
}

/// Deserializable record from SurrealDB queries
#[derive(Deserialize)]
struct AuditRow {
    id: serde_json::Value,
    timestamp: String,
    kind: String,
    severity: i64,
    source_ip: Option<String>,
    source_user_agent: Option<String>,
    source_subject: Option<String>,
    source_request_id: Option<String>,
    method: Option<String>,
    path: Option<String>,
    status_code: Option<i64>,
    duration_ms: Option<i64>,
    service_name: String,
    metadata: Option<String>,
    hash: String,
    previous_hash: Option<String>,
    sequence: i64,
}

impl From<AuditRow> for AuditEvent {
    fn from(row: AuditRow) -> Self {
        // Extract the UUID from the SurrealDB record ID
        let id_str = match &row.id {
            serde_json::Value::String(s) => {
                // Handle "audit_events:uuid" format
                s.split(':').last().unwrap_or(s).to_string()
            }
            serde_json::Value::Object(obj) => {
                // Handle { "tb": "audit_events", "id": { "String": "uuid" } } format
                obj.get("id")
                    .and_then(|v| match v {
                        serde_json::Value::String(s) => Some(s.clone()),
                        serde_json::Value::Object(inner) => inner
                            .get("String")
                            .and_then(|s| s.as_str().map(String::from)),
                        _ => None,
                    })
                    .unwrap_or_default()
            }
            _ => String::new(),
        };
        let id = uuid::Uuid::parse_str(&id_str).unwrap_or_else(|_| uuid::Uuid::new_v4());

        let timestamp = DateTime::parse_from_rfc3339(&row.timestamp)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        let kind = parse_event_kind(&row.kind);
        let severity = parse_severity(row.severity as i16);

        AuditEvent {
            id,
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
            status_code: row.status_code.map(|c| c as u16),
            duration_ms: row.duration_ms.map(|d| d as u64),
            service_name: row.service_name,
            metadata: row.metadata.and_then(|s| serde_json::from_str(&s).ok()),
            hash: Some(row.hash),
            previous_hash: row.previous_hash,
            sequence: row.sequence as u64,
        }
    }
}

#[async_trait]
impl AuditStorage for SurrealAuditStorage {
    async fn append(&self, event: &AuditEvent) -> Result<(), Error> {
        let record = AuditRecord {
            id: event.id.to_string(),
            timestamp: event.timestamp.to_rfc3339(),
            kind: event.kind.to_string(),
            severity: event.severity.as_syslog_severity() as i64,
            source_ip: event.source.ip.clone(),
            source_user_agent: event.source.user_agent.clone(),
            source_subject: event.source.subject.clone(),
            source_request_id: event.source.request_id.clone(),
            method: event.method.clone(),
            path: event.path.clone(),
            status_code: event.status_code.map(|c| c as i64),
            duration_ms: event.duration_ms.map(|d| d as i64),
            service_name: event.service_name.clone(),
            metadata: event
                .metadata
                .as_ref()
                .map(|m| serde_json::to_string(m).unwrap_or_default()),
            hash: event.hash.clone().unwrap_or_default(),
            previous_hash: event.previous_hash.clone(),
            sequence: event.sequence as i64,
        };

        // Use owned String for the record ID to satisfy .bind() requirements
        let record_id = event.id.to_string();

        self.client
            .query("CREATE type::thing('audit_events', $id) CONTENT $data")
            .bind(("id", record_id))
            .bind(("data", record))
            .await
            .map_err(|e| Error::Internal(format!("Failed to append audit event: {}", e)))?;

        Ok(())
    }

    async fn latest(&self) -> Result<Option<AuditEvent>, Error> {
        let mut result = self
            .client
            .query("SELECT * FROM audit_events ORDER BY sequence DESC LIMIT 1")
            .await
            .map_err(|e| Error::Internal(format!("Failed to query latest audit event: {}", e)))?;

        let rows: Vec<AuditRow> = result
            .take(0)
            .map_err(|e| Error::Internal(format!("Failed to deserialize audit event: {}", e)))?;

        Ok(rows.into_iter().next().map(Into::into))
    }

    async fn query_range(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<AuditEvent>, Error> {
        let from_str = from.to_rfc3339();
        let to_str = to.to_rfc3339();

        let mut result = self
            .client
            .query("SELECT * FROM audit_events WHERE timestamp >= $from AND timestamp <= $to ORDER BY sequence ASC LIMIT $limit")
            .bind(("from", from_str))
            .bind(("to", to_str))
            .bind(("limit", limit as i64))
            .await
            .map_err(|e| Error::Internal(format!("Failed to query audit events: {}", e)))?;

        let rows: Vec<AuditRow> = result
            .take(0)
            .map_err(|e| Error::Internal(format!("Failed to deserialize audit events: {}", e)))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn verify_chain(&self, from_sequence: u64) -> Result<Option<u64>, Error> {
        let mut result = self
            .client
            .query("SELECT * FROM audit_events WHERE sequence >= $seq ORDER BY sequence ASC")
            .bind(("seq", from_sequence as i64))
            .await
            .map_err(|e| {
                Error::Internal(format!("Failed to fetch events for verification: {}", e))
            })?;

        let rows: Vec<AuditRow> = result
            .take(0)
            .map_err(|e| Error::Internal(format!("Failed to deserialize audit events: {}", e)))?;

        let events: Vec<AuditEvent> = rows.into_iter().map(Into::into).collect();

        match crate::audit::chain::verify_chain(&events) {
            Ok(()) => Ok(None),
            Err(e) => Ok(Some(e.sequence)),
        }
    }
}

fn parse_event_kind(s: &str) -> AuditEventKind {
    match s {
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
        "http.request" => AuditEventKind::HttpRequest,
        "http.request.denied" => AuditEventKind::HttpRequestDenied,
        other => {
            let name = other.strip_prefix("custom.").unwrap_or(other);
            AuditEventKind::Custom(name.to_string())
        }
    }
}

fn parse_severity(val: i16) -> AuditSeverity {
    match val {
        0 => AuditSeverity::Emergency,
        1 => AuditSeverity::Alert,
        2 => AuditSeverity::Critical,
        3 => AuditSeverity::Error,
        4 => AuditSeverity::Warning,
        5 => AuditSeverity::Notice,
        7 => AuditSeverity::Debug,
        _ => AuditSeverity::Informational,
    }
}
