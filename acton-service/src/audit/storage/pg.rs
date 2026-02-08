//! PostgreSQL audit storage backend
//!
//! Enforces immutability using `CREATE RULE` to silently discard UPDATE/DELETE operations.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::AuditStorage;
use crate::audit::event::AuditEvent;
use crate::error::Error;

/// PostgreSQL-backed audit storage
pub struct PgAuditStorage {
    pool: PgPool,
}

impl PgAuditStorage {
    /// Create a new PostgreSQL audit storage
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Initialize the audit_events table and immutability rules
    ///
    /// Should be called once during application startup.
    pub async fn initialize(&self) -> Result<(), Error> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS audit_events (
                id UUID PRIMARY KEY,
                timestamp TIMESTAMPTZ NOT NULL,
                kind TEXT NOT NULL,
                severity SMALLINT NOT NULL,
                source_ip TEXT,
                source_user_agent TEXT,
                source_subject TEXT,
                source_request_id TEXT,
                method TEXT,
                path TEXT,
                status_code SMALLINT,
                duration_ms BIGINT,
                service_name TEXT NOT NULL,
                metadata JSONB,
                hash TEXT NOT NULL,
                previous_hash TEXT,
                sequence BIGINT NOT NULL UNIQUE
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Internal(format!("Failed to create audit_events table: {}", e)))?;

        // Create index on sequence for chain verification queries
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_audit_events_sequence ON audit_events (sequence)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Internal(format!("Failed to create audit index: {}", e)))?;

        // Create index on timestamp for range queries
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_audit_events_timestamp ON audit_events (timestamp)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Internal(format!("Failed to create audit timestamp index: {}", e)))?;

        // Enforce immutability: silently discard UPDATE/DELETE
        sqlx::query(
            r#"
            DO $$
            BEGIN
                IF NOT EXISTS (
                    SELECT 1 FROM pg_rules
                    WHERE rulename = 'audit_no_update' AND tablename = 'audit_events'
                ) THEN
                    CREATE RULE audit_no_update AS ON UPDATE TO audit_events DO INSTEAD NOTHING;
                END IF;

                IF NOT EXISTS (
                    SELECT 1 FROM pg_rules
                    WHERE rulename = 'audit_no_delete' AND tablename = 'audit_events'
                ) THEN
                    CREATE RULE audit_no_delete AS ON DELETE TO audit_events DO INSTEAD NOTHING;
                END IF;
            END
            $$;
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Internal(format!("Failed to create audit immutability rules: {}", e)))?;

        Ok(())
    }
}

#[async_trait]
impl AuditStorage for PgAuditStorage {
    async fn append(&self, event: &AuditEvent) -> Result<(), Error> {
        sqlx::query(
            r#"
            INSERT INTO audit_events (
                id, timestamp, kind, severity,
                source_ip, source_user_agent, source_subject, source_request_id,
                method, path, status_code, duration_ms,
                service_name, metadata, hash, previous_hash, sequence
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
            "#,
        )
        .bind(event.id)
        .bind(event.timestamp)
        .bind(event.kind.to_string())
        .bind(event.severity.as_syslog_severity() as i16)
        .bind(&event.source.ip)
        .bind(&event.source.user_agent)
        .bind(&event.source.subject)
        .bind(&event.source.request_id)
        .bind(&event.method)
        .bind(&event.path)
        .bind(event.status_code.map(|c| c as i16))
        .bind(event.duration_ms.map(|d| d as i64))
        .bind(&event.service_name)
        .bind(&event.metadata)
        .bind(&event.hash)
        .bind(&event.previous_hash)
        .bind(event.sequence as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Internal(format!("Failed to append audit event: {}", e)))?;

        Ok(())
    }

    async fn latest(&self) -> Result<Option<AuditEvent>, Error> {
        let row = sqlx::query_as::<_, AuditEventRow>(
            "SELECT * FROM audit_events ORDER BY sequence DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Internal(format!("Failed to fetch latest audit event: {}", e)))?;

        Ok(row.map(Into::into))
    }

    async fn query_range(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<AuditEvent>, Error> {
        let rows = sqlx::query_as::<_, AuditEventRow>(
            "SELECT * FROM audit_events WHERE timestamp >= $1 AND timestamp <= $2 ORDER BY sequence ASC LIMIT $3",
        )
        .bind(from)
        .bind(to)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Internal(format!("Failed to query audit events: {}", e)))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn verify_chain(&self, from_sequence: u64) -> Result<Option<u64>, Error> {
        let rows = sqlx::query_as::<_, AuditEventRow>(
            "SELECT * FROM audit_events WHERE sequence >= $1 ORDER BY sequence ASC",
        )
        .bind(from_sequence as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Internal(format!("Failed to fetch audit events for verification: {}", e)))?;

        let events: Vec<AuditEvent> = rows.into_iter().map(Into::into).collect();

        match crate::audit::chain::verify_chain(&events) {
            Ok(()) => Ok(None),
            Err(e) => Ok(Some(e.sequence)),
        }
    }
}

/// Internal row type for sqlx mapping
#[derive(sqlx::FromRow)]
struct AuditEventRow {
    id: uuid::Uuid,
    timestamp: DateTime<Utc>,
    kind: String,
    severity: i16,
    source_ip: Option<String>,
    source_user_agent: Option<String>,
    source_subject: Option<String>,
    source_request_id: Option<String>,
    method: Option<String>,
    path: Option<String>,
    status_code: Option<i16>,
    duration_ms: Option<i64>,
    service_name: String,
    metadata: Option<serde_json::Value>,
    hash: Option<String>,
    previous_hash: Option<String>,
    sequence: i64,
}

impl From<AuditEventRow> for AuditEvent {
    fn from(row: AuditEventRow) -> Self {
        use crate::audit::event::{AuditEventKind, AuditSeverity, AuditSource};

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

        AuditEvent {
            id: row.id,
            timestamp: row.timestamp,
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
            metadata: row.metadata,
            hash: row.hash,
            previous_hash: row.previous_hash,
            sequence: row.sequence as u64,
        }
    }
}
