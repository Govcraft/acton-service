//! Turso/libsql signing key storage backend
//!
//! Stores signing key metadata in a `signing_keys` table using libsql.
//! Uses CHECK constraints for status validation and conditional WHERE
//! clauses for safe concurrent status transitions.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;

use super::KeyRotationStorage;
use crate::auth::key_rotation::key_metadata::{KeyFormat, KeyStatus, SigningKeyMetadata};
use crate::error::Error;

/// Turso-backed signing key storage
pub struct TursoKeyRotationStorage {
    db: Arc<libsql::Database>,
}

impl TursoKeyRotationStorage {
    /// Create a new Turso key rotation storage
    pub fn new(db: Arc<libsql::Database>) -> Self {
        Self { db }
    }

    /// Get a connection from the database
    fn connect(&self) -> Result<libsql::Connection, Error> {
        self.db
            .connect()
            .map_err(|e| Error::Internal(format!("Failed to connect for key rotation: {}", e)))
    }
}

#[async_trait]
impl KeyRotationStorage for TursoKeyRotationStorage {
    async fn initialize(&self) -> Result<(), Error> {
        let conn = self.connect()?;

        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS signing_keys (
                kid TEXT PRIMARY KEY,
                format TEXT NOT NULL,
                key_material TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active'
                    CHECK (status IN ('active', 'draining', 'retired')),
                created_at TEXT NOT NULL,
                activated_at TEXT,
                draining_since TEXT,
                retired_at TEXT,
                drain_expires_at TEXT,
                service_name TEXT NOT NULL,
                key_hash TEXT NOT NULL
            )
            "#,
            (),
        )
        .await
        .map_err(|e| Error::Internal(format!("Failed to create signing_keys table: {}", e)))?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_signing_keys_status ON signing_keys (status)",
            (),
        )
        .await
        .map_err(|e| {
            Error::Internal(format!("Failed to create signing_keys status index: {}", e))
        })?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_signing_keys_service \
             ON signing_keys (service_name, status)",
            (),
        )
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
        let conn = self.connect()?;

        conn.execute(
            r#"
            INSERT INTO signing_keys (
                kid, format, key_material, status, created_at,
                activated_at, draining_since, retired_at, drain_expires_at,
                service_name, key_hash
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
            libsql::params![
                key.kid.clone(),
                key.format.to_string(),
                key.key_material.clone(),
                key.status.to_string(),
                key.created_at.to_rfc3339(),
                key.activated_at.map(|t| t.to_rfc3339()),
                key.draining_since.map(|t| t.to_rfc3339()),
                key.retired_at.map(|t| t.to_rfc3339()),
                key.drain_expires_at.map(|t| t.to_rfc3339()),
                key.service_name.clone(),
                key.key_hash.clone(),
            ],
        )
        .await
        .map_err(|e| Error::Internal(format!("Failed to store signing key: {}", e)))?;

        Ok(())
    }

    async fn get_active_key(
        &self,
        service_name: &str,
    ) -> Result<Option<SigningKeyMetadata>, Error> {
        let conn = self.connect()?;

        let mut rows = conn
            .query(
                "SELECT * FROM signing_keys WHERE service_name = ?1 AND status = 'active' LIMIT 1",
                libsql::params![service_name.to_string()],
            )
            .await
            .map_err(|e| Error::Internal(format!("Failed to get active signing key: {}", e)))?;

        match rows.next().await {
            Ok(Some(row)) => Ok(Some(row_to_metadata(&row)?)),
            Ok(None) => Ok(None),
            Err(e) => Err(Error::Internal(format!(
                "Failed to read signing key row: {}",
                e
            ))),
        }
    }

    async fn get_key_by_kid(&self, kid: &str) -> Result<Option<SigningKeyMetadata>, Error> {
        let conn = self.connect()?;

        let mut rows = conn
            .query(
                "SELECT * FROM signing_keys WHERE kid = ?1",
                libsql::params![kid.to_string()],
            )
            .await
            .map_err(|e| Error::Internal(format!("Failed to get signing key by kid: {}", e)))?;

        match rows.next().await {
            Ok(Some(row)) => Ok(Some(row_to_metadata(&row)?)),
            Ok(None) => Ok(None),
            Err(e) => Err(Error::Internal(format!(
                "Failed to read signing key row: {}",
                e
            ))),
        }
    }

    async fn get_verification_keys(
        &self,
        service_name: &str,
    ) -> Result<Vec<SigningKeyMetadata>, Error> {
        let conn = self.connect()?;

        let mut rows = conn
            .query(
                "SELECT * FROM signing_keys \
                 WHERE service_name = ?1 AND status IN ('active', 'draining') \
                 ORDER BY created_at DESC",
                libsql::params![service_name.to_string()],
            )
            .await
            .map_err(|e| Error::Internal(format!("Failed to get verification keys: {}", e)))?;

        let mut keys = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            keys.push(row_to_metadata(&row)?);
        }
        Ok(keys)
    }

    async fn update_key_status(
        &self,
        kid: &str,
        new_status: KeyStatus,
        timestamp: DateTime<Utc>,
    ) -> Result<(), Error> {
        let conn = self.connect()?;

        let ts = timestamp.to_rfc3339();
        let kid_owned = kid.to_string();

        let affected = match new_status {
            KeyStatus::Active => {
                conn.execute(
                    "UPDATE signing_keys SET status = 'active', activated_at = ?1 WHERE kid = ?2",
                    libsql::params![ts, kid_owned],
                )
                .await
            }
            KeyStatus::Draining => {
                conn.execute(
                    "UPDATE signing_keys SET status = 'draining', draining_since = ?1 \
                     WHERE kid = ?2 AND status = 'active'",
                    libsql::params![ts, kid_owned],
                )
                .await
            }
            KeyStatus::Retired => {
                conn.execute(
                    "UPDATE signing_keys SET status = 'retired', retired_at = ?1 \
                     WHERE kid = ?2 AND status = 'draining'",
                    libsql::params![ts, kid_owned],
                )
                .await
            }
        }
        .map_err(|e| Error::Internal(format!("Failed to update signing key status: {}", e)))?;

        if affected == 0 {
            return Err(Error::Conflict(format!(
                "Key '{}' was not updated to '{}' -- it may not exist or is not in the expected state",
                kid, new_status
            )));
        }

        Ok(())
    }

    async fn retire_expired_draining_keys(&self, now: DateTime<Utc>) -> Result<u64, Error> {
        let conn = self.connect()?;
        let now_str = now.to_rfc3339();

        let affected = conn
            .execute(
                "UPDATE signing_keys SET status = 'retired', retired_at = ?1 \
                 WHERE status = 'draining' AND drain_expires_at <= ?1",
                libsql::params![now_str],
            )
            .await
            .map_err(|e| {
                Error::Internal(format!(
                    "Failed to retire expired draining keys: {}",
                    e
                ))
            })?;

        Ok(affected)
    }
}

/// Parse a libsql row into SigningKeyMetadata
fn row_to_metadata(row: &libsql::Row) -> Result<SigningKeyMetadata, Error> {
    let kid: String = row
        .get(0)
        .map_err(|e| Error::Internal(format!("Failed to read kid: {}", e)))?;

    let format_str: String = row
        .get(1)
        .map_err(|e| Error::Internal(format!("Failed to read format: {}", e)))?;
    let format: KeyFormat = format_str
        .parse()
        .map_err(|e| Error::Internal(format!("Invalid key format in database: {}", e)))?;

    let key_material: String = row
        .get(2)
        .map_err(|e| Error::Internal(format!("Failed to read key_material: {}", e)))?;

    let status_str: String = row
        .get(3)
        .map_err(|e| Error::Internal(format!("Failed to read status: {}", e)))?;
    let status: KeyStatus = status_str
        .parse()
        .map_err(|e| Error::Internal(format!("Invalid key status in database: {}", e)))?;

    let created_at_str: String = row
        .get(4)
        .map_err(|e| Error::Internal(format!("Failed to read created_at: {}", e)))?;
    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| Error::Internal(format!("Failed to parse created_at: {}", e)))?;

    let activated_at = parse_optional_timestamp(row, 5)?;
    let draining_since = parse_optional_timestamp(row, 6)?;
    let retired_at = parse_optional_timestamp(row, 7)?;
    let drain_expires_at = parse_optional_timestamp(row, 8)?;

    let service_name: String = row
        .get(9)
        .map_err(|e| Error::Internal(format!("Failed to read service_name: {}", e)))?;

    let key_hash: String = row
        .get(10)
        .map_err(|e| Error::Internal(format!("Failed to read key_hash: {}", e)))?;

    Ok(SigningKeyMetadata {
        kid,
        format,
        key_material,
        status,
        created_at,
        activated_at,
        draining_since,
        retired_at,
        drain_expires_at,
        service_name,
        key_hash,
    })
}

/// Parse an optional RFC 3339 timestamp from a libsql row column
fn parse_optional_timestamp(
    row: &libsql::Row,
    index: i32,
) -> Result<Option<DateTime<Utc>>, Error> {
    match row.get::<String>(index) {
        Ok(s) if !s.is_empty() => {
            let dt = DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| {
                    Error::Internal(format!(
                        "Failed to parse timestamp at column {}: {}",
                        index, e
                    ))
                })?;
            Ok(Some(dt))
        }
        _ => Ok(None),
    }
}
