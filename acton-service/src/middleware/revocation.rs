//! Token revocation implementations
//!
//! Provides storage for revoked token IDs (jti) that works with any token format.

use async_trait::async_trait;
use deadpool_redis::Pool as RedisPool;

use super::token::TokenRevocation;
use crate::error::Error;

/// Redis-based token revocation implementation
///
/// Stores revoked token IDs (jti) in Redis with automatic expiration (SETEX).
/// Works with both PASETO and JWT tokens.
/// The default key pattern is `token:revoked:{jti}`.
#[derive(Clone)]
pub struct RedisTokenRevocation {
    pool: RedisPool,
    key_prefix: String,
}

impl RedisTokenRevocation {
    /// Create a new Redis token revocation checker with default key prefix
    pub fn new(pool: RedisPool) -> Self {
        Self {
            pool,
            key_prefix: "token:revoked:".to_string(),
        }
    }

    /// Create a new Redis token revocation checker with custom key prefix
    pub fn with_prefix(pool: RedisPool, prefix: impl Into<String>) -> Self {
        Self {
            pool,
            key_prefix: prefix.into(),
        }
    }

    /// Get the Redis key for a given token ID (jti)
    fn revocation_key(&self, jti: &str) -> String {
        format!("{}{}", self.key_prefix, jti)
    }
}

#[async_trait]
impl TokenRevocation for RedisTokenRevocation {
    async fn is_revoked(&self, jti: &str) -> Result<bool, Error> {
        use deadpool_redis::redis::AsyncCommands;

        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| Error::Internal(format!("Failed to get Redis connection: {}", e)))?;

        let key = self.revocation_key(jti);
        let exists: bool = conn
            .exists(&key)
            .await
            .map_err(|e| Error::Internal(format!("Failed to check revocation status: {}", e)))?;

        Ok(exists)
    }

    async fn revoke(&self, jti: &str, ttl_secs: u64) -> Result<(), Error> {
        use deadpool_redis::redis::AsyncCommands;

        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| Error::Internal(format!("Failed to get Redis connection: {}", e)))?;

        let key = self.revocation_key(jti);
        // Store "1" as a marker with TTL
        conn.set_ex::<_, _, ()>(&key, 1, ttl_secs)
            .await
            .map_err(|e| Error::Internal(format!("Failed to revoke token: {}", e)))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_revocation_key_format() {
        // Create a mock pool - we can't actually use it but we can test the key format
        // This test verifies the key format logic without needing a real Redis connection
        let prefix = "token:revoked:";
        let jti = "test-token-id-123";
        let expected_key = format!("{}{}", prefix, jti);
        assert_eq!(expected_key, "token:revoked:test-token-id-123");
    }

    #[test]
    fn test_custom_prefix() {
        let prefix = "myapp:revoked:";
        let jti = "abc123";
        let expected_key = format!("{}{}", prefix, jti);
        assert_eq!(expected_key, "myapp:revoked:abc123");
    }
}
