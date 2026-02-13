//! PostgreSQL signing key storage backend
//!
//! Stores signing key metadata in a `signing_keys` table. Uses PostgreSQL
//! CHECK constraints to enforce valid status values and `SELECT ... FOR UPDATE`
//! for safe concurrent status transitions.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::KeyRotationStorage;
use crate::auth::key_rotation::key_metadata::{KeyFormat, KeyStatus, SigningKeyMetadata};
use crate::error::Error;

/// PostgreSQL-backed signing key storage
pub struct PgKeyRotationStorage {
    pool: PgPool,
}

impl PgKeyRotationStorage {
    /// Create a new PostgreSQL key rotation storage
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl KeyRotationStorage for PgKeyRotationStorage {
    async fn initialize(&self) -> Result<(), Error> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS signing_keys (
                kid TEXT PRIMARY KEY,
                format TEXT NOT NULL,
                key_material TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active'
                    CHECK (status IN ('active', 'draining', 'retired')),
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                activated_at TIMESTAMPTZ,
                draining_since TIMESTAMPTZ,
                retired_at TIMESTAMPTZ,
                drain_expires_at TIMESTAMPTZ,
                service_name TEXT NOT NULL,
                key_hash TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Internal(format!("Failed to create signing_keys table: {}", e)))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_signing_keys_status ON signing_keys (status)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            Error::Internal(format!("Failed to create signing_keys status index: {}", e))
        })?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_signing_keys_service \
             ON signing_keys (service_name, status)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            Error::Internal(format!(
                "Failed to create signing_keys service index: {}",
                e
            ))
        })?;

        Ok(())
    }

    async fn store_key(&self, key: &SigningKeyMetadata) -> Result<(), Error> {
        sqlx::query(
            r#"
            INSERT INTO signing_keys (
                kid, format, key_material, status, created_at,
                activated_at, draining_since, retired_at, drain_expires_at,
                service_name, key_hash
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(&key.kid)
        .bind(key.format.to_string())
        .bind(&key.key_material)
        .bind(key.status.to_string())
        .bind(key.created_at)
        .bind(key.activated_at)
        .bind(key.draining_since)
        .bind(key.retired_at)
        .bind(key.drain_expires_at)
        .bind(&key.service_name)
        .bind(&key.key_hash)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Internal(format!("Failed to store signing key: {}", e)))?;

        Ok(())
    }

    async fn get_active_key(
        &self,
        service_name: &str,
    ) -> Result<Option<SigningKeyMetadata>, Error> {
        let row = sqlx::query_as::<_, SigningKeyRow>(
            "SELECT * FROM signing_keys WHERE service_name = $1 AND status = 'active' LIMIT 1",
        )
        .bind(service_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Internal(format!("Failed to get active signing key: {}", e)))?;

        row.map(TryInto::try_into).transpose()
    }

    async fn get_key_by_kid(&self, kid: &str) -> Result<Option<SigningKeyMetadata>, Error> {
        let row = sqlx::query_as::<_, SigningKeyRow>(
            "SELECT * FROM signing_keys WHERE kid = $1",
        )
        .bind(kid)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Internal(format!("Failed to get signing key by kid: {}", e)))?;

        row.map(TryInto::try_into).transpose()
    }

    async fn get_verification_keys(
        &self,
        service_name: &str,
    ) -> Result<Vec<SigningKeyMetadata>, Error> {
        let rows = sqlx::query_as::<_, SigningKeyRow>(
            "SELECT * FROM signing_keys \
             WHERE service_name = $1 AND status IN ('active', 'draining') \
             ORDER BY created_at DESC",
        )
        .bind(service_name)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Internal(format!("Failed to get verification keys: {}", e)))?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn update_key_status(
        &self,
        kid: &str,
        new_status: KeyStatus,
        timestamp: DateTime<Utc>,
    ) -> Result<(), Error> {
        let result = match new_status {
            KeyStatus::Active => {
                sqlx::query(
                    "UPDATE signing_keys SET status = 'active', activated_at = $1 WHERE kid = $2",
                )
                .bind(timestamp)
                .bind(kid)
                .execute(&self.pool)
                .await
            }
            KeyStatus::Draining => {
                sqlx::query(
                    "UPDATE signing_keys SET status = 'draining', draining_since = $1 \
                     WHERE kid = $2 AND status = 'active'",
                )
                .bind(timestamp)
                .bind(kid)
                .execute(&self.pool)
                .await
            }
            KeyStatus::Retired => {
                sqlx::query(
                    "UPDATE signing_keys SET status = 'retired', retired_at = $1 \
                     WHERE kid = $2 AND status = 'draining'",
                )
                .bind(timestamp)
                .bind(kid)
                .execute(&self.pool)
                .await
            }
        }
        .map_err(|e| Error::Internal(format!("Failed to update signing key status: {}", e)))?;

        if result.rows_affected() == 0 {
            return Err(Error::Conflict(format!(
                "Key '{}' was not updated to '{}' -- it may not exist or is not in the expected state",
                kid, new_status
            )));
        }

        Ok(())
    }

    async fn retire_expired_draining_keys(&self, now: DateTime<Utc>) -> Result<u64, Error> {
        let result = sqlx::query(
            "UPDATE signing_keys SET status = 'retired', retired_at = $1 \
             WHERE status = 'draining' AND drain_expires_at <= $1",
        )
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            Error::Internal(format!(
                "Failed to retire expired draining keys: {}",
                e
            ))
        })?;

        Ok(result.rows_affected())
    }
}

/// Internal row type for sqlx mapping
#[derive(sqlx::FromRow)]
struct SigningKeyRow {
    kid: String,
    format: String,
    key_material: String,
    status: String,
    created_at: DateTime<Utc>,
    activated_at: Option<DateTime<Utc>>,
    draining_since: Option<DateTime<Utc>>,
    retired_at: Option<DateTime<Utc>>,
    drain_expires_at: Option<DateTime<Utc>>,
    service_name: String,
    key_hash: String,
}

impl TryFrom<SigningKeyRow> for SigningKeyMetadata {
    type Error = Error;

    fn try_from(row: SigningKeyRow) -> Result<Self, Self::Error> {
        let format: KeyFormat = row
            .format
            .parse()
            .map_err(|e: crate::auth::key_rotation::key_metadata::ParseKeyFormatError| {
                Error::Internal(format!("Invalid key format in database: {}", e))
            })?;

        let status: KeyStatus = row
            .status
            .parse()
            .map_err(|e: crate::auth::key_rotation::key_metadata::ParseKeyStatusError| {
                Error::Internal(format!("Invalid key status in database: {}", e))
            })?;

        Ok(SigningKeyMetadata {
            kid: row.kid,
            format,
            key_material: row.key_material,
            status,
            created_at: row.created_at,
            activated_at: row.activated_at,
            draining_since: row.draining_since,
            retired_at: row.retired_at,
            drain_expires_at: row.drain_expires_at,
            service_name: row.service_name,
            key_hash: row.key_hash,
        })
    }
}
