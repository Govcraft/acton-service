//! Microsoft SQL Server signing-key storage.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use super::KeyRotationStorage;
use crate::{
    auth::key_rotation::key_metadata::{KeyFormat, KeyStatus, SigningKeyMetadata},
    error::Error,
    mssql::{execute, query, MssqlPool},
};

/// SQL Server-backed signing-key storage.
pub struct MssqlKeyRotationStorage {
    pool: MssqlPool,
}

impl MssqlKeyRotationStorage {
    /// Creates storage backed by the supplied pool.
    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }
}

fn text(row: &tiberius::Row, name: &str) -> Result<String, Error> {
    row.get::<&str, _>(name)
        .map(str::to_owned)
        .ok_or_else(|| Error::Internal(format!("SQL Server returned NULL for {name}")))
}

fn decode(row: &tiberius::Row) -> Result<SigningKeyMetadata, Error> {
    let format = text(row, "format")?
        .parse::<KeyFormat>()
        .map_err(|error| Error::Internal(error.to_string()))?;
    let status = text(row, "status")?
        .parse::<KeyStatus>()
        .map_err(|error| Error::Internal(error.to_string()))?;
    Ok(SigningKeyMetadata {
        kid: text(row, "kid")?,
        format,
        key_material: text(row, "key_material")?,
        status,
        created_at: row
            .get("created_at")
            .ok_or_else(|| Error::Internal("missing created_at".to_string()))?,
        activated_at: row.get("activated_at"),
        draining_since: row.get("draining_since"),
        retired_at: row.get("retired_at"),
        drain_expires_at: row.get("drain_expires_at"),
        service_name: text(row, "service_name")?,
        key_hash: text(row, "key_hash")?,
    })
}

#[async_trait]
impl KeyRotationStorage for MssqlKeyRotationStorage {
    async fn initialize(&self) -> Result<(), Error> {
        execute(&self.pool, "IF OBJECT_ID(N'signing_keys', N'U') IS NULL CREATE TABLE signing_keys (kid NVARCHAR(255) PRIMARY KEY, format NVARCHAR(32) NOT NULL, key_material NVARCHAR(MAX) NOT NULL, status NVARCHAR(16) NOT NULL CONSTRAINT CK_signing_keys_status CHECK (status IN ('active','draining','retired')), created_at DATETIMEOFFSET NOT NULL, activated_at DATETIMEOFFSET NULL, draining_since DATETIMEOFFSET NULL, retired_at DATETIMEOFFSET NULL, drain_expires_at DATETIMEOFFSET NULL, service_name NVARCHAR(255) NOT NULL, key_hash NVARCHAR(255) NOT NULL)", &[]).await?;
        execute(&self.pool, "IF NOT EXISTS (SELECT 1 FROM sys.indexes WHERE name='idx_signing_keys_status') CREATE INDEX idx_signing_keys_status ON signing_keys(status)", &[]).await?;
        Ok(())
    }
    async fn store_key(&self, key: &SigningKeyMetadata) -> Result<(), Error> {
        let format = key.format.to_string();
        let status = key.status.to_string();
        execute(&self.pool, "INSERT INTO signing_keys (kid,format,key_material,status,created_at,activated_at,draining_since,retired_at,drain_expires_at,service_name,key_hash) VALUES (@P1,@P2,@P3,@P4,@P5,@P6,@P7,@P8,@P9,@P10,@P11)", &[&key.kid,&format,&key.key_material,&status,&key.created_at,&key.activated_at,&key.draining_since,&key.retired_at,&key.drain_expires_at,&key.service_name,&key.key_hash]).await?;
        Ok(())
    }
    async fn get_active_key(
        &self,
        service_name: &str,
    ) -> Result<Option<SigningKeyMetadata>, Error> {
        query(&self.pool, "SELECT TOP (1) * FROM signing_keys WHERE service_name=@P1 AND status='active' ORDER BY created_at DESC", &[&service_name]).await?.first().map(decode).transpose()
    }
    async fn get_key_by_kid(&self, kid: &str) -> Result<Option<SigningKeyMetadata>, Error> {
        query(
            &self.pool,
            "SELECT * FROM signing_keys WHERE kid=@P1",
            &[&kid],
        )
        .await?
        .first()
        .map(decode)
        .transpose()
    }
    async fn get_verification_keys(
        &self,
        service_name: &str,
    ) -> Result<Vec<SigningKeyMetadata>, Error> {
        query(&self.pool, "SELECT * FROM signing_keys WHERE service_name=@P1 AND status IN ('active','draining') ORDER BY created_at DESC", &[&service_name]).await?.iter().map(decode).collect()
    }
    async fn update_key_status(
        &self,
        kid: &str,
        status: KeyStatus,
        timestamp: DateTime<Utc>,
    ) -> Result<(), Error> {
        let (sql, expected) = match status { KeyStatus::Active => ("UPDATE signing_keys SET status='active',activated_at=@P1 WHERE kid=@P2", None), KeyStatus::Draining => ("UPDATE signing_keys SET status='draining',draining_since=@P1 WHERE kid=@P2 AND status='active'", Some("active")), KeyStatus::Retired => ("UPDATE signing_keys SET status='retired',retired_at=@P1 WHERE kid=@P2 AND status='draining'", Some("draining")) };
        if execute(&self.pool, sql, &[&timestamp, &kid]).await? == 0 {
            return Err(Error::Conflict(format!(
                "key {kid} was not in expected state {expected:?}"
            )));
        }
        Ok(())
    }
    async fn retire_expired_draining_keys(&self, now: DateTime<Utc>) -> Result<u64, Error> {
        execute(&self.pool, "UPDATE signing_keys SET status='retired',retired_at=@P1 WHERE status='draining' AND drain_expires_at<=@P1", &[&now]).await
    }
}
