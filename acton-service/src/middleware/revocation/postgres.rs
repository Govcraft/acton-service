use super::{expires_at, RevocationNamespace, TokenRevocation};
use crate::error::Error;
use async_trait::async_trait;

/// PostgreSQL-backed revocation, scoped by namespace.
#[derive(Clone)]
pub struct PgTokenRevocation {
    pool: sqlx::PgPool,
    namespace: RevocationNamespace,
}
impl PgTokenRevocation {
    /// Create a checker using an existing pool.
    pub fn new(pool: sqlx::PgPool, namespace: RevocationNamespace) -> Self {
        Self { pool, namespace }
    }
    /// Create the revocation table. Safe to run repeatedly.
    pub async fn initialize(&self) -> Result<(), Error> {
        sqlx::query("CREATE TABLE IF NOT EXISTS token_revocations (namespace TEXT NOT NULL, kind TEXT NOT NULL CHECK (kind IN ('token', 'subject')), identifier TEXT NOT NULL, value BIGINT NOT NULL, PRIMARY KEY(namespace, kind, identifier))")
            .execute(&self.pool).await.map_err(storage_error)?;
        Ok(())
    }
    async fn read(&self, kind: &str, identifier: &str) -> Result<Option<i64>, Error> {
        sqlx::query_scalar(
            "SELECT value FROM token_revocations WHERE namespace=$1 AND kind=$2 AND identifier=$3",
        )
        .bind(self.namespace.as_str())
        .bind(kind)
        .bind(identifier)
        .fetch_optional(&self.pool)
        .await
        .map_err(storage_error)
    }
    async fn write(&self, kind: &str, identifier: &str, value: i64) -> Result<(), Error> {
        sqlx::query("INSERT INTO token_revocations(namespace,kind,identifier,value) VALUES($1,$2,$3,$4) ON CONFLICT(namespace,kind,identifier) DO UPDATE SET value=GREATEST(token_revocations.value,EXCLUDED.value)")
            .bind(self.namespace.as_str()).bind(kind).bind(identifier).bind(value).execute(&self.pool).await.map_err(storage_error)?;
        Ok(())
    }
    /// Remove expired token records in this namespace. Subject cutoffs persist.
    pub async fn cleanup_expired(&self) -> Result<(), Error> {
        sqlx::query(
            "DELETE FROM token_revocations WHERE namespace=$1 AND kind='token' AND value <= $2",
        )
        .bind(self.namespace.as_str())
        .bind(chrono::Utc::now().timestamp())
        .execute(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(())
    }
}
fn storage_error(error: sqlx::Error) -> Error {
    Error::Internal(format!("PostgreSQL revocation storage: {error}"))
}
#[async_trait]
impl TokenRevocation for PgTokenRevocation {
    async fn initialize(&self) -> Result<(), Error> {
        PgTokenRevocation::initialize(self).await
    }
    async fn cleanup_expired(&self) -> Result<(), Error> {
        PgTokenRevocation::cleanup_expired(self).await
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
