//! SurrealDB signing key storage backend
//!
//! Stores signing key metadata in a `signing_keys` table using SurrealDB's
//! SCHEMAFULL mode. Uses `FOR delete NONE` to prevent key deletion but
//! `FOR update FULL` to allow status transitions. Optimistic concurrency
//! via conditional WHERE clauses ensures safe concurrent rotations.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::KeyRotationStorage;
use crate::auth::key_rotation::key_metadata::{KeyFormat, KeyStatus, SigningKeyMetadata};
use crate::error::Error;
use crate::surrealdb_backend::SurrealClient;

/// SurrealDB-backed signing key storage
pub struct SurrealKeyRotationStorage {
    client: Arc<SurrealClient>,
}

impl SurrealKeyRotationStorage {
    /// Create a new SurrealDB key rotation storage
    pub fn new(client: Arc<SurrealClient>) -> Self {
        Self { client }
    }
}

/// Serializable record for SurrealDB inserts
#[derive(Serialize)]
struct SigningKeyRecord {
    kid: String,
    format: String,
    key_material: String,
    status: String,
    created_at: String,
    activated_at: Option<String>,
    draining_since: Option<String>,
    retired_at: Option<String>,
    drain_expires_at: Option<String>,
    service_name: String,
    key_hash: String,
}

/// Deserializable record from SurrealDB queries
#[derive(Deserialize)]
struct SigningKeyRow {
    // SurrealDB may return `id` as a Thing or string; we ignore it and use `kid`
    #[allow(dead_code)]
    id: serde_json::Value,
    kid: String,
    format: String,
    key_material: String,
    status: String,
    created_at: String,
    activated_at: Option<String>,
    draining_since: Option<String>,
    retired_at: Option<String>,
    drain_expires_at: Option<String>,
    service_name: String,
    key_hash: String,
}

impl TryFrom<SigningKeyRow> for SigningKeyMetadata {
    type Error = Error;

    fn try_from(row: SigningKeyRow) -> Result<Self, Self::Error> {
        let format: KeyFormat = row
            .format
            .parse()
            .map_err(|e| Error::Internal(format!("Invalid key format in database: {}", e)))?;

        let status: KeyStatus = row
            .status
            .parse()
            .map_err(|e| Error::Internal(format!("Invalid key status in database: {}", e)))?;

        let created_at = DateTime::parse_from_rfc3339(&row.created_at)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| Error::Internal(format!("Failed to parse created_at: {}", e)))?;

        let activated_at = parse_optional_rfc3339(&row.activated_at)?;
        let draining_since = parse_optional_rfc3339(&row.draining_since)?;
        let retired_at = parse_optional_rfc3339(&row.retired_at)?;
        let drain_expires_at = parse_optional_rfc3339(&row.drain_expires_at)?;

        Ok(SigningKeyMetadata {
            kid: row.kid,
            format,
            key_material: row.key_material,
            status,
            created_at,
            activated_at,
            draining_since,
            retired_at,
            drain_expires_at,
            service_name: row.service_name,
            key_hash: row.key_hash,
        })
    }
}

/// Parse an optional RFC 3339 timestamp string
fn parse_optional_rfc3339(
    value: &Option<String>,
) -> Result<Option<DateTime<Utc>>, Error> {
    match value {
        Some(s) if !s.is_empty() => {
            let dt = DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| {
                    Error::Internal(format!("Failed to parse timestamp '{}': {}", s, e))
                })?;
            Ok(Some(dt))
        }
        _ => Ok(None),
    }
}

#[async_trait]
impl KeyRotationStorage for SurrealKeyRotationStorage {
    async fn initialize(&self) -> Result<(), Error> {
        self.client
            .query(
                r#"
                DEFINE TABLE IF NOT EXISTS signing_keys SCHEMAFULL
                    PERMISSIONS
                        FOR select FULL
                        FOR create FULL
                        FOR update FULL
                        FOR delete NONE;

                DEFINE FIELD IF NOT EXISTS kid ON signing_keys TYPE string;
                DEFINE FIELD IF NOT EXISTS format ON signing_keys TYPE string;
                DEFINE FIELD IF NOT EXISTS key_material ON signing_keys TYPE string;
                DEFINE FIELD IF NOT EXISTS status ON signing_keys TYPE string
                    ASSERT $value IN ['active', 'draining', 'retired'];
                DEFINE FIELD IF NOT EXISTS created_at ON signing_keys TYPE string;
                DEFINE FIELD IF NOT EXISTS activated_at ON signing_keys TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS draining_since ON signing_keys TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS retired_at ON signing_keys TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS drain_expires_at ON signing_keys TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS service_name ON signing_keys TYPE string;
                DEFINE FIELD IF NOT EXISTS key_hash ON signing_keys TYPE string;

                DEFINE INDEX IF NOT EXISTS idx_signing_keys_kid ON signing_keys FIELDS kid UNIQUE;
                DEFINE INDEX IF NOT EXISTS idx_signing_keys_status ON signing_keys FIELDS status;
                DEFINE INDEX IF NOT EXISTS idx_signing_keys_service ON signing_keys FIELDS service_name, status;
                "#,
            )
            .await
            .map_err(|e| {
                Error::Internal(format!(
                    "Failed to initialize signing_keys schema: {}",
                    e
                ))
            })?;

        Ok(())
    }

    async fn store_key(&self, key: &SigningKeyMetadata) -> Result<(), Error> {
        let record = SigningKeyRecord {
            kid: key.kid.clone(),
            format: key.format.to_string(),
            key_material: key.key_material.clone(),
            status: key.status.to_string(),
            created_at: key.created_at.to_rfc3339(),
            activated_at: key.activated_at.map(|t| t.to_rfc3339()),
            draining_since: key.draining_since.map(|t| t.to_rfc3339()),
            retired_at: key.retired_at.map(|t| t.to_rfc3339()),
            drain_expires_at: key.drain_expires_at.map(|t| t.to_rfc3339()),
            service_name: key.service_name.clone(),
            key_hash: key.key_hash.clone(),
        };

        // Use owned String for the record ID to satisfy .bind() requirements
        let record_kid = key.kid.clone();

        self.client
            .query("CREATE type::thing('signing_keys', $kid) CONTENT $data")
            .bind(("kid", record_kid))
            .bind(("data", record))
            .await
            .map_err(|e| Error::Internal(format!("Failed to store signing key: {}", e)))?;

        Ok(())
    }

    async fn get_active_key(
        &self,
        service_name: &str,
    ) -> Result<Option<SigningKeyMetadata>, Error> {
        // Pass owned String to .bind()
        let svc = service_name.to_string();

        let mut result = self
            .client
            .query(
                "SELECT * FROM signing_keys \
                 WHERE service_name = $svc AND status = 'active' LIMIT 1",
            )
            .bind(("svc", svc))
            .await
            .map_err(|e| Error::Internal(format!("Failed to get active signing key: {}", e)))?;

        let rows: Vec<SigningKeyRow> = result
            .take(0)
            .map_err(|e| Error::Internal(format!("Failed to deserialize signing key: {}", e)))?;

        rows.into_iter()
            .next()
            .map(TryInto::try_into)
            .transpose()
    }

    async fn get_key_by_kid(&self, kid: &str) -> Result<Option<SigningKeyMetadata>, Error> {
        let kid_owned = kid.to_string();

        let mut result = self
            .client
            .query("SELECT * FROM signing_keys WHERE kid = $kid")
            .bind(("kid", kid_owned))
            .await
            .map_err(|e| {
                Error::Internal(format!("Failed to get signing key by kid: {}", e))
            })?;

        let rows: Vec<SigningKeyRow> = result
            .take(0)
            .map_err(|e| Error::Internal(format!("Failed to deserialize signing key: {}", e)))?;

        rows.into_iter()
            .next()
            .map(TryInto::try_into)
            .transpose()
    }

    async fn get_verification_keys(
        &self,
        service_name: &str,
    ) -> Result<Vec<SigningKeyMetadata>, Error> {
        let svc = service_name.to_string();

        let mut result = self
            .client
            .query(
                "SELECT * FROM signing_keys \
                 WHERE service_name = $svc AND (status = 'active' OR status = 'draining') \
                 ORDER BY created_at DESC",
            )
            .bind(("svc", svc))
            .await
            .map_err(|e| Error::Internal(format!("Failed to get verification keys: {}", e)))?;

        let rows: Vec<SigningKeyRow> = result
            .take(0)
            .map_err(|e| {
                Error::Internal(format!("Failed to deserialize verification keys: {}", e))
            })?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn update_key_status(
        &self,
        kid: &str,
        new_status: KeyStatus,
        timestamp: DateTime<Utc>,
    ) -> Result<(), Error> {
        let kid_owned = kid.to_string();
        let ts = timestamp.to_rfc3339();

        // Use optimistic concurrency: WHERE includes old status check.
        // If 0 rows returned, someone else already transitioned this key.
        let query_str = match new_status {
            KeyStatus::Active => {
                "UPDATE signing_keys SET status = 'active', activated_at = $ts \
                 WHERE kid = $kid RETURN AFTER"
            }
            KeyStatus::Draining => {
                "UPDATE signing_keys SET status = 'draining', draining_since = $ts \
                 WHERE kid = $kid AND status = 'active' RETURN AFTER"
            }
            KeyStatus::Retired => {
                "UPDATE signing_keys SET status = 'retired', retired_at = $ts \
                 WHERE kid = $kid AND status = 'draining' RETURN AFTER"
            }
        };

        let mut result = self
            .client
            .query(query_str)
            .bind(("kid", kid_owned))
            .bind(("ts", ts))
            .await
            .map_err(|e| {
                Error::Internal(format!("Failed to update signing key status: {}", e))
            })?;

        let rows: Vec<SigningKeyRow> = result.take(0).unwrap_or_default();

        if rows.is_empty() {
            return Err(Error::Conflict(format!(
                "Key '{}' was not updated to '{}' -- it may not exist or is not in the expected state",
                kid, new_status
            )));
        }

        Ok(())
    }

    async fn retire_expired_draining_keys(&self, now: DateTime<Utc>) -> Result<u64, Error> {
        let now_str = now.to_rfc3339();

        // Count how many will be affected first
        let mut count_result = self
            .client
            .query(
                "SELECT count() AS total FROM signing_keys \
                 WHERE status = 'draining' AND drain_expires_at <= $now GROUP ALL",
            )
            .bind(("now", now_str.clone()))
            .await
            .map_err(|e| {
                Error::Internal(format!(
                    "Failed to count expired draining keys: {}",
                    e
                ))
            })?;

        #[derive(Deserialize)]
        struct CountRow {
            total: i64,
        }

        let count_rows: Vec<CountRow> = count_result.take(0).unwrap_or_default();
        let total = count_rows.first().map(|r| r.total).unwrap_or(0);

        // Perform the update
        self.client
            .query(
                "UPDATE signing_keys SET status = 'retired', retired_at = $now \
                 WHERE status = 'draining' AND drain_expires_at <= $now",
            )
            .bind(("now", now_str))
            .await
            .map_err(|e| {
                Error::Internal(format!(
                    "Failed to retire expired draining keys: {}",
                    e
                ))
            })?;

        Ok(total as u64)
    }
}
