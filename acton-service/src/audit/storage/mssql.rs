//! Append-only Microsoft SQL Server audit storage.
use super::lazy::InitializableStorage;
use super::AuditStorage;
use crate::{
    audit::event::{AuditEvent, AuditEventKind, AuditSeverity, AuditSource},
    error::Error,
    mssql::{execute, query, MssqlPool},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// SQL Server-backed append-only audit storage.
pub struct MssqlAuditStorage {
    pool: MssqlPool,
}
impl MssqlAuditStorage {
    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }
    pub async fn initialize(&self) -> Result<(), Error> {
        execute(&self.pool,"IF OBJECT_ID(N'audit_events',N'U') IS NULL CREATE TABLE audit_events(id UNIQUEIDENTIFIER PRIMARY KEY,[timestamp] DATETIMEOFFSET NOT NULL,kind NVARCHAR(255) NOT NULL,severity SMALLINT NOT NULL,source_ip NVARCHAR(255) NULL,source_user_agent NVARCHAR(MAX) NULL,source_subject NVARCHAR(255) NULL,source_request_id NVARCHAR(255) NULL,method NVARCHAR(32) NULL,path NVARCHAR(MAX) NULL,status_code SMALLINT NULL,duration_ms BIGINT NULL,service_name NVARCHAR(255) NOT NULL,metadata NVARCHAR(MAX) NULL,[hash] NVARCHAR(255) NOT NULL,previous_hash NVARCHAR(255) NULL,sequence BIGINT NOT NULL UNIQUE)",&[]).await?;
        execute(&self.pool,"IF OBJECT_ID(N'audit_no_update',N'TR') IS NULL EXEC('CREATE TRIGGER audit_no_update ON audit_events INSTEAD OF UPDATE AS THROW 51000, ''audit events are immutable'', 1')",&[]).await?;
        execute(&self.pool,"IF OBJECT_ID(N'audit_no_delete',N'TR') IS NULL EXEC('CREATE TRIGGER audit_no_delete ON audit_events INSTEAD OF DELETE AS THROW 51000, ''audit events are immutable'', 1')",&[]).await?;
        Ok(())
    }
}
fn req(row: &tiberius::Row, name: &str) -> Result<String, Error> {
    row.get::<&str, _>(name)
        .map(str::to_owned)
        .ok_or_else(|| Error::Internal(format!("missing {name}")))
}
fn kind(value: &str) -> AuditEventKind {
    match value {
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
        "config.loaded" => AuditEventKind::ConfigLoaded,
        "config.drift_detected" => AuditEventKind::ConfigDriftDetected,
        "http.request" => AuditEventKind::HttpRequest,
        "http.request.denied" => AuditEventKind::HttpRequestDenied,
        other => AuditEventKind::Custom(super::parse_custom_kind(other)),
    }
}
fn decode(row: &tiberius::Row) -> Result<AuditEvent, Error> {
    let severity = match row.get::<i16, _>("severity").unwrap_or(6) {
        0 => AuditSeverity::Emergency,
        1 => AuditSeverity::Alert,
        2 => AuditSeverity::Critical,
        3 => AuditSeverity::Error,
        4 => AuditSeverity::Warning,
        5 => AuditSeverity::Notice,
        7 => AuditSeverity::Debug,
        _ => AuditSeverity::Informational,
    };
    Ok(AuditEvent {
        id: row
            .get("id")
            .ok_or_else(|| Error::Internal("missing id".to_string()))?,
        timestamp: row
            .get("timestamp")
            .ok_or_else(|| Error::Internal("missing timestamp".to_string()))?,
        kind: kind(&req(row, "kind")?),
        severity,
        source: AuditSource {
            ip: row.get::<&str, _>("source_ip").map(str::to_owned),
            user_agent: row.get::<&str, _>("source_user_agent").map(str::to_owned),
            subject: row.get::<&str, _>("source_subject").map(str::to_owned),
            request_id: row.get::<&str, _>("source_request_id").map(str::to_owned),
        },
        method: row.get::<&str, _>("method").map(str::to_owned),
        path: row.get::<&str, _>("path").map(str::to_owned),
        status_code: row.get::<i16, _>("status_code").map(|v| v as u16),
        duration_ms: row.get::<i64, _>("duration_ms").map(|v| v as u64),
        service_name: req(row, "service_name")?,
        metadata: row
            .get::<&str, _>("metadata")
            .and_then(|v| serde_json::from_str(v).ok()),
        hash: row.get::<&str, _>("hash").map(str::to_owned),
        previous_hash: row.get::<&str, _>("previous_hash").map(str::to_owned),
        sequence: row.get::<i64, _>("sequence").unwrap_or(0) as u64,
    })
}
#[async_trait]
impl AuditStorage for MssqlAuditStorage {
    async fn append(&self, e: &AuditEvent) -> Result<(), Error> {
        let id = e.id;
        let kind = e.kind.to_string();
        let severity = e.severity.as_syslog_severity() as i16;
        let status = e.status_code.map(|v| v as i16);
        let duration = e.duration_ms.map(|v| v as i64);
        let metadata = e
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|x| Error::Internal(x.to_string()))?;
        let sequence = e.sequence as i64;
        execute(&self.pool,"INSERT INTO audit_events(id,[timestamp],kind,severity,source_ip,source_user_agent,source_subject,source_request_id,method,path,status_code,duration_ms,service_name,metadata,[hash],previous_hash,sequence) VALUES(@P1,@P2,@P3,@P4,@P5,@P6,@P7,@P8,@P9,@P10,@P11,@P12,@P13,@P14,@P15,@P16,@P17)",&[&id,&e.timestamp,&kind,&severity,&e.source.ip,&e.source.user_agent,&e.source.subject,&e.source.request_id,&e.method,&e.path,&status,&duration,&e.service_name,&metadata,&e.hash,&e.previous_hash,&sequence]).await?;
        Ok(())
    }
    async fn latest(&self) -> Result<Option<AuditEvent>, Error> {
        query(
            &self.pool,
            "SELECT TOP (1) * FROM audit_events ORDER BY sequence DESC",
            &[],
        )
        .await?
        .first()
        .map(decode)
        .transpose()
    }
    async fn query_range(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<AuditEvent>, Error> {
        let limit = limit as i64;
        query(&self.pool,"SELECT TOP (@P3) * FROM audit_events WHERE [timestamp]>=@P1 AND [timestamp]<=@P2 ORDER BY sequence",&[&from,&to,&limit]).await?.iter().map(decode).collect()
    }
    async fn query_before(
        &self,
        cutoff: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<AuditEvent>, Error> {
        let limit = limit as i64;
        query(
            &self.pool,
            "SELECT TOP (@P2) * FROM audit_events WHERE [timestamp]<@P1 ORDER BY sequence",
            &[&cutoff, &limit],
        )
        .await?
        .iter()
        .map(decode)
        .collect()
    }
    async fn purge_before(&self, cutoff: DateTime<Utc>) -> Result<u64, Error> {
        execute(&self.pool,"SET XACT_ABORT ON; BEGIN TRANSACTION; DISABLE TRIGGER audit_no_delete ON audit_events; DELETE FROM audit_events WHERE [timestamp]<@P1; ENABLE TRIGGER audit_no_delete ON audit_events; COMMIT TRANSACTION",&[&cutoff]).await
    }
    async fn verify_chain(&self, from: u64) -> Result<Option<u64>, Error> {
        let from = from as i64;
        let events: Vec<_> = query(
            &self.pool,
            "SELECT * FROM audit_events WHERE sequence>=@P1 ORDER BY sequence",
            &[&from],
        )
        .await?
        .iter()
        .map(decode)
        .collect::<Result<_, _>>()?;
        Ok(crate::audit::chain::verify_chain(&events)
            .err()
            .map(|e| e.sequence))
    }
}

#[async_trait]
impl InitializableStorage for MssqlAuditStorage {
    type Conn = MssqlPool;
    fn from_conn(conn: Self::Conn) -> Self {
        Self::new(conn)
    }
    async fn init_schema(&self) -> Result<(), Error> {
        self.initialize().await
    }
    fn backend_name() -> &'static str {
        "Microsoft SQL Server"
    }
}
