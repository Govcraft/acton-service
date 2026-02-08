//! Turso/libsql audit storage backend
//!
//! Enforces immutability using triggers that RAISE(ABORT) on UPDATE/DELETE.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;

use super::AuditStorage;
use crate::audit::event::{AuditEvent, AuditEventKind, AuditSeverity, AuditSource};
use crate::error::Error;

/// Turso-backed audit storage
pub struct TursoAuditStorage {
    db: Arc<libsql::Database>,
}

impl TursoAuditStorage {
    /// Create a new Turso audit storage
    pub fn new(db: Arc<libsql::Database>) -> Self {
        Self { db }
    }

    /// Initialize the audit_events table and immutability triggers
    pub async fn initialize(&self) -> Result<(), Error> {
        let conn = self
            .db
            .connect()
            .map_err(|e| Error::Internal(format!("Failed to connect for audit init: {}", e)))?;

        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS audit_events (
                id TEXT PRIMARY KEY,
                timestamp TEXT NOT NULL,
                kind TEXT NOT NULL,
                severity INTEGER NOT NULL,
                source_ip TEXT,
                source_user_agent TEXT,
                source_subject TEXT,
                source_request_id TEXT,
                method TEXT,
                path TEXT,
                status_code INTEGER,
                duration_ms INTEGER,
                service_name TEXT NOT NULL,
                metadata TEXT,
                hash TEXT NOT NULL,
                previous_hash TEXT,
                sequence INTEGER NOT NULL UNIQUE
            )
            "#,
            (),
        )
        .await
        .map_err(|e| Error::Internal(format!("Failed to create audit_events table: {}", e)))?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_audit_events_sequence ON audit_events (sequence)",
            (),
        )
        .await
        .map_err(|e| Error::Internal(format!("Failed to create audit index: {}", e)))?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_audit_events_timestamp ON audit_events (timestamp)",
            (),
        )
        .await
        .map_err(|e| Error::Internal(format!("Failed to create audit timestamp index: {}", e)))?;

        // Immutability triggers
        conn.execute(
            r#"
            CREATE TRIGGER IF NOT EXISTS audit_no_update
            BEFORE UPDATE ON audit_events
            BEGIN
                SELECT RAISE(ABORT, 'audit events are immutable');
            END
            "#,
            (),
        )
        .await
        .map_err(|e| Error::Internal(format!("Failed to create update trigger: {}", e)))?;

        conn.execute(
            r#"
            CREATE TRIGGER IF NOT EXISTS audit_no_delete
            BEFORE DELETE ON audit_events
            BEGIN
                SELECT RAISE(ABORT, 'audit events are immutable');
            END
            "#,
            (),
        )
        .await
        .map_err(|e| Error::Internal(format!("Failed to create delete trigger: {}", e)))?;

        Ok(())
    }
}

#[async_trait]
impl AuditStorage for TursoAuditStorage {
    async fn append(&self, event: &AuditEvent) -> Result<(), Error> {
        let conn = self
            .db
            .connect()
            .map_err(|e| Error::Internal(format!("Failed to connect for audit append: {}", e)))?;

        let metadata_str = event
            .metadata
            .as_ref()
            .map(|m| serde_json::to_string(m).unwrap_or_default());

        conn.execute(
            r#"
            INSERT INTO audit_events (
                id, timestamp, kind, severity,
                source_ip, source_user_agent, source_subject, source_request_id,
                method, path, status_code, duration_ms,
                service_name, metadata, hash, previous_hash, sequence
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
            "#,
            libsql::params![
                event.id.to_string(),
                event.timestamp.to_rfc3339(),
                event.kind.to_string(),
                event.severity.as_syslog_severity() as i64,
                event.source.ip.clone(),
                event.source.user_agent.clone(),
                event.source.subject.clone(),
                event.source.request_id.clone(),
                event.method.clone(),
                event.path.clone(),
                event.status_code.map(|c| c as i64),
                event.duration_ms.map(|d| d as i64),
                event.service_name.clone(),
                metadata_str,
                event.hash.clone(),
                event.previous_hash.clone(),
                event.sequence as i64,
            ],
        )
        .await
        .map_err(|e| Error::Internal(format!("Failed to append audit event: {}", e)))?;

        Ok(())
    }

    async fn latest(&self) -> Result<Option<AuditEvent>, Error> {
        let conn = self
            .db
            .connect()
            .map_err(|e| Error::Internal(format!("Failed to connect for audit query: {}", e)))?;

        let mut rows = conn
            .query(
                "SELECT * FROM audit_events ORDER BY sequence DESC LIMIT 1",
                (),
            )
            .await
            .map_err(|e| Error::Internal(format!("Failed to query latest audit event: {}", e)))?;

        match rows.next().await {
            Ok(Some(row)) => Ok(Some(row_to_event(&row)?)),
            Ok(None) => Ok(None),
            Err(e) => Err(Error::Internal(format!(
                "Failed to read audit event row: {}",
                e
            ))),
        }
    }

    async fn query_range(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<AuditEvent>, Error> {
        let conn = self
            .db
            .connect()
            .map_err(|e| Error::Internal(format!("Failed to connect for audit query: {}", e)))?;

        let mut rows = conn
            .query(
                "SELECT * FROM audit_events WHERE timestamp >= ?1 AND timestamp <= ?2 ORDER BY sequence ASC LIMIT ?3",
                libsql::params![from.to_rfc3339(), to.to_rfc3339(), limit as i64],
            )
            .await
            .map_err(|e| Error::Internal(format!("Failed to query audit events: {}", e)))?;

        let mut events = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            events.push(row_to_event(&row)?);
        }
        Ok(events)
    }

    async fn verify_chain(&self, from_sequence: u64) -> Result<Option<u64>, Error> {
        let conn = self
            .db
            .connect()
            .map_err(|e| Error::Internal(format!("Failed to connect for chain verify: {}", e)))?;

        let mut rows = conn
            .query(
                "SELECT * FROM audit_events WHERE sequence >= ?1 ORDER BY sequence ASC",
                libsql::params![from_sequence as i64],
            )
            .await
            .map_err(|e| Error::Internal(format!("Failed to fetch events for verification: {}", e)))?;

        let mut events = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            events.push(row_to_event(&row)?);
        }

        match crate::audit::chain::verify_chain(&events) {
            Ok(()) => Ok(None),
            Err(e) => Ok(Some(e.sequence)),
        }
    }
}

fn row_to_event(row: &libsql::Row) -> Result<AuditEvent, Error> {
    let id_str: String = row
        .get(0)
        .map_err(|e| Error::Internal(format!("Failed to read id: {}", e)))?;
    let id = uuid::Uuid::parse_str(&id_str)
        .map_err(|e| Error::Internal(format!("Failed to parse UUID: {}", e)))?;

    let timestamp_str: String = row
        .get(1)
        .map_err(|e| Error::Internal(format!("Failed to read timestamp: {}", e)))?;
    let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| Error::Internal(format!("Failed to parse timestamp: {}", e)))?;

    let kind_str: String = row
        .get(2)
        .map_err(|e| Error::Internal(format!("Failed to read kind: {}", e)))?;
    let kind = parse_event_kind(&kind_str);

    let severity_val: i64 = row
        .get(3)
        .map_err(|e| Error::Internal(format!("Failed to read severity: {}", e)))?;
    let severity = parse_severity(severity_val as i16);

    let sequence: i64 = row
        .get(16)
        .map_err(|e| Error::Internal(format!("Failed to read sequence: {}", e)))?;

    Ok(AuditEvent {
        id,
        timestamp,
        kind,
        severity,
        source: AuditSource {
            ip: row.get(4).ok(),
            user_agent: row.get(5).ok(),
            subject: row.get(6).ok(),
            request_id: row.get(7).ok(),
        },
        method: row.get(8).ok(),
        path: row.get(9).ok(),
        status_code: row.get::<i64>(10).ok().map(|c| c as u16),
        duration_ms: row.get::<i64>(11).ok().map(|d| d as u64),
        service_name: row
            .get(12)
            .map_err(|e| Error::Internal(format!("Failed to read service_name: {}", e)))?,
        metadata: row
            .get::<String>(13)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok()),
        hash: row.get(14).ok(),
        previous_hash: row.get(15).ok(),
        sequence: sequence as u64,
    })
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
