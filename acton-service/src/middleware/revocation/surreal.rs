use super::{expires_at, RevocationNamespace, TokenRevocation};
use crate::{error::Error, surrealdb_backend::SurrealClient};
use async_trait::async_trait;
use std::sync::Arc;
use surrealdb::types::SurrealValue;

/// SurrealDB-backed revocation, scoped by namespace.
#[derive(Clone)]
pub struct SurrealTokenRevocation {
    client: Arc<SurrealClient>,
    namespace: RevocationNamespace,
}
impl SurrealTokenRevocation {
    /// Create a checker using an existing client.
    pub fn new(client: Arc<SurrealClient>, namespace: RevocationNamespace) -> Self {
        Self { client, namespace }
    }
    /// Create the revocation schema. Safe to run repeatedly.
    pub async fn initialize(&self) -> Result<(), Error> {
        self.client.query("DEFINE TABLE IF NOT EXISTS token_revocations SCHEMAFULL; DEFINE FIELD IF NOT EXISTS namespace ON token_revocations TYPE string; DEFINE FIELD IF NOT EXISTS kind ON token_revocations TYPE string ASSERT $value IN ['token','subject']; DEFINE FIELD IF NOT EXISTS identifier ON token_revocations TYPE string; DEFINE FIELD IF NOT EXISTS stamp ON token_revocations TYPE int; DEFINE INDEX IF NOT EXISTS token_revocations_lookup ON token_revocations FIELDS namespace,kind,identifier UNIQUE;").await.map_err(storage_error)?.check().map_err(storage_error)?;
        Ok(())
    }
    async fn ensure_schema(&self) -> Result<(), Error> {
        self.client
            .query("INFO FOR TABLE token_revocations")
            .await
            .map_err(storage_error)?
            .check()
            .map_err(storage_error)?;
        Ok(())
    }
    async fn read(&self, kind: &str, identifier: &str) -> Result<Option<i64>, Error> {
        self.ensure_schema().await?;
        #[derive(SurrealValue)]
        struct Row {
            stamp: i64,
        }
        let mut response=self.client.query("SELECT stamp FROM token_revocations WHERE namespace=$namespace AND kind=$kind AND identifier=$identifier")
            .bind(("namespace",self.namespace.as_str().to_owned())).bind(("kind",kind.to_owned())).bind(("identifier",identifier.to_owned())).await.map_err(storage_error)?.check().map_err(storage_error)?;
        let rows: Vec<Row> = response.take(0).map_err(storage_error)?;
        Ok(rows.first().map(|row| row.stamp))
    }
    async fn write(&self, kind: &str, identifier: &str, value: i64) -> Result<(), Error> {
        self.ensure_schema().await?;
        self.client.query("INSERT INTO token_revocations (namespace,kind,identifier,stamp) VALUES ($namespace,$kind,$identifier,$value) ON DUPLICATE KEY UPDATE stamp = math::max([stamp,$input.stamp])")
            .bind(("namespace",self.namespace.as_str().to_owned())).bind(("kind",kind.to_owned())).bind(("identifier",identifier.to_owned())).bind(("value",value)).await.map_err(storage_error)?.check().map_err(storage_error)?;
        Ok(())
    }
    /// Remove expired token records in this namespace. Subject cutoffs persist.
    pub async fn cleanup_expired(&self) -> Result<(), Error> {
        self.ensure_schema().await?;
        self.client.query("DELETE token_revocations WHERE namespace=$namespace AND kind='token' AND stamp <= $now")
            .bind(("namespace",self.namespace.as_str().to_owned())).bind(("now",chrono::Utc::now().timestamp())).await.map_err(storage_error)?.check().map_err(storage_error)?;
        Ok(())
    }
}
fn storage_error(error: surrealdb::Error) -> Error {
    Error::Internal(format!("SurrealDB revocation storage: {error}"))
}
#[async_trait]
impl TokenRevocation for SurrealTokenRevocation {
    async fn initialize(&self) -> Result<(), Error> {
        SurrealTokenRevocation::initialize(self).await
    }
    async fn cleanup_expired(&self) -> Result<(), Error> {
        SurrealTokenRevocation::cleanup_expired(self).await
    }
    async fn is_revoked(&self, jti: &str) -> Result<bool, Error> {
        Ok(self
            .read("token", jti)
            .await?
            .is_some_and(|expiry| expiry > chrono::Utc::now().timestamp()))
    }
    async fn revoke(&self, jti: &str, ttl_secs: u64) -> Result<(), Error> {
        self.write("token", jti, expires_at(ttl_secs)?).await
    }
    async fn subject_not_before(&self, subject: &str) -> Result<Option<i64>, Error> {
        self.read("subject", subject).await
    }
    async fn revoke_subject(&self, subject: &str, not_before: i64) -> Result<(), Error> {
        self.write("subject", subject, not_before).await
    }
}
