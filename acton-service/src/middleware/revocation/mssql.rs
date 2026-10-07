use super::{expires_at, RevocationNamespace, TokenRevocation};
use crate::{
    error::Error,
    mssql::{execute, query, MssqlPool},
};
use async_trait::async_trait;

/// SQL Server-backed revocation, scoped by namespace.
#[derive(Clone)]
pub struct MssqlTokenRevocation {
    pool: MssqlPool,
    namespace: RevocationNamespace,
}
impl MssqlTokenRevocation {
    /// Create a checker using an existing pool.
    pub fn new(pool: MssqlPool, namespace: RevocationNamespace) -> Self {
        Self { pool, namespace }
    }
    /// Create the revocation table. Safe to run repeatedly.
    pub async fn initialize(&self) -> Result<(), Error> {
        execute(&self.pool,"IF OBJECT_ID(N'token_revocations',N'U') IS NULL CREATE TABLE token_revocations (namespace VARBINARY(129) NOT NULL,kind NVARCHAR(16) COLLATE Latin1_General_100_BIN2 NOT NULL CHECK (kind IN ('token','subject')),identifier VARBINARY(513) NOT NULL,value BIGINT NOT NULL,PRIMARY KEY(namespace,kind,identifier))",&[]).await?;
        Ok(())
    }
    async fn read(&self, kind: &str, identifier: &str) -> Result<Option<i64>, Error> {
        let namespace = terminated_key(self.namespace.as_str());
        let identifier = identifier_key(identifier)?;
        let rows=query(&self.pool,"SELECT value FROM token_revocations WHERE namespace=@P1 AND kind=@P2 AND identifier=@P3",&[&namespace.as_slice(),&kind,&identifier.as_slice()]).await?;
        rows.first()
            .map(|row| {
                row.get("value").ok_or_else(|| {
                    Error::Internal("SQL Server revocation returned a NULL timestamp".into())
                })
            })
            .transpose()
    }
    async fn write(&self, kind: &str, identifier: &str, value: i64) -> Result<(), Error> {
        let namespace = terminated_key(self.namespace.as_str());
        let identifier = identifier_key(identifier)?;
        execute(&self.pool,"MERGE token_revocations WITH (HOLDLOCK) AS target USING (SELECT @P1 AS namespace,@P2 AS kind,@P3 AS identifier,@P4 AS value) AS source ON target.namespace=source.namespace AND target.kind=source.kind AND target.identifier=source.identifier WHEN MATCHED AND target.value < source.value THEN UPDATE SET value=source.value WHEN NOT MATCHED THEN INSERT(namespace,kind,identifier,value) VALUES(source.namespace,source.kind,source.identifier,source.value);",&[&namespace.as_slice(),&kind,&identifier.as_slice(),&value]).await?;
        Ok(())
    }
    /// Remove expired token records in this namespace. Subject cutoffs persist.
    pub async fn cleanup_expired(&self) -> Result<(), Error> {
        let namespace = terminated_key(self.namespace.as_str());
        execute(
            &self.pool,
            "DELETE FROM token_revocations WHERE namespace=@P1 AND kind='token' AND value <= @P2",
            &[&namespace.as_slice(), &chrono::Utc::now().timestamp()],
        )
        .await?;
        Ok(())
    }
}
// SQL Server pads even VARBINARY comparisons with zero bytes. A nonzero
// terminator makes opaque UTF-8 strings injective under those comparisons.
fn terminated_key(value: &str) -> Vec<u8> {
    let mut key = value.as_bytes().to_vec();
    key.push(1);
    key
}
fn identifier_key(identifier: &str) -> Result<Vec<u8>, Error> {
    if identifier.len() > 512 {
        return Err(Error::Internal(
            "SQL Server revocation identifiers must contain at most 512 UTF-8 bytes".into(),
        ));
    }
    Ok(terminated_key(identifier))
}

#[async_trait]
impl TokenRevocation for MssqlTokenRevocation {
    async fn initialize(&self) -> Result<(), Error> {
        MssqlTokenRevocation::initialize(self).await
    }
    async fn cleanup_expired(&self) -> Result<(), Error> {
        MssqlTokenRevocation::cleanup_expired(self).await
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
