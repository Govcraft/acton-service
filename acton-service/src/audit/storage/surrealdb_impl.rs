//! SurrealDB audit storage backend
//!
//! Enforces immutability using `PERMISSIONS FOR update, delete NONE` on the audit_events table.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use surrealdb::types::{RecordId, RecordIdKey, SurrealValue};

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
            .and_then(|response| response.check())
            .map_err(|e| Error::Internal(format!("Failed to initialize audit schema: {}", e)))?;

        Ok(())
    }
}

/// Serializable record for SurrealDB insert
#[derive(Serialize, SurrealValue)]
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
#[derive(Deserialize, SurrealValue)]
struct AuditRow {
    id: RecordId,
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

impl TryFrom<AuditRow> for AuditEvent {
    type Error = Error;

    fn try_from(row: AuditRow) -> Result<Self, Error> {
        let RecordIdKey::String(id_str) = row.id.key else {
            return Err(Error::Internal(
                "Audit record ID must contain a UUID string key".into(),
            ));
        };
        let id = uuid::Uuid::parse_str(&id_str)
            .map_err(|e| Error::Internal(format!("Invalid stored audit UUID: {e}")))?;
        let timestamp = DateTime::parse_from_rfc3339(&row.timestamp)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| Error::Internal(format!("Invalid stored audit timestamp: {e}")))?;
        let metadata = row
            .metadata
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(|e| Error::Internal(format!("Invalid stored audit metadata: {e}")))?;

        let kind = parse_event_kind(&row.kind);
        let severity = parse_severity(row.severity as i16);

        Ok(AuditEvent {
            id: id.into(),
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
            metadata,
            hash: Some(row.hash),
            previous_hash: row.previous_hash,
            sequence: row.sequence as u64,
        })
    }
}

#[async_trait]
impl AuditStorage for SurrealAuditStorage {
    async fn append(&self, event: &AuditEvent) -> Result<(), Error> {
        let record = AuditRecord {
            id: event.id.as_uuid().to_string(),
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
        let record_id = event.id.as_uuid().to_string();

        self.client
            .query("CREATE type::record('audit_events', $id) CONTENT $data")
            .bind(("id", record_id))
            .bind(("data", record))
            .await
            .and_then(|response| response.check())
            .map_err(|e| Error::Internal(format!("Failed to append audit event: {}", e)))?;

        Ok(())
    }

    async fn latest(&self) -> Result<Option<AuditEvent>, Error> {
        let mut result = self
            .client
            .query("SELECT * FROM audit_events ORDER BY sequence DESC LIMIT 1")
            .await
            .and_then(|response| response.check())
            .map_err(|e| Error::Internal(format!("Failed to query latest audit event: {}", e)))?;

        let rows: Vec<AuditRow> = result
            .take(0)
            .map_err(|e| Error::Internal(format!("Failed to deserialize audit event: {}", e)))?;

        rows.into_iter().next().map(TryInto::try_into).transpose()
    }

    async fn query_filtered(&self, q: &super::AuditQuery) -> Result<Vec<AuditEvent>, Error> {
        q.validate()?;
        let (comparison, direction) = match q.order {
            super::AuditOrder::NewestFirst => ("<", "DESC"),
            super::AuditOrder::OldestFirst => (">", "ASC"),
        };
        let mut cursor = q.cursor.map(|v| v as i64);
        let mut matches = Vec::new();
        // Immutable legacy metadata is JSON text. Decode only bounded candidates,
        // never alter historical records or silently return an incomplete page.
        for _ in 0..10 {
            let statement = format!(
                r#"SELECT * FROM audit_events WHERE ($cursor = NONE OR sequence {comparison} $cursor)
 AND ($ceiling = NONE OR sequence <= $ceiling)
 AND ($from = NONE OR <datetime>timestamp >= <datetime>$from)
 AND ($to = NONE OR <datetime>timestamp <= <datetime>$to)
 AND ($kind = NONE OR kind = $kind)
 AND ($severity = NONE OR severity = $severity)
 AND ($subject = NONE OR source_subject = $subject)
 AND ($request = NONE OR source_request_id = $request)
 AND ($service = NONE OR service_name = $service)
 ORDER BY sequence {direction} LIMIT 1000"#
            );
            let mut result = self
                .client
                .query(statement)
                .bind(("cursor", cursor))
                .bind(("ceiling", q.through_sequence.map(|v| v as i64)))
                .bind(("from", q.from.map(|v| v.to_rfc3339())))
                .bind(("to", q.to.map(|v| v.to_rfc3339())))
                .bind(("kind", q.kind.as_ref().map(ToString::to_string)))
                .bind((
                    "severity",
                    q.severity.map(|v| i64::from(v.as_syslog_severity())),
                ))
                .bind(("subject", q.subject.clone()))
                .bind(("request", q.request_id.clone()))
                .bind(("service", q.service_name.clone()))
                .await
                .and_then(|r| r.check())
                .map_err(|e| Error::Internal(format!("Failed to query audit events: {e}")))?;
            let rows: Vec<AuditRow> = result
                .take(0)
                .map_err(|e| Error::Internal(format!("Failed to decode audit events: {e}")))?;
            let exhausted = rows.len() < 1000;
            for row in rows {
                let event: AuditEvent = row.try_into()?;
                cursor = Some(event.sequence as i64);
                if q.matches(&event) {
                    matches.push(event);
                    if matches.len() == q.limit {
                        return Ok(matches);
                    }
                }
            }
            if exhausted {
                return Ok(matches);
            }
        }
        Err(Error::Internal(
            "Audit metadata query exceeded 10000 candidates; narrow the time or event filters"
                .into(),
        ))
    }

    async fn query_sequence(
        &self,
        from: u64,
        to: u64,
        limit: usize,
    ) -> Result<Vec<AuditEvent>, Error> {
        let from = i64::try_from(from)
            .map_err(|_| Error::Internal("Audit sequence exceeds storage range".into()))?;
        let to = i64::try_from(to)
            .map_err(|_| Error::Internal("Audit sequence exceeds storage range".into()))?;
        let limit = i64::try_from(limit)
            .map_err(|_| Error::Internal("Audit query limit exceeds storage range".into()))?;

        let mut result = self
            .client
            .query("SELECT * FROM audit_events WHERE sequence >= $from AND sequence <= $to ORDER BY sequence ASC LIMIT $limit")
            .bind(("from", from))
            .bind(("to", to))
            .bind(("limit", limit))
            .await
            .and_then(|response| response.check())
            .map_err(|e| Error::Internal(format!("Failed to query audit events: {}", e)))?;

        let rows: Vec<AuditRow> = result
            .take(0)
            .map_err(|e| Error::Internal(format!("Failed to deserialize audit events: {}", e)))?;

        rows.into_iter().map(TryInto::try_into).collect()
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
            .and_then(|response| response.check())
            .map_err(|e| Error::Internal(format!("Failed to query audit events: {}", e)))?;

        let rows: Vec<AuditRow> = result
            .take(0)
            .map_err(|e| Error::Internal(format!("Failed to deserialize audit events: {}", e)))?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn query_before(
        &self,
        cutoff: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<AuditEvent>, Error> {
        let cutoff_str = cutoff.to_rfc3339();

        let mut result = self
            .client
            .query("SELECT * FROM audit_events WHERE timestamp < $cutoff ORDER BY sequence ASC LIMIT $limit")
            .bind(("cutoff", cutoff_str))
            .bind(("limit", limit as i64))
            .await
            .and_then(|response| response.check())
            .map_err(|e| {
                Error::Internal(format!("Failed to query audit events before cutoff: {}", e))
            })?;

        let rows: Vec<AuditRow> = result
            .take(0)
            .map_err(|e| Error::Internal(format!("Failed to deserialize audit events: {}", e)))?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn purge_before(&self, cutoff: DateTime<Utc>) -> Result<u64, Error> {
        let cutoff_str = cutoff.to_rfc3339();

        // Count events to delete first (DELETE doesn't return count directly)
        let mut count_result = self
            .client
            .query("SELECT count() AS total FROM audit_events WHERE timestamp < $cutoff GROUP ALL")
            .bind(("cutoff", cutoff_str.clone()))
            .await
            .and_then(|response| response.check())
            .map_err(|e| {
                Error::Internal(format!("Failed to count audit events for purge: {}", e))
            })?;

        #[derive(Deserialize, SurrealValue)]
        struct CountRow {
            total: i64,
        }

        let count_rows: Vec<CountRow> = count_result
            .take(0)
            .map_err(|e| Error::Internal(format!("Failed to decode audit purge count: {e}")))?;
        let total = count_rows.first().map(|r| r.total).unwrap_or(0);

        // Temporarily allow deletes
        self.client
            .query(
                r#"
                DEFINE TABLE OVERWRITE audit_events SCHEMAFUL
                    PERMISSIONS
                        FOR select FULL
                        FOR create FULL
                        FOR update NONE
                        FOR delete FULL
                "#,
            )
            .await
            .and_then(|response| response.check())
            .map_err(|e| {
                Error::Internal(format!(
                    "Failed to temporarily allow deletes on audit_events: {}",
                    e
                ))
            })?;

        // Perform the delete
        let delete_result = self
            .client
            .query("DELETE FROM audit_events WHERE timestamp < $cutoff")
            .bind(("cutoff", cutoff_str))
            .await
            .and_then(|response| response.check());

        // Reinstate immutability regardless of delete outcome
        let reinstate_result = self
            .client
            .query(
                r#"
                DEFINE TABLE OVERWRITE audit_events SCHEMAFUL
                    PERMISSIONS
                        FOR select FULL
                        FOR create FULL
                        FOR update NONE
                        FOR delete NONE
                "#,
            )
            .await
            .and_then(|response| response.check());

        reinstate_result.map_err(|e| {
            Error::Internal(format!(
                "Failed to reinstate audit_events delete protection: {e}"
            ))
        })?;

        delete_result
            .map_err(|e| Error::Internal(format!("Failed to purge audit events: {}", e)))?;

        Ok(total as u64)
    }

    async fn verify_chain(&self, from_sequence: u64) -> Result<Option<u64>, Error> {
        let query_from = i64::try_from(from_sequence.saturating_sub(1)).map_err(|_| {
            Error::Internal("Audit verification sequence exceeds the storage range".to_string())
        })?;
        let mut result = self
            .client
            .query("SELECT * FROM audit_events WHERE sequence >= $seq ORDER BY sequence ASC")
            .bind(("seq", query_from))
            .await
            .and_then(|response| response.check())
            .map_err(|e| {
                Error::Internal(format!("Failed to fetch events for verification: {}", e))
            })?;

        let rows: Vec<AuditRow> = result
            .take(0)
            .map_err(|e| Error::Internal(format!("Failed to deserialize audit events: {}", e)))?;

        let events: Vec<AuditEvent> = rows
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<_, _>>()?;

        super::verify_stored_chain(&events, from_sequence)
    }
}

fn parse_event_kind(s: &str) -> AuditEventKind {
    match s {
        "auth.token.validated" => AuditEventKind::AuthTokenValidated,
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

#[async_trait]
impl super::lazy::InitializableStorage for SurrealAuditStorage {
    type Conn = Arc<SurrealClient>;

    fn from_conn(conn: Self::Conn) -> Self {
        Self::new(conn)
    }

    async fn init_schema(&self) -> Result<(), Error> {
        self.initialize().await
    }

    fn backend_name() -> &'static str {
        "SurrealDB"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::{storage::AuditVerification, AuditChain};

    #[tokio::test]
    async fn embedded_surreal_round_trip_paging_verification_and_retention() {
        let client = surrealdb::engine::any::connect("mem://").await.unwrap();
        client
            .use_ns("audit_test")
            .use_db("audit_test")
            .await
            .unwrap();
        let storage = SurrealAuditStorage::new(Arc::new(client));
        storage.initialize().await.unwrap();
        storage.initialize().await.unwrap();
        let mut chain = AuditChain::new("surreal-test".into());
        let mut events = Vec::new();
        for offset in 0..5 {
            let mut event = AuditEvent::new(
                AuditEventKind::HttpRequest,
                AuditSeverity::Informational,
                "surreal-test".into(),
            );
            event.timestamp =
                DateTime::from_timestamp(1_700_000_000 + offset, 123_456_789).unwrap();
            event.metadata = Some(
                serde_json::json!({"schema":"case","action":"read","nested":{"value":3,"tags":["one","two"],"optional":null}}),
            );
            event.source = AuditSource {
                ip: Some("127.0.0.1".into()),
                user_agent: Some("regression-client/1".into()),
                subject: Some("user_test".into()),
                request_id: Some("req_test".into()),
            };
            event.method = Some("PATCH".into());
            event.path = Some("/entities/project/one?revision=2".into());
            event.status_code = Some(200);
            event.duration_ms = Some(123);
            event.kind = AuditEventKind::Custom("project.updated".into());
            if offset == 0 {
                event.id = "dca93650-9d2c-4ca8-a00f-79a63467c187".parse().unwrap();
            }
            if offset == 4 {
                event.kind = AuditEventKind::AuthTokenValidated;
            }
            let event = chain.seal(event);
            storage.append(&event).await.unwrap();
            events.push(event);
        }
        assert!(
            storage.append(&events[0]).await.is_err(),
            "duplicate record statement errors must reach the caller"
        );
        let mut q = super::super::AuditQuery {
            limit: 2,
            through_sequence: Some(4),
            actor: Some("user_test".into()),
            request_id: Some("req_test".into()),
            from: Some(events[0].timestamp),
            to: Some(events[4].timestamp),
            ..Default::default()
        };
        let first = storage.query_filtered(&q).await.unwrap();
        assert_eq!(
            first.iter().map(|e| e.sequence).collect::<Vec<_>>(),
            vec![4, 3]
        );
        q.cursor = Some(3);
        assert_eq!(
            storage
                .query_filtered(&q)
                .await
                .unwrap()
                .iter()
                .map(|e| e.sequence)
                .collect::<Vec<_>>(),
            vec![2, 1]
        );
        q.order = super::super::AuditOrder::OldestFirst;
        assert_eq!(
            storage
                .query_filtered(&q)
                .await
                .unwrap()
                .iter()
                .map(|e| e.sequence)
                .collect::<Vec<_>>(),
            vec![4]
        );
        q.actor = None;
        q.schema = Some("case".into());
        q.metadata_kinds = Some(vec![]);
        assert!(storage.query_filtered(&q).await.unwrap().is_empty());
        q.metadata_kinds = Some(vec!["custom.project.updated".into()]);
        q.schema = Some("does-not-exist".into());
        assert!(storage.query_filtered(&q).await.unwrap().is_empty());
        let last = storage.latest().await.unwrap().unwrap();
        assert_eq!(last.id, events[4].id);
        assert_eq!(last.timestamp, events[4].timestamp);
        assert_eq!(last.metadata, events[4].metadata);
        assert_eq!(
            serde_json::to_value(&last).unwrap(),
            serde_json::to_value(&events[4]).unwrap()
        );
        assert_eq!(storage.sequence_bounds().await.unwrap(), Some((1, 5)));
        assert_eq!(
            storage
                .query_sequence(2, 4, 2)
                .await
                .unwrap()
                .iter()
                .map(|e| e.sequence)
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert_eq!(
            storage.verify_chain_range(2, 4).await.unwrap(),
            AuditVerification::Consistent
        );
        assert_eq!(storage.verify_chain(1).await.unwrap(), None);
        assert_eq!(
            storage
                .query_before(events[2].timestamp, 10)
                .await
                .unwrap()
                .len(),
            2
        );
        assert_eq!(storage.purge_before(events[2].timestamp).await.unwrap(), 2);
        assert_eq!(storage.sequence_bounds().await.unwrap(), Some((3, 5)));
        assert_eq!(
            storage.verify_chain_range(3, 5).await.unwrap(),
            AuditVerification::Incomplete
        );
        assert_eq!(
            storage.verify_chain_range(4, 5).await.unwrap(),
            AuditVerification::Consistent
        );
    }
    #[tokio::test]
    async fn legacy_v1_and_v2_archives_round_trip_in_surreal_storage() {
        for archived in [
            include_str!("../fixtures/legacy-v1.json"),
            include_str!("../fixtures/legacy-v2.json"),
        ] {
            let client = surrealdb::engine::any::connect("mem://").await.unwrap();
            client.use_ns("legacy").use_db("legacy").await.unwrap();
            let storage = SurrealAuditStorage::new(Arc::new(client));
            storage.initialize().await.unwrap();
            let legacy: AuditEvent = serde_json::from_str(archived).unwrap();
            storage.append(&legacy).await.unwrap();
            let restored = storage.latest().await.unwrap().unwrap();
            assert_eq!(
                serde_json::to_value(&restored).unwrap(),
                serde_json::to_value(&legacy).unwrap()
            );
            let mut chain =
                AuditChain::resume(legacy.service_name.clone(), legacy.hash.clone().unwrap(), 1);
            let modern = chain.seal(AuditEvent::new(
                AuditEventKind::HttpRequest,
                AuditSeverity::Informational,
                legacy.service_name,
            ));
            assert_eq!(modern.id.as_uuid().get_version_num(), 7);
            storage.append(&modern).await.unwrap();
            assert_eq!(
                storage.verify_chain_range(1, 2).await.unwrap(),
                AuditVerification::Consistent
            );
        }
    }
}
