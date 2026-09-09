//! PostgreSQL audit storage backend
//!
//! Enforces immutability using `CREATE RULE` to silently discard UPDATE/DELETE operations.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::lazy::InitializableStorage;
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
        .map_err(|e| {
            Error::Internal(format!("Failed to create audit immutability rules: {}", e))
        })?;

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

    async fn query_before(
        &self,
        cutoff: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<AuditEvent>, Error> {
        let rows = sqlx::query_as::<_, AuditEventRow>(
            "SELECT * FROM audit_events WHERE timestamp < $1 ORDER BY sequence ASC LIMIT $2",
        )
        .bind(cutoff)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            Error::Internal(format!("Failed to query audit events before cutoff: {}", e))
        })?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn purge_before(&self, cutoff: DateTime<Utc>) -> Result<u64, Error> {
        // Temporarily drop the no-delete rule
        sqlx::query("DROP RULE IF EXISTS audit_no_delete ON audit_events")
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Internal(format!("Failed to drop audit_no_delete rule: {}", e)))?;

        // Perform the delete
        let result = sqlx::query("DELETE FROM audit_events WHERE timestamp < $1")
            .bind(cutoff)
            .execute(&self.pool)
            .await;

        // Reinstate the rule regardless of delete outcome
        let reinstate_result = sqlx::query(
            "CREATE RULE audit_no_delete AS ON DELETE TO audit_events DO INSTEAD NOTHING",
        )
        .execute(&self.pool)
        .await;

        if let Err(e) = reinstate_result {
            tracing::error!("CRITICAL: Failed to reinstate audit_no_delete rule: {}", e);
        }

        let rows =
            result.map_err(|e| Error::Internal(format!("Failed to purge audit events: {}", e)))?;

        Ok(rows.rows_affected())
    }

    async fn verify_chain(&self, from_sequence: u64) -> Result<Option<u64>, Error> {
        let query_from = i64::try_from(from_sequence.saturating_sub(1)).map_err(|_| {
            Error::Internal("Audit verification sequence exceeds the storage range".to_string())
        })?;
        let rows = sqlx::query_as::<_, AuditEventRow>(
            "SELECT * FROM audit_events WHERE sequence >= $1 ORDER BY sequence ASC",
        )
        .bind(query_from)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            Error::Internal(format!(
                "Failed to fetch audit events for verification: {}",
                e
            ))
        })?;

        let events: Vec<AuditEvent> = rows.into_iter().map(Into::into).collect();

        super::verify_stored_chain(&events, from_sequence)
    }
}

#[async_trait]
impl InitializableStorage for PgAuditStorage {
    type Conn = PgPool;

    fn from_conn(conn: Self::Conn) -> Self {
        Self::new(conn)
    }

    async fn init_schema(&self) -> Result<(), Error> {
        self.initialize().await
    }

    fn backend_name() -> &'static str {
        "PostgreSQL"
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
            "auth.token.missing" => AuditEventKind::AuthTokenMissing,
            "auth.token.invalid" => AuditEventKind::AuthTokenInvalid,
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
            #[cfg(feature = "login-lockout")]
            "auth.account.locked" => AuditEventKind::AuthAccountLocked,
            #[cfg(feature = "login-lockout")]
            "auth.account.unlocked" => AuditEventKind::AuthAccountUnlocked,
            #[cfg(feature = "accounts")]
            "account.created" => AuditEventKind::AccountCreated,
            #[cfg(feature = "accounts")]
            "account.disabled" => AuditEventKind::AccountDisabled,
            #[cfg(feature = "accounts")]
            "account.enabled" => AuditEventKind::AccountEnabled,
            #[cfg(feature = "accounts")]
            "account.locked" => AuditEventKind::AccountLocked,
            #[cfg(feature = "accounts")]
            "account.unlocked" => AuditEventKind::AccountUnlocked,
            #[cfg(feature = "accounts")]
            "account.expired" => AuditEventKind::AccountExpired,
            #[cfg(feature = "accounts")]
            "account.deleted" => AuditEventKind::AccountDeleted,
            #[cfg(feature = "accounts")]
            "account.updated" => AuditEventKind::AccountUpdated,
            "config.loaded" => AuditEventKind::ConfigLoaded,
            "config.drift_detected" => AuditEventKind::ConfigDriftDetected,
            "http.request" => AuditEventKind::HttpRequest,
            "http.request.denied" => AuditEventKind::HttpRequestDenied,
            other => AuditEventKind::Custom(super::parse_custom_kind(other)),
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

#[cfg(test)]
mod verification_tests {
    use super::*;
    use crate::audit::chain::AuditChain;
    use crate::audit::event::{AuditEventKind, AuditSeverity};

    #[tokio::test]
    #[ignore = "requires AUDIT_TEST_DATABASE_URL pointing to a dedicated PostgreSQL test database"]
    async fn verifies_postgres_suffix_and_retention_boundaries() {
        let url = std::env::var("AUDIT_TEST_DATABASE_URL").expect("dedicated test database URL");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect to test database");
        let schema = format!("audit_verify_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .unwrap();
        let storage = PgAuditStorage::new(pool.clone());
        storage.initialize().await.unwrap();
        assert!(storage.verify_chain(1).await.is_err());

        let mut chain = AuditChain::new("postgres-verification-test".to_string());
        let mut events = Vec::new();
        for offset in 0..5 {
            let mut event = AuditEvent::new(
                AuditEventKind::HttpRequest,
                AuditSeverity::Informational,
                "postgres-verification-test".to_string(),
            );
            event.timestamp = DateTime::from_timestamp(1_700_000_000 + offset, 0).unwrap();
            let event = chain.seal(event);
            storage.append(&event).await.unwrap();
            events.push(event);
        }
        for from in [0, 1, 2, 3, 5] {
            assert_eq!(storage.verify_chain(from).await.unwrap(), None);
        }
        assert!(storage.verify_chain(6).await.is_err());
        assert!(storage.verify_chain(u64::MAX).await.is_err());
        assert_eq!(storage.purge_before(events[2].timestamp).await.unwrap(), 2);
        assert!(storage.verify_chain(1).await.is_err());
        assert!(storage.verify_chain(3).await.is_err());
        assert_eq!(storage.verify_chain(4).await.unwrap(), None);

        // Simulate privileged tampering that bypasses the immutability rule.
        sqlx::query("DROP RULE audit_no_update ON audit_events")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE audit_events SET path = '/tampered' WHERE sequence = 4")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(storage.verify_chain(4).await.unwrap(), Some(4));
        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }
}
