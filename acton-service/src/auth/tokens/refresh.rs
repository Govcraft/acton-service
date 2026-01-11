//! Refresh token storage and management
//!
//! Provides refresh token storage with rotation and reuse detection.
//! Implementations are available for Redis, PostgreSQL, and Turso.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::Error;

/// Refresh token metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshTokenMetadata {
    /// User agent string from the request
    pub user_agent: Option<String>,

    /// Client IP address
    pub ip_address: Option<String>,

    /// Device identifier (for mobile apps)
    pub device_id: Option<String>,

    /// When the token was created
    pub created_at: DateTime<Utc>,
}

impl Default for RefreshTokenMetadata {
    fn default() -> Self {
        Self {
            user_agent: None,
            ip_address: None,
            device_id: None,
            created_at: Utc::now(),
        }
    }
}

/// Refresh token data stored in the backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshTokenData {
    /// Token ID (jti)
    pub token_id: String,

    /// User ID this token belongs to
    pub user_id: String,

    /// Token family ID for rotation tracking
    pub family_id: String,

    /// Whether this token has been revoked
    pub is_revoked: bool,

    /// When this token expires
    pub expires_at: DateTime<Utc>,

    /// Token metadata
    pub metadata: RefreshTokenMetadata,
}

/// Refresh token storage trait
///
/// Implementations of this trait provide storage for refresh tokens
/// with support for rotation and reuse detection.
#[async_trait]
pub trait RefreshTokenStorage: Send + Sync {
    /// Store a new refresh token
    async fn store(
        &self,
        token_id: &str,
        user_id: &str,
        family_id: &str,
        expires_at: DateTime<Utc>,
        metadata: &RefreshTokenMetadata,
    ) -> Result<(), Error>;

    /// Get refresh token data by token ID
    async fn get(&self, token_id: &str) -> Result<Option<RefreshTokenData>, Error>;

    /// Revoke a specific refresh token
    async fn revoke(&self, token_id: &str) -> Result<(), Error>;

    /// Revoke all tokens in a family (for reuse detection)
    async fn revoke_family(&self, family_id: &str) -> Result<u64, Error>;

    /// Revoke all tokens for a user
    async fn revoke_all_for_user(&self, user_id: &str) -> Result<u64, Error>;

    /// Rotate a token: atomically revoke old and create new
    async fn rotate(
        &self,
        old_token_id: &str,
        new_token_id: &str,
        user_id: &str,
        family_id: &str,
        expires_at: DateTime<Utc>,
        metadata: &RefreshTokenMetadata,
    ) -> Result<(), Error>;

    /// Clean up expired tokens
    async fn cleanup_expired(&self) -> Result<u64, Error>;
}

/// Redis-based refresh token storage
///
/// Uses Redis for fast token lookups with automatic TTL-based expiration.
#[cfg(feature = "cache")]
pub mod redis_storage {
    use super::*;
    use deadpool_redis::Pool;
    use redis::AsyncCommands;

    /// Redis-backed refresh token storage
    #[derive(Clone)]
    pub struct RedisRefreshStorage {
        pool: Pool,
        key_prefix: String,
    }

    impl RedisRefreshStorage {
        /// Create a new Redis refresh token storage
        pub fn new(pool: Pool) -> Self {
            Self {
                pool,
                key_prefix: "refresh_token".to_string(),
            }
        }

        /// Create with a custom key prefix
        pub fn with_prefix(pool: Pool, prefix: impl Into<String>) -> Self {
            Self {
                pool,
                key_prefix: prefix.into(),
            }
        }

        fn token_key(&self, token_id: &str) -> String {
            format!("{}:{}", self.key_prefix, token_id)
        }

        fn family_key(&self, family_id: &str) -> String {
            format!("{}:family:{}", self.key_prefix, family_id)
        }

        fn user_key(&self, user_id: &str) -> String {
            format!("{}:user:{}", self.key_prefix, user_id)
        }
    }

    #[async_trait]
    impl RefreshTokenStorage for RedisRefreshStorage {
        async fn store(
            &self,
            token_id: &str,
            user_id: &str,
            family_id: &str,
            expires_at: DateTime<Utc>,
            metadata: &RefreshTokenMetadata,
        ) -> Result<(), Error> {
            let mut conn = self.pool.get().await.map_err(|e| {
                Error::Internal(format!("Failed to get Redis connection: {}", e))
            })?;

            let data = RefreshTokenData {
                token_id: token_id.to_string(),
                user_id: user_id.to_string(),
                family_id: family_id.to_string(),
                is_revoked: false,
                expires_at,
                metadata: metadata.clone(),
            };

            let json = serde_json::to_string(&data)
                .map_err(|e| Error::Internal(format!("Failed to serialize token: {}", e)))?;

            let ttl = (expires_at - Utc::now()).num_seconds().max(1) as u64;

            // Store the token data with TTL
            let token_key = self.token_key(token_id);
            conn.set_ex::<_, _, ()>(&token_key, &json, ttl)
                .await
                .map_err(|e| Error::Internal(format!("Failed to store refresh token: {}", e)))?;

            // Add to family set for batch revocation
            let family_key = self.family_key(family_id);
            conn.sadd::<_, _, ()>(&family_key, token_id)
                .await
                .map_err(|e| Error::Internal(format!("Failed to add to family set: {}", e)))?;
            conn.expire::<_, ()>(&family_key, ttl as i64)
                .await
                .map_err(|e| Error::Internal(format!("Failed to set family TTL: {}", e)))?;

            // Add to user set for user-level revocation
            let user_key = self.user_key(user_id);
            conn.sadd::<_, _, ()>(&user_key, token_id)
                .await
                .map_err(|e| Error::Internal(format!("Failed to add to user set: {}", e)))?;
            conn.expire::<_, ()>(&user_key, ttl as i64)
                .await
                .map_err(|e| Error::Internal(format!("Failed to set user TTL: {}", e)))?;

            Ok(())
        }

        async fn get(&self, token_id: &str) -> Result<Option<RefreshTokenData>, Error> {
            let mut conn = self.pool.get().await.map_err(|e| {
                Error::Internal(format!("Failed to get Redis connection: {}", e))
            })?;

            let key = self.token_key(token_id);
            let json: Option<String> = conn
                .get(&key)
                .await
                .map_err(|e| Error::Internal(format!("Failed to get refresh token: {}", e)))?;

            match json {
                Some(j) => {
                    let data: RefreshTokenData = serde_json::from_str(&j)
                        .map_err(|e| Error::Internal(format!("Failed to parse token: {}", e)))?;
                    Ok(Some(data))
                }
                None => Ok(None),
            }
        }

        async fn revoke(&self, token_id: &str) -> Result<(), Error> {
            let mut conn = self.pool.get().await.map_err(|e| {
                Error::Internal(format!("Failed to get Redis connection: {}", e))
            })?;

            // Get the token first to mark it as revoked
            if let Some(mut data) = self.get(token_id).await? {
                data.is_revoked = true;
                let json = serde_json::to_string(&data)
                    .map_err(|e| Error::Internal(format!("Failed to serialize token: {}", e)))?;

                let ttl = (data.expires_at - Utc::now()).num_seconds().max(1) as u64;
                let key = self.token_key(token_id);
                conn.set_ex::<_, _, ()>(&key, &json, ttl)
                    .await
                    .map_err(|e| Error::Internal(format!("Failed to revoke token: {}", e)))?;
            }

            Ok(())
        }

        async fn revoke_family(&self, family_id: &str) -> Result<u64, Error> {
            let mut conn = self.pool.get().await.map_err(|e| {
                Error::Internal(format!("Failed to get Redis connection: {}", e))
            })?;

            let family_key = self.family_key(family_id);
            let token_ids: Vec<String> = conn
                .smembers(&family_key)
                .await
                .map_err(|e| Error::Internal(format!("Failed to get family members: {}", e)))?;

            let mut revoked = 0u64;
            for token_id in &token_ids {
                self.revoke(token_id).await?;
                revoked += 1;
            }

            Ok(revoked)
        }

        async fn revoke_all_for_user(&self, user_id: &str) -> Result<u64, Error> {
            let mut conn = self.pool.get().await.map_err(|e| {
                Error::Internal(format!("Failed to get Redis connection: {}", e))
            })?;

            let user_key = self.user_key(user_id);
            let token_ids: Vec<String> = conn
                .smembers(&user_key)
                .await
                .map_err(|e| Error::Internal(format!("Failed to get user tokens: {}", e)))?;

            let mut revoked = 0u64;
            for token_id in &token_ids {
                self.revoke(token_id).await?;
                revoked += 1;
            }

            Ok(revoked)
        }

        async fn rotate(
            &self,
            old_token_id: &str,
            new_token_id: &str,
            user_id: &str,
            family_id: &str,
            expires_at: DateTime<Utc>,
            metadata: &RefreshTokenMetadata,
        ) -> Result<(), Error> {
            // Revoke the old token
            self.revoke(old_token_id).await?;

            // Store the new token
            self.store(new_token_id, user_id, family_id, expires_at, metadata)
                .await?;

            Ok(())
        }

        async fn cleanup_expired(&self) -> Result<u64, Error> {
            // Redis handles expiration automatically via TTL
            // This method is a no-op for Redis but required by the trait
            Ok(0)
        }
    }
}

#[cfg(feature = "cache")]
pub use redis_storage::RedisRefreshStorage;

/// PostgreSQL-based refresh token storage
#[cfg(feature = "database")]
pub mod pg_storage {
    use super::*;
    use sqlx::PgPool;

    /// PostgreSQL-backed refresh token storage
    #[derive(Clone)]
    pub struct PgRefreshStorage {
        pool: PgPool,
    }

    impl PgRefreshStorage {
        /// Create a new PostgreSQL refresh token storage
        pub fn new(pool: PgPool) -> Self {
            Self { pool }
        }
    }

    #[async_trait]
    impl RefreshTokenStorage for PgRefreshStorage {
        async fn store(
            &self,
            token_id: &str,
            user_id: &str,
            family_id: &str,
            expires_at: DateTime<Utc>,
            metadata: &RefreshTokenMetadata,
        ) -> Result<(), Error> {
            let metadata_json = serde_json::to_value(metadata)
                .map_err(|e| Error::Internal(format!("Failed to serialize metadata: {}", e)))?;

            sqlx::query(
                r#"
                INSERT INTO refresh_tokens (id, user_id, family_id, expires_at, metadata, is_revoked)
                VALUES ($1, $2, $3, $4, $5, false)
                "#,
            )
            .bind(token_id)
            .bind(user_id)
            .bind(family_id)
            .bind(expires_at)
            .bind(metadata_json)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Internal(format!("Failed to store refresh token: {}", e)))?;

            Ok(())
        }

        async fn get(&self, token_id: &str) -> Result<Option<RefreshTokenData>, Error> {
            let row = sqlx::query_as::<_, (String, String, String, bool, DateTime<Utc>, serde_json::Value)>(
                r#"
                SELECT id, user_id, family_id, is_revoked, expires_at, metadata
                FROM refresh_tokens
                WHERE id = $1 AND expires_at > NOW()
                "#,
            )
            .bind(token_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| Error::Internal(format!("Failed to get refresh token: {}", e)))?;

            match row {
                Some((id, user_id, family_id, is_revoked, expires_at, metadata_json)) => {
                    let metadata: RefreshTokenMetadata = serde_json::from_value(metadata_json)
                        .unwrap_or_default();
                    Ok(Some(RefreshTokenData {
                        token_id: id,
                        user_id,
                        family_id,
                        is_revoked,
                        expires_at,
                        metadata,
                    }))
                }
                None => Ok(None),
            }
        }

        async fn revoke(&self, token_id: &str) -> Result<(), Error> {
            sqlx::query("UPDATE refresh_tokens SET is_revoked = true WHERE id = $1")
                .bind(token_id)
                .execute(&self.pool)
                .await
                .map_err(|e| Error::Internal(format!("Failed to revoke token: {}", e)))?;

            Ok(())
        }

        async fn revoke_family(&self, family_id: &str) -> Result<u64, Error> {
            let result = sqlx::query(
                "UPDATE refresh_tokens SET is_revoked = true WHERE family_id = $1",
            )
            .bind(family_id)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Internal(format!("Failed to revoke family: {}", e)))?;

            Ok(result.rows_affected())
        }

        async fn revoke_all_for_user(&self, user_id: &str) -> Result<u64, Error> {
            let result = sqlx::query(
                "UPDATE refresh_tokens SET is_revoked = true WHERE user_id = $1",
            )
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Internal(format!("Failed to revoke user tokens: {}", e)))?;

            Ok(result.rows_affected())
        }

        async fn rotate(
            &self,
            old_token_id: &str,
            new_token_id: &str,
            user_id: &str,
            family_id: &str,
            expires_at: DateTime<Utc>,
            metadata: &RefreshTokenMetadata,
        ) -> Result<(), Error> {
            // Use a transaction for atomic rotation
            let mut tx = self.pool.begin().await.map_err(|e| {
                Error::Internal(format!("Failed to begin transaction: {}", e))
            })?;

            // Revoke old token
            sqlx::query("UPDATE refresh_tokens SET is_revoked = true WHERE id = $1")
                .bind(old_token_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| Error::Internal(format!("Failed to revoke old token: {}", e)))?;

            // Insert new token
            let metadata_json = serde_json::to_value(metadata)
                .map_err(|e| Error::Internal(format!("Failed to serialize metadata: {}", e)))?;

            sqlx::query(
                r#"
                INSERT INTO refresh_tokens (id, user_id, family_id, expires_at, metadata, is_revoked)
                VALUES ($1, $2, $3, $4, $5, false)
                "#,
            )
            .bind(new_token_id)
            .bind(user_id)
            .bind(family_id)
            .bind(expires_at)
            .bind(metadata_json)
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::Internal(format!("Failed to store new token: {}", e)))?;

            tx.commit().await.map_err(|e| {
                Error::Internal(format!("Failed to commit transaction: {}", e))
            })?;

            Ok(())
        }

        async fn cleanup_expired(&self) -> Result<u64, Error> {
            let result = sqlx::query("DELETE FROM refresh_tokens WHERE expires_at < NOW()")
                .execute(&self.pool)
                .await
                .map_err(|e| Error::Internal(format!("Failed to cleanup expired tokens: {}", e)))?;

            Ok(result.rows_affected())
        }
    }
}

#[cfg(feature = "database")]
pub use pg_storage::PgRefreshStorage;

/// Turso/libsql-based refresh token storage
#[cfg(feature = "turso")]
pub mod turso_storage {
    use super::*;
    use libsql::Connection;
    use std::sync::Arc;

    /// Turso-backed refresh token storage
    #[derive(Clone)]
    pub struct TursoRefreshStorage {
        conn: Arc<Connection>,
    }

    impl TursoRefreshStorage {
        /// Create a new Turso refresh token storage
        pub fn new(conn: Arc<Connection>) -> Self {
            Self { conn }
        }
    }

    #[async_trait]
    impl RefreshTokenStorage for TursoRefreshStorage {
        async fn store(
            &self,
            token_id: &str,
            user_id: &str,
            family_id: &str,
            expires_at: DateTime<Utc>,
            metadata: &RefreshTokenMetadata,
        ) -> Result<(), Error> {
            let metadata_json = serde_json::to_string(metadata)
                .map_err(|e| Error::Internal(format!("Failed to serialize metadata: {}", e)))?;

            self.conn
                .execute(
                    "INSERT INTO refresh_tokens (id, user_id, family_id, expires_at, metadata, is_revoked) VALUES (?1, ?2, ?3, ?4, ?5, 0)",
                    libsql::params![token_id, user_id, family_id, expires_at.to_rfc3339(), metadata_json],
                )
                .await
                .map_err(|e| Error::Internal(format!("Failed to store refresh token: {}", e)))?;

            Ok(())
        }

        async fn get(&self, token_id: &str) -> Result<Option<RefreshTokenData>, Error> {
            let mut rows = self
                .conn
                .query(
                    "SELECT id, user_id, family_id, is_revoked, expires_at, metadata FROM refresh_tokens WHERE id = ?1 AND expires_at > datetime('now')",
                    libsql::params![token_id],
                )
                .await
                .map_err(|e| Error::Internal(format!("Failed to get refresh token: {}", e)))?;

            if let Some(row) = rows.next().await.map_err(|e| {
                Error::Internal(format!("Failed to fetch row: {}", e))
            })? {
                let id: String = row.get(0).map_err(|e| {
                    Error::Internal(format!("Failed to get id: {}", e))
                })?;
                let user_id: String = row.get(1).map_err(|e| {
                    Error::Internal(format!("Failed to get user_id: {}", e))
                })?;
                let family_id: String = row.get(2).map_err(|e| {
                    Error::Internal(format!("Failed to get family_id: {}", e))
                })?;
                let is_revoked: i64 = row.get(3).map_err(|e| {
                    Error::Internal(format!("Failed to get is_revoked: {}", e))
                })?;
                let expires_at_str: String = row.get(4).map_err(|e| {
                    Error::Internal(format!("Failed to get expires_at: {}", e))
                })?;
                let metadata_str: String = row.get(5).map_err(|e| {
                    Error::Internal(format!("Failed to get metadata: {}", e))
                })?;

                let expires_at = DateTime::parse_from_rfc3339(&expires_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());

                let metadata: RefreshTokenMetadata =
                    serde_json::from_str(&metadata_str).unwrap_or_default();

                Ok(Some(RefreshTokenData {
                    token_id: id,
                    user_id,
                    family_id,
                    is_revoked: is_revoked != 0,
                    expires_at,
                    metadata,
                }))
            } else {
                Ok(None)
            }
        }

        async fn revoke(&self, token_id: &str) -> Result<(), Error> {
            self.conn
                .execute(
                    "UPDATE refresh_tokens SET is_revoked = 1 WHERE id = ?1",
                    libsql::params![token_id],
                )
                .await
                .map_err(|e| Error::Internal(format!("Failed to revoke token: {}", e)))?;

            Ok(())
        }

        async fn revoke_family(&self, family_id: &str) -> Result<u64, Error> {
            let rows = self
                .conn
                .execute(
                    "UPDATE refresh_tokens SET is_revoked = 1 WHERE family_id = ?1",
                    libsql::params![family_id],
                )
                .await
                .map_err(|e| Error::Internal(format!("Failed to revoke family: {}", e)))?;

            Ok(rows)
        }

        async fn revoke_all_for_user(&self, user_id: &str) -> Result<u64, Error> {
            let rows = self
                .conn
                .execute(
                    "UPDATE refresh_tokens SET is_revoked = 1 WHERE user_id = ?1",
                    libsql::params![user_id],
                )
                .await
                .map_err(|e| Error::Internal(format!("Failed to revoke user tokens: {}", e)))?;

            Ok(rows)
        }

        async fn rotate(
            &self,
            old_token_id: &str,
            new_token_id: &str,
            user_id: &str,
            family_id: &str,
            expires_at: DateTime<Utc>,
            metadata: &RefreshTokenMetadata,
        ) -> Result<(), Error> {
            // Turso doesn't support transactions in the same way, so we do sequential ops
            self.revoke(old_token_id).await?;
            self.store(new_token_id, user_id, family_id, expires_at, metadata)
                .await?;

            Ok(())
        }

        async fn cleanup_expired(&self) -> Result<u64, Error> {
            let rows = self
                .conn
                .execute(
                    "DELETE FROM refresh_tokens WHERE expires_at < datetime('now')",
                    (),
                )
                .await
                .map_err(|e| Error::Internal(format!("Failed to cleanup expired tokens: {}", e)))?;

            Ok(rows)
        }
    }
}

#[cfg(feature = "turso")]
pub use turso_storage::TursoRefreshStorage;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_refresh_token_metadata_default() {
        let metadata = RefreshTokenMetadata::default();
        assert!(metadata.user_agent.is_none());
        assert!(metadata.ip_address.is_none());
        assert!(metadata.device_id.is_none());
    }

    #[test]
    fn test_refresh_token_data_serialization() {
        let data = RefreshTokenData {
            token_id: "token123".to_string(),
            user_id: "user456".to_string(),
            family_id: "family789".to_string(),
            is_revoked: false,
            expires_at: Utc::now(),
            metadata: RefreshTokenMetadata::default(),
        };

        let json = serde_json::to_string(&data).expect("Failed to serialize");
        let deserialized: RefreshTokenData =
            serde_json::from_str(&json).expect("Failed to deserialize");

        assert_eq!(data.token_id, deserialized.token_id);
        assert_eq!(data.user_id, deserialized.user_id);
        assert_eq!(data.family_id, deserialized.family_id);
        assert_eq!(data.is_revoked, deserialized.is_revoked);
    }
}
