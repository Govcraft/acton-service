use super::TokenRevocation;
use crate::error::Error;
use async_trait::async_trait;
use deadpool_redis::{
    redis::{AsyncCommands, Script},
    Pool,
};

/// Redis-backed token revocation with monotonic subject cutoffs.
///
/// Default token keys retain `token:revoked:{jti}`. The reserved jti
/// `subject-cutoffs` is refused to avoid colliding with the subject hash.
#[derive(Clone)]
pub struct RedisTokenRevocation {
    pool: Pool,
    key_prefix: String,
}
impl RedisTokenRevocation {
    /// Create a checker with the historical default key prefix.
    pub fn new(pool: Pool) -> Self {
        Self::with_prefix(pool, "token:revoked:")
    }
    /// Create a checker with a service-specific prefix.
    pub fn with_prefix(pool: Pool, prefix: impl Into<String>) -> Self {
        Self {
            pool,
            key_prefix: prefix.into(),
        }
    }
    fn token_key(&self, jti: &str) -> Result<String, Error> {
        if jti == "subject-cutoffs" {
            return Err(Error::Internal(
                "Token ID conflicts with the reserved Redis subject key".into(),
            ));
        }
        Ok(format!("{}{jti}", self.key_prefix))
    }
    fn subject_key(&self) -> String {
        format!("{}subject-cutoffs", self.key_prefix)
    }
}
fn storage_error(error: impl std::fmt::Display) -> Error {
    Error::Internal(format!("Redis revocation storage: {error}"))
}
#[async_trait]
impl TokenRevocation for RedisTokenRevocation {
    async fn is_revoked(&self, jti: &str) -> Result<bool, Error> {
        self.pool
            .get()
            .await
            .map_err(storage_error)?
            .exists(self.token_key(jti)?)
            .await
            .map_err(storage_error)
    }
    async fn revoke(&self, jti: &str, ttl_secs: u64) -> Result<(), Error> {
        let ttl = i64::try_from(ttl_secs)
            .ok()
            .and_then(|seconds| seconds.checked_mul(1000))
            .filter(|milliseconds| *milliseconds <= (1_i64 << 53))
            .ok_or_else(|| {
                Error::Internal(
                    "Redis revocation lifetime exceeds supported millisecond range".into(),
                )
            })?;
        let key = self.token_key(jti)?;
        if ttl == 0 {
            return Ok(());
        }
        let mut connection = self.pool.get().await.map_err(storage_error)?;
        Script::new("local ttl=redis.call('PTTL',KEYS[1]); if ttl == -2 or (ttl >= 0 and ttl < tonumber(ARGV[1])) then redis.call('SET',KEYS[1],'1','PX',ARGV[1]); end; return 1")
            .key(key).arg(ttl).invoke_async::<i64>(&mut *connection).await.map_err(storage_error)?;
        Ok(())
    }
    async fn subject_not_before(&self, subject: &str) -> Result<Option<i64>, Error> {
        self.pool
            .get()
            .await
            .map_err(storage_error)?
            .hget(self.subject_key(), subject)
            .await
            .map_err(storage_error)
    }
    async fn revoke_subject(&self, subject: &str, not_before: i64) -> Result<(), Error> {
        // Lua numbers represent these seconds exactly throughout the practical Unix range.
        if !(-(1_i64 << 53)..=(1_i64 << 53)).contains(&not_before) {
            return Err(Error::Internal(
                "Redis subject cutoff exceeds exact integer range".into(),
            ));
        }
        let mut connection = self.pool.get().await.map_err(storage_error)?;
        Script::new("local old=redis.call('HGET',KEYS[1],ARGV[1]); if not old or tonumber(old)<tonumber(ARGV[2]) then redis.call('HSET',KEYS[1],ARGV[1],ARGV[2]); end; return 1")
            .key(self.subject_key()).arg(subject).arg(not_before).invoke_async::<i64>(&mut *connection).await.map_err(storage_error)?;
        Ok(())
    }
}
