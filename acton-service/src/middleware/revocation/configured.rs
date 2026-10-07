use super::{RevocationNamespace, TokenRevocation};
use crate::{
    config::{Config, RevocationBackend, RevocationConfig},
    error::Error,
    state::AppState,
};
use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;

pub(crate) struct ConfiguredRevocation<T>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    state: AppState<T>,
    backend: RevocationBackend,
    namespace: RevocationNamespace,
}
impl<T> ConfiguredRevocation<T>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    pub(crate) fn new(
        config: &Config<T>,
        revocation: &RevocationConfig,
        mut state: AppState<T>,
    ) -> Result<Self, Error> {
        let enabled = match revocation.backend {
            RevocationBackend::Redis => cfg!(feature = "cache") && config.redis.is_some(),
            RevocationBackend::Postgres => cfg!(feature = "database") && config.database.is_some(),
            RevocationBackend::Mssql => cfg!(feature = "mssql") && config.database.is_some(),
            RevocationBackend::Turso => {
                #[cfg(feature = "turso")]
                {
                    config.turso.is_some()
                }
                #[cfg(not(feature = "turso"))]
                {
                    false
                }
            }
            RevocationBackend::Surrealdb => {
                #[cfg(feature = "surrealdb")]
                {
                    config.surrealdb.is_some()
                }
                #[cfg(not(feature = "surrealdb"))]
                {
                    false
                }
            }
        };
        if !enabled {
            return Err(Error::Internal(format!("Revocation backend {:?} requires its compiled feature and connection configuration",revocation.backend)));
        }
        // The snapshot shares only pool handles, never another installed checker.
        state.set_token_revocation(None);
        Ok(Self {
            state,
            backend: revocation.backend,
            namespace: RevocationNamespace::new(&revocation.namespace)?,
        })
    }
    async fn storage(&self) -> Result<Arc<dyn TokenRevocation>, Error> {
        let unavailable = || {
            Error::Internal(format!(
                "Revocation backend {:?} for service {} in namespace {} is unavailable; refusing protected request",
                self.backend, self.state.config().service.name, self.namespace.as_str()
            ))
        };
        match self.backend {
            #[cfg(feature = "cache")]
            RevocationBackend::Redis => Ok(Arc::new(super::RedisTokenRevocation::with_prefix(
                self.state.redis().await.ok_or_else(unavailable)?,
                format!(
                    "token:revoked:{}:{}:",
                    self.namespace.as_str().len(),
                    self.namespace.as_str()
                ),
            ))),
            #[cfg(feature = "database")]
            RevocationBackend::Postgres => Ok(Arc::new(super::PgTokenRevocation::new(
                self.state.db().await.ok_or_else(unavailable)?,
                self.namespace.clone(),
            ))),
            #[cfg(feature = "mssql")]
            RevocationBackend::Mssql => Ok(Arc::new(super::MssqlTokenRevocation::new(
                self.state.mssql().await.ok_or_else(unavailable)?,
                self.namespace.clone(),
            ))),
            #[cfg(feature = "turso")]
            RevocationBackend::Turso => Ok(Arc::new(super::TursoTokenRevocation::new(
                self.state.turso().await.ok_or_else(unavailable)?,
                self.namespace.clone(),
            ))),
            #[cfg(feature = "surrealdb")]
            RevocationBackend::Surrealdb => Ok(Arc::new(super::SurrealTokenRevocation::new(
                self.state.surrealdb().await.ok_or_else(unavailable)?,
                self.namespace.clone(),
            ))),
            _ => Err(unavailable()),
        }
    }
}
#[async_trait]
impl<T> TokenRevocation for ConfiguredRevocation<T>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    async fn check_claims(&self, claims: &crate::middleware::Claims) -> Result<bool, Error> {
        self.storage().await?.check_claims(claims).await
    }
    async fn initialize(&self) -> Result<(), Error> {
        self.storage().await?.initialize().await
    }
    async fn cleanup_expired(&self) -> Result<(), Error> {
        self.storage().await?.cleanup_expired().await
    }
    async fn is_revoked(&self, jti: &str) -> Result<bool, Error> {
        self.storage().await?.is_revoked(jti).await
    }
    async fn revoke(&self, jti: &str, ttl_secs: u64) -> Result<(), Error> {
        self.storage().await?.revoke(jti, ttl_secs).await
    }
    async fn subject_not_before(&self, subject: &str) -> Result<Option<i64>, Error> {
        self.storage().await?.subject_not_before(subject).await
    }
    async fn revoke_subject(&self, subject: &str, not_before: i64) -> Result<(), Error> {
        self.storage()
            .await?
            .revoke_subject(subject, not_before)
            .await
    }
}
