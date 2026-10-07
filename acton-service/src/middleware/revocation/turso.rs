use super::{expires_at, RevocationNamespace, TokenRevocation};
use crate::error::Error;
use async_trait::async_trait;
use std::sync::Arc;

/// Turso/libsql-backed revocation, scoped by namespace.
#[derive(Clone)]
pub struct TursoTokenRevocation {
    db: Arc<libsql::Database>,
    namespace: RevocationNamespace,
}
impl TursoTokenRevocation {
    /// Create a checker using an existing database.
    pub fn new(db: Arc<libsql::Database>, namespace: RevocationNamespace) -> Self {
        Self { db, namespace }
    }
    fn connect(&self) -> Result<libsql::Connection, Error> {
        self.db.connect().map_err(storage_error)
    }
    /// Create the revocation table. Safe to run repeatedly.
    pub async fn initialize(&self) -> Result<(), Error> {
        self.connect()?.execute("CREATE TABLE IF NOT EXISTS token_revocations (namespace TEXT NOT NULL, kind TEXT NOT NULL CHECK (kind IN ('token', 'subject')), identifier TEXT NOT NULL, value INTEGER NOT NULL, PRIMARY KEY(namespace, kind, identifier))",()).await.map_err(storage_error)?;
        Ok(())
    }
    async fn read(&self, kind: &str, identifier: &str) -> Result<Option<i64>, Error> {
        let mut rows=self.connect()?.query("SELECT value FROM token_revocations WHERE namespace=?1 AND kind=?2 AND identifier=?3",libsql::params![self.namespace.as_str(),kind,identifier]).await.map_err(storage_error)?;
        rows.next()
            .await
            .map_err(storage_error)?
            .map(|row| row.get(0).map_err(storage_error))
            .transpose()
    }
    async fn write(&self, kind: &str, identifier: &str, value: i64) -> Result<(), Error> {
        self.connect()?.execute("INSERT INTO token_revocations(namespace,kind,identifier,value) VALUES(?1,?2,?3,?4) ON CONFLICT(namespace,kind,identifier) DO UPDATE SET value=MAX(token_revocations.value,excluded.value)",libsql::params![self.namespace.as_str(),kind,identifier,value]).await.map_err(storage_error)?;
        Ok(())
    }
    /// Remove expired token records in this namespace. Subject cutoffs persist.
    pub async fn cleanup_expired(&self) -> Result<(), Error> {
        self.connect()?
            .execute(
                "DELETE FROM token_revocations WHERE namespace=?1 AND kind='token' AND value <= ?2",
                libsql::params![self.namespace.as_str(), chrono::Utc::now().timestamp()],
            )
            .await
            .map_err(storage_error)?;
        Ok(())
    }
}
fn storage_error(error: libsql::Error) -> Error {
    Error::Internal(format!("Turso revocation storage: {error}"))
}
#[async_trait]
impl TokenRevocation for TursoTokenRevocation {
    async fn initialize(&self) -> Result<(), Error> {
        TursoTokenRevocation::initialize(self).await
    }
    async fn cleanup_expired(&self) -> Result<(), Error> {
        TursoTokenRevocation::cleanup_expired(self).await
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
