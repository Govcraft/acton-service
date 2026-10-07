//! Persistent token-ID and subject revocation.
//!
//! Initialize database schemas with each backend's `initialize()` before serving.
//! Authentication fails closed while a configured pool or schema is unavailable.

use super::token::TokenRevocation;
use crate::error::Error;

#[cfg(feature = "cache")]
mod redis;
#[cfg(feature = "cache")]
pub use redis::RedisTokenRevocation;
#[cfg(feature = "database")]
mod postgres;
#[cfg(feature = "database")]
pub use postgres::PgTokenRevocation;
#[cfg(feature = "mssql")]
mod mssql;
#[cfg(feature = "mssql")]
pub use mssql::MssqlTokenRevocation;
#[cfg(feature = "turso")]
mod turso;
#[cfg(feature = "turso")]
pub use turso::TursoTokenRevocation;
#[cfg(feature = "surrealdb")]
mod surreal;
#[cfg(feature = "surrealdb")]
pub use surreal::SurrealTokenRevocation;
mod configured;
pub(crate) use configured::ConfiguredRevocation;

/// Namespace isolating services that share the same revocation database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevocationNamespace(String);

impl RevocationNamespace {
    /// Validate a nonempty namespace of at most 128 bytes.
    pub fn new(namespace: impl Into<String>) -> Result<Self, Error> {
        let namespace = namespace.into();
        if namespace.trim().is_empty() || namespace.len() > 128 {
            return Err(Error::Internal(
                "Revocation namespace must contain 1 to 128 bytes".into(),
            ));
        }
        Ok(Self(namespace))
    }
    /// Get the exact namespace used by storage.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RevocationNamespace {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}
impl std::str::FromStr for RevocationNamespace {
    type Err = Error;
    fn from_str(namespace: &str) -> Result<Self, Self::Err> {
        Self::new(namespace)
    }
}
impl AsRef<str> for RevocationNamespace {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[cfg(any(
    feature = "database",
    feature = "mssql",
    feature = "turso",
    feature = "surrealdb"
))]
fn expires_at(ttl_secs: u64) -> Result<i64, Error> {
    let ttl = i64::try_from(ttl_secs).map_err(|_| {
        Error::Internal("Revocation lifetime exceeds supported timestamp range".into())
    })?;
    chrono::Utc::now()
        .timestamp()
        .checked_add(ttl)
        .ok_or_else(|| {
            Error::Internal("Revocation expiration exceeds supported timestamp range".into())
        })
}
