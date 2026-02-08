//! API key authentication module
//!
//! Provides API key generation, validation, and storage for service-to-service
//! authentication. API keys follow the format: `{prefix}_{random_base32}`.
//!
//! # Example
//!
//! ```rust,ignore
//! use acton_service::auth::{ApiKeyGenerator, ApiKey};
//!
//! let generator = ApiKeyGenerator::new("sk_live");
//!
//! // Generate a new API key
//! let (key, key_hash) = generator.generate();
//! // key = "sk_live_abc123..." (show to user once)
//! // key_hash = "$argon2id$..." (store in database)
//!
//! // Later, verify an incoming key
//! if generator.verify(&incoming_key, &stored_hash)? {
//!     // Key is valid
//! }
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::auth::password::PasswordHasher;
use crate::error::Error;

/// API key structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    /// Database ID
    pub id: String,

    /// User/owner ID
    pub user_id: String,

    /// User-provided name for the key
    pub name: String,

    /// Key prefix (e.g., "sk_live")
    pub prefix: String,

    /// Hashed key value (stored, not the actual key)
    pub key_hash: String,

    /// Allowed scopes/permissions
    #[serde(default)]
    pub scopes: Vec<String>,

    /// Rate limit (requests per minute, None = default)
    pub rate_limit: Option<u32>,

    /// Whether this key has been revoked
    #[serde(default)]
    pub is_revoked: bool,

    /// When this key was last used
    pub last_used_at: Option<DateTime<Utc>>,

    /// When this key expires (None = never)
    pub expires_at: Option<DateTime<Utc>>,

    /// When this key was created
    pub created_at: DateTime<Utc>,
}

impl ApiKey {
    /// Check if the key is currently valid (not revoked, not expired)
    pub fn is_valid(&self) -> bool {
        if self.is_revoked {
            return false;
        }

        if let Some(expires_at) = self.expires_at {
            if expires_at < Utc::now() {
                return false;
            }
        }

        true
    }

    /// Check if the key has a specific scope
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }
}

/// API key generator
///
/// Generates API keys in the format `{prefix}_{random_base32}`.
/// Keys are hashed using Argon2id before storage.
#[derive(Clone)]
pub struct ApiKeyGenerator {
    prefix: String,
    hasher: PasswordHasher,
}

impl ApiKeyGenerator {
    /// Create a new API key generator with the given prefix
    ///
    /// # Arguments
    ///
    /// * `prefix` - Key prefix (e.g., "sk_live", "sk_test", "acton")
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            hasher: PasswordHasher::default(),
        }
    }

    /// Generate a new API key
    ///
    /// Returns a tuple of (key, hash) where:
    /// - `key` is the plaintext key to show to the user (once!)
    /// - `hash` is the Argon2id hash to store in the database
    pub fn generate(&self) -> (String, String) {
        // Generate 24 random bytes (192 bits of entropy)
        let random_bytes: [u8; 24] = rand::random();

        // Encode as base32 (no padding, lowercase)
        let encoded = base32_encode(&random_bytes);

        // Create the full key
        let key = format!("{}_{}", self.prefix, encoded);

        // Hash the key for storage
        // Note: We use a custom hasher config with lower memory for API keys
        // since they have high entropy and don't need the same protection as passwords
        let hash = self.hasher.hash(&key).expect("Failed to hash API key");

        (key, hash)
    }

    /// Verify an API key against a stored hash
    pub fn verify(&self, key: &str, hash: &str) -> Result<bool, Error> {
        self.hasher.verify(key, hash)
    }

    /// Extract the prefix from a key
    pub fn extract_prefix(key: &str) -> Option<&str> {
        key.split('_').next()
    }

    /// Get the first few characters of a key for lookup
    /// (useful for indexing without storing the full key)
    pub fn key_prefix_for_lookup(key: &str) -> Option<String> {
        // Return the prefix + first 8 chars of the random part
        // Key format is "{prefix}_{random}" where prefix can contain underscores (e.g., "sk_live")
        // So we split from the right to find the random part
        let parts: Vec<&str> = key.rsplitn(2, '_').collect();
        if parts.len() == 2 && parts[0].len() >= 8 {
            // parts[0] is the random part, parts[1] is the prefix
            Some(format!("{}_{}", parts[1], &parts[0][..8]))
        } else {
            None
        }
    }
}

/// Encode bytes as lowercase base32 without padding
fn base32_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";

    let mut result = String::with_capacity((bytes.len() * 8).div_ceil(5));
    let mut buffer = 0u64;
    let mut bits = 0;

    for &byte in bytes {
        buffer = (buffer << 8) | byte as u64;
        bits += 8;

        while bits >= 5 {
            bits -= 5;
            let index = ((buffer >> bits) & 0x1f) as usize;
            result.push(ALPHABET[index] as char);
        }
    }

    if bits > 0 {
        let index = ((buffer << (5 - bits)) & 0x1f) as usize;
        result.push(ALPHABET[index] as char);
    }

    result
}

use async_trait::async_trait;

/// API key storage trait
///
/// Implementations of this trait provide storage for API keys.
#[async_trait]
pub trait ApiKeyStorage: Send + Sync {
    /// Get an API key by the full key value (for verification)
    async fn get_by_key(&self, key: &str) -> Result<Option<ApiKey>, Error>;

    /// Get an API key by key prefix (for quick lookup)
    async fn get_by_prefix(&self, prefix: &str) -> Result<Option<ApiKey>, Error>;

    /// Get an API key by its database ID
    async fn get_by_id(&self, id: &str) -> Result<Option<ApiKey>, Error>;

    /// Store a new API key
    async fn create(&self, key: &ApiKey) -> Result<(), Error>;

    /// Update the last_used_at timestamp
    async fn update_last_used(&self, id: &str) -> Result<(), Error>;

    /// Revoke an API key
    async fn revoke(&self, id: &str) -> Result<(), Error>;

    /// List all API keys for a user
    async fn list_by_user(&self, user_id: &str) -> Result<Vec<ApiKey>, Error>;

    /// Delete an API key
    async fn delete(&self, id: &str) -> Result<(), Error>;
}

/// Redis-based API key storage
#[cfg(feature = "cache")]
pub mod redis_storage {
    use super::*;
    use deadpool_redis::Pool;
    use redis::AsyncCommands;

    /// Redis-backed API key storage
    #[derive(Clone)]
    pub struct RedisApiKeyStorage {
        pool: Pool,
        key_prefix: String,
        generator: ApiKeyGenerator,
    }

    impl RedisApiKeyStorage {
        /// Create a new Redis API key storage
        pub fn new(pool: Pool, api_key_prefix: impl Into<String>) -> Self {
            let prefix: String = api_key_prefix.into();
            Self {
                pool,
                key_prefix: "api_key".to_string(),
                generator: ApiKeyGenerator::new(&prefix),
            }
        }

        fn id_key(&self, id: &str) -> String {
            format!("{}:id:{}", self.key_prefix, id)
        }

        fn prefix_key(&self, prefix: &str) -> String {
            format!("{}:prefix:{}", self.key_prefix, prefix)
        }

        fn user_key(&self, user_id: &str) -> String {
            format!("{}:user:{}", self.key_prefix, user_id)
        }
    }

    #[async_trait]
    impl ApiKeyStorage for RedisApiKeyStorage {
        async fn get_by_key(&self, key: &str) -> Result<Option<ApiKey>, Error> {
            // Get by lookup prefix
            let lookup_prefix = ApiKeyGenerator::key_prefix_for_lookup(key)
                .ok_or_else(|| Error::ValidationError("Invalid API key format".to_string()))?;

            if let Some(api_key) = self.get_by_prefix(&lookup_prefix).await? {
                // Verify the full key matches
                if self.generator.verify(key, &api_key.key_hash)? {
                    return Ok(Some(api_key));
                }
            }
            Ok(None)
        }

        async fn get_by_prefix(&self, prefix: &str) -> Result<Option<ApiKey>, Error> {
            let mut conn =
                self.pool.get().await.map_err(|e| {
                    Error::Internal(format!("Failed to get Redis connection: {}", e))
                })?;

            let key = self.prefix_key(prefix);
            let id: Option<String> = conn
                .get(&key)
                .await
                .map_err(|e| Error::Internal(format!("Failed to get API key prefix: {}", e)))?;

            match id {
                Some(id) => self.get_by_id(&id).await,
                None => Ok(None),
            }
        }

        async fn get_by_id(&self, id: &str) -> Result<Option<ApiKey>, Error> {
            let mut conn =
                self.pool.get().await.map_err(|e| {
                    Error::Internal(format!("Failed to get Redis connection: {}", e))
                })?;

            let key = self.id_key(id);
            let json: Option<String> = conn
                .get(&key)
                .await
                .map_err(|e| Error::Internal(format!("Failed to get API key: {}", e)))?;

            match json {
                Some(j) => {
                    let api_key: ApiKey = serde_json::from_str(&j)
                        .map_err(|e| Error::Internal(format!("Failed to parse API key: {}", e)))?;
                    Ok(Some(api_key))
                }
                None => Ok(None),
            }
        }

        async fn create(&self, api_key: &ApiKey) -> Result<(), Error> {
            let mut conn =
                self.pool.get().await.map_err(|e| {
                    Error::Internal(format!("Failed to get Redis connection: {}", e))
                })?;

            let json = serde_json::to_string(api_key)
                .map_err(|e| Error::Internal(format!("Failed to serialize API key: {}", e)))?;

            // Store by ID
            let id_key = self.id_key(&api_key.id);
            conn.set::<_, _, ()>(&id_key, &json)
                .await
                .map_err(|e| Error::Internal(format!("Failed to store API key: {}", e)))?;

            // Store prefix -> ID mapping
            let prefix_key = self.prefix_key(&api_key.prefix);
            conn.set::<_, _, ()>(&prefix_key, &api_key.id)
                .await
                .map_err(|e| Error::Internal(format!("Failed to store prefix mapping: {}", e)))?;

            // Add to user's key set
            let user_key = self.user_key(&api_key.user_id);
            conn.sadd::<_, _, ()>(&user_key, &api_key.id)
                .await
                .map_err(|e| Error::Internal(format!("Failed to add to user set: {}", e)))?;

            Ok(())
        }

        async fn update_last_used(&self, id: &str) -> Result<(), Error> {
            if let Some(mut api_key) = self.get_by_id(id).await? {
                api_key.last_used_at = Some(Utc::now());
                let json = serde_json::to_string(&api_key)
                    .map_err(|e| Error::Internal(format!("Failed to serialize API key: {}", e)))?;

                let mut conn = self.pool.get().await.map_err(|e| {
                    Error::Internal(format!("Failed to get Redis connection: {}", e))
                })?;

                let key = self.id_key(id);
                conn.set::<_, _, ()>(&key, &json)
                    .await
                    .map_err(|e| Error::Internal(format!("Failed to update API key: {}", e)))?;
            }
            Ok(())
        }

        async fn revoke(&self, id: &str) -> Result<(), Error> {
            if let Some(mut api_key) = self.get_by_id(id).await? {
                api_key.is_revoked = true;
                let json = serde_json::to_string(&api_key)
                    .map_err(|e| Error::Internal(format!("Failed to serialize API key: {}", e)))?;

                let mut conn = self.pool.get().await.map_err(|e| {
                    Error::Internal(format!("Failed to get Redis connection: {}", e))
                })?;

                let key = self.id_key(id);
                conn.set::<_, _, ()>(&key, &json)
                    .await
                    .map_err(|e| Error::Internal(format!("Failed to revoke API key: {}", e)))?;
            }
            Ok(())
        }

        async fn list_by_user(&self, user_id: &str) -> Result<Vec<ApiKey>, Error> {
            let mut conn =
                self.pool.get().await.map_err(|e| {
                    Error::Internal(format!("Failed to get Redis connection: {}", e))
                })?;

            let user_key = self.user_key(user_id);
            let ids: Vec<String> = conn
                .smembers(&user_key)
                .await
                .map_err(|e| Error::Internal(format!("Failed to get user keys: {}", e)))?;

            let mut keys = Vec::new();
            for id in ids {
                if let Some(api_key) = self.get_by_id(&id).await? {
                    keys.push(api_key);
                }
            }
            Ok(keys)
        }

        async fn delete(&self, id: &str) -> Result<(), Error> {
            if let Some(api_key) = self.get_by_id(id).await? {
                let mut conn = self.pool.get().await.map_err(|e| {
                    Error::Internal(format!("Failed to get Redis connection: {}", e))
                })?;

                // Remove from user set
                let user_key = self.user_key(&api_key.user_id);
                conn.srem::<_, _, ()>(&user_key, id).await.map_err(|e| {
                    Error::Internal(format!("Failed to remove from user set: {}", e))
                })?;

                // Remove prefix mapping
                let prefix_key = self.prefix_key(&api_key.prefix);
                conn.del::<_, ()>(&prefix_key)
                    .await
                    .map_err(|e| Error::Internal(format!("Failed to delete prefix: {}", e)))?;

                // Remove the key itself
                let id_key = self.id_key(id);
                conn.del::<_, ()>(&id_key)
                    .await
                    .map_err(|e| Error::Internal(format!("Failed to delete API key: {}", e)))?;
            }
            Ok(())
        }
    }
}

#[cfg(feature = "cache")]
pub use redis_storage::RedisApiKeyStorage;

/// PostgreSQL-based API key storage
#[cfg(feature = "database")]
pub mod pg_storage {
    use super::*;
    use sqlx::PgPool;

    /// PostgreSQL-backed API key storage
    #[derive(Clone)]
    pub struct PgApiKeyStorage {
        pool: PgPool,
        generator: ApiKeyGenerator,
    }

    impl PgApiKeyStorage {
        /// Create a new PostgreSQL API key storage
        pub fn new(pool: PgPool, api_key_prefix: impl Into<String>) -> Self {
            Self {
                pool,
                generator: ApiKeyGenerator::new(api_key_prefix),
            }
        }
    }

    #[async_trait]
    impl ApiKeyStorage for PgApiKeyStorage {
        async fn get_by_key(&self, key: &str) -> Result<Option<ApiKey>, Error> {
            // Get by lookup prefix
            let lookup_prefix = ApiKeyGenerator::key_prefix_for_lookup(key)
                .ok_or_else(|| Error::ValidationError("Invalid API key format".to_string()))?;

            if let Some(api_key) = self.get_by_prefix(&lookup_prefix).await? {
                // Verify the full key matches
                if self.generator.verify(key, &api_key.key_hash)? {
                    return Ok(Some(api_key));
                }
            }
            Ok(None)
        }

        async fn get_by_prefix(&self, prefix: &str) -> Result<Option<ApiKey>, Error> {
            let row = sqlx::query_as::<_, (String, String, String, String, String, serde_json::Value, Option<i32>, bool, Option<DateTime<Utc>>, Option<DateTime<Utc>>, DateTime<Utc>)>(
                r#"
                SELECT id, user_id, name, key_prefix, key_hash, scopes, rate_limit, is_revoked, last_used_at, expires_at, created_at
                FROM api_keys
                WHERE key_prefix = $1
                "#,
            )
            .bind(prefix)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| Error::Internal(format!("Failed to get API key: {}", e)))?;

            match row {
                Some((
                    id,
                    user_id,
                    name,
                    prefix,
                    key_hash,
                    scopes_json,
                    rate_limit,
                    is_revoked,
                    last_used_at,
                    expires_at,
                    created_at,
                )) => {
                    let scopes: Vec<String> =
                        serde_json::from_value(scopes_json).unwrap_or_default();
                    Ok(Some(ApiKey {
                        id,
                        user_id,
                        name,
                        prefix,
                        key_hash,
                        scopes,
                        rate_limit: rate_limit.map(|r| r as u32),
                        is_revoked,
                        last_used_at,
                        expires_at,
                        created_at,
                    }))
                }
                None => Ok(None),
            }
        }

        async fn get_by_id(&self, id: &str) -> Result<Option<ApiKey>, Error> {
            let row = sqlx::query_as::<_, (String, String, String, String, String, serde_json::Value, Option<i32>, bool, Option<DateTime<Utc>>, Option<DateTime<Utc>>, DateTime<Utc>)>(
                r#"
                SELECT id, user_id, name, key_prefix, key_hash, scopes, rate_limit, is_revoked, last_used_at, expires_at, created_at
                FROM api_keys
                WHERE id = $1
                "#,
            )
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| Error::Internal(format!("Failed to get API key: {}", e)))?;

            match row {
                Some((
                    id,
                    user_id,
                    name,
                    prefix,
                    key_hash,
                    scopes_json,
                    rate_limit,
                    is_revoked,
                    last_used_at,
                    expires_at,
                    created_at,
                )) => {
                    let scopes: Vec<String> =
                        serde_json::from_value(scopes_json).unwrap_or_default();
                    Ok(Some(ApiKey {
                        id,
                        user_id,
                        name,
                        prefix,
                        key_hash,
                        scopes,
                        rate_limit: rate_limit.map(|r| r as u32),
                        is_revoked,
                        last_used_at,
                        expires_at,
                        created_at,
                    }))
                }
                None => Ok(None),
            }
        }

        async fn create(&self, api_key: &ApiKey) -> Result<(), Error> {
            let scopes_json = serde_json::to_value(&api_key.scopes)
                .map_err(|e| Error::Internal(format!("Failed to serialize scopes: {}", e)))?;

            sqlx::query(
                r#"
                INSERT INTO api_keys (id, user_id, name, key_prefix, key_hash, scopes, rate_limit, is_revoked, expires_at, created_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                "#,
            )
            .bind(&api_key.id)
            .bind(&api_key.user_id)
            .bind(&api_key.name)
            .bind(&api_key.prefix)
            .bind(&api_key.key_hash)
            .bind(scopes_json)
            .bind(api_key.rate_limit.map(|r| r as i32))
            .bind(api_key.is_revoked)
            .bind(api_key.expires_at)
            .bind(api_key.created_at)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Internal(format!("Failed to create API key: {}", e)))?;

            Ok(())
        }

        async fn update_last_used(&self, id: &str) -> Result<(), Error> {
            sqlx::query("UPDATE api_keys SET last_used_at = NOW() WHERE id = $1")
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| Error::Internal(format!("Failed to update last_used_at: {}", e)))?;

            Ok(())
        }

        async fn revoke(&self, id: &str) -> Result<(), Error> {
            sqlx::query("UPDATE api_keys SET is_revoked = true WHERE id = $1")
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| Error::Internal(format!("Failed to revoke API key: {}", e)))?;

            Ok(())
        }

        async fn list_by_user(&self, user_id: &str) -> Result<Vec<ApiKey>, Error> {
            let rows = sqlx::query_as::<_, (String, String, String, String, String, serde_json::Value, Option<i32>, bool, Option<DateTime<Utc>>, Option<DateTime<Utc>>, DateTime<Utc>)>(
                r#"
                SELECT id, user_id, name, key_prefix, key_hash, scopes, rate_limit, is_revoked, last_used_at, expires_at, created_at
                FROM api_keys
                WHERE user_id = $1
                ORDER BY created_at DESC
                "#,
            )
            .bind(user_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Error::Internal(format!("Failed to list API keys: {}", e)))?;

            let keys = rows
                .into_iter()
                .map(
                    |(
                        id,
                        user_id,
                        name,
                        prefix,
                        key_hash,
                        scopes_json,
                        rate_limit,
                        is_revoked,
                        last_used_at,
                        expires_at,
                        created_at,
                    )| {
                        let scopes: Vec<String> =
                            serde_json::from_value(scopes_json).unwrap_or_default();
                        ApiKey {
                            id,
                            user_id,
                            name,
                            prefix,
                            key_hash,
                            scopes,
                            rate_limit: rate_limit.map(|r| r as u32),
                            is_revoked,
                            last_used_at,
                            expires_at,
                            created_at,
                        }
                    },
                )
                .collect();

            Ok(keys)
        }

        async fn delete(&self, id: &str) -> Result<(), Error> {
            sqlx::query("DELETE FROM api_keys WHERE id = $1")
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| Error::Internal(format!("Failed to delete API key: {}", e)))?;

            Ok(())
        }
    }
}

#[cfg(feature = "database")]
pub use pg_storage::PgApiKeyStorage;

/// Turso/libsql-based API key storage
#[cfg(feature = "turso")]
pub mod turso_storage {
    use super::*;
    use libsql::Connection;
    use std::sync::Arc;

    /// Turso-backed API key storage
    #[derive(Clone)]
    pub struct TursoApiKeyStorage {
        conn: Arc<Connection>,
        generator: ApiKeyGenerator,
    }

    impl TursoApiKeyStorage {
        /// Create a new Turso API key storage
        pub fn new(conn: Arc<Connection>, api_key_prefix: impl Into<String>) -> Self {
            Self {
                conn,
                generator: ApiKeyGenerator::new(api_key_prefix),
            }
        }
    }

    #[async_trait]
    impl ApiKeyStorage for TursoApiKeyStorage {
        async fn get_by_key(&self, key: &str) -> Result<Option<ApiKey>, Error> {
            let lookup_prefix = ApiKeyGenerator::key_prefix_for_lookup(key)
                .ok_or_else(|| Error::ValidationError("Invalid API key format".to_string()))?;

            if let Some(api_key) = self.get_by_prefix(&lookup_prefix).await? {
                if self.generator.verify(key, &api_key.key_hash)? {
                    return Ok(Some(api_key));
                }
            }
            Ok(None)
        }

        async fn get_by_prefix(&self, prefix: &str) -> Result<Option<ApiKey>, Error> {
            let mut rows = self
                .conn
                .query(
                    "SELECT id, user_id, name, key_prefix, key_hash, scopes, rate_limit, is_revoked, last_used_at, expires_at, created_at FROM api_keys WHERE key_prefix = ?1",
                    libsql::params![prefix],
                )
                .await
                .map_err(|e| Error::Internal(format!("Failed to get API key: {}", e)))?;

            if let Some(row) = rows
                .next()
                .await
                .map_err(|e| Error::Internal(format!("Failed to fetch row: {}", e)))?
            {
                let api_key = parse_api_key_row(&row)?;
                Ok(Some(api_key))
            } else {
                Ok(None)
            }
        }

        async fn get_by_id(&self, id: &str) -> Result<Option<ApiKey>, Error> {
            let mut rows = self
                .conn
                .query(
                    "SELECT id, user_id, name, key_prefix, key_hash, scopes, rate_limit, is_revoked, last_used_at, expires_at, created_at FROM api_keys WHERE id = ?1",
                    libsql::params![id],
                )
                .await
                .map_err(|e| Error::Internal(format!("Failed to get API key: {}", e)))?;

            if let Some(row) = rows
                .next()
                .await
                .map_err(|e| Error::Internal(format!("Failed to fetch row: {}", e)))?
            {
                let api_key = parse_api_key_row(&row)?;
                Ok(Some(api_key))
            } else {
                Ok(None)
            }
        }

        async fn create(&self, api_key: &ApiKey) -> Result<(), Error> {
            let scopes_json = serde_json::to_string(&api_key.scopes)
                .map_err(|e| Error::Internal(format!("Failed to serialize scopes: {}", e)))?;

            let expires_at = api_key.expires_at.map(|dt| dt.to_rfc3339());

            self.conn
                .execute(
                    "INSERT INTO api_keys (id, user_id, name, key_prefix, key_hash, scopes, rate_limit, is_revoked, expires_at, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    libsql::params![
                        api_key.id.clone(),
                        api_key.user_id.clone(),
                        api_key.name.clone(),
                        api_key.prefix.clone(),
                        api_key.key_hash.clone(),
                        scopes_json,
                        api_key.rate_limit.map(|r| r as i64),
                        if api_key.is_revoked { 1i64 } else { 0i64 },
                        expires_at,
                        api_key.created_at.to_rfc3339()
                    ],
                )
                .await
                .map_err(|e| Error::Internal(format!("Failed to create API key: {}", e)))?;

            Ok(())
        }

        async fn update_last_used(&self, id: &str) -> Result<(), Error> {
            self.conn
                .execute(
                    "UPDATE api_keys SET last_used_at = datetime('now') WHERE id = ?1",
                    libsql::params![id],
                )
                .await
                .map_err(|e| Error::Internal(format!("Failed to update last_used_at: {}", e)))?;

            Ok(())
        }

        async fn revoke(&self, id: &str) -> Result<(), Error> {
            self.conn
                .execute(
                    "UPDATE api_keys SET is_revoked = 1 WHERE id = ?1",
                    libsql::params![id],
                )
                .await
                .map_err(|e| Error::Internal(format!("Failed to revoke API key: {}", e)))?;

            Ok(())
        }

        async fn list_by_user(&self, user_id: &str) -> Result<Vec<ApiKey>, Error> {
            let mut rows = self
                .conn
                .query(
                    "SELECT id, user_id, name, key_prefix, key_hash, scopes, rate_limit, is_revoked, last_used_at, expires_at, created_at FROM api_keys WHERE user_id = ?1 ORDER BY created_at DESC",
                    libsql::params![user_id],
                )
                .await
                .map_err(|e| Error::Internal(format!("Failed to list API keys: {}", e)))?;

            let mut keys = Vec::new();
            while let Some(row) = rows
                .next()
                .await
                .map_err(|e| Error::Internal(format!("Failed to fetch row: {}", e)))?
            {
                let api_key = parse_api_key_row(&row)?;
                keys.push(api_key);
            }
            Ok(keys)
        }

        async fn delete(&self, id: &str) -> Result<(), Error> {
            self.conn
                .execute("DELETE FROM api_keys WHERE id = ?1", libsql::params![id])
                .await
                .map_err(|e| Error::Internal(format!("Failed to delete API key: {}", e)))?;

            Ok(())
        }
    }

    fn parse_api_key_row(row: &libsql::Row) -> Result<ApiKey, Error> {
        let id: String = row
            .get(0)
            .map_err(|e| Error::Internal(format!("Failed to get id: {}", e)))?;
        let user_id: String = row
            .get(1)
            .map_err(|e| Error::Internal(format!("Failed to get user_id: {}", e)))?;
        let name: String = row
            .get(2)
            .map_err(|e| Error::Internal(format!("Failed to get name: {}", e)))?;
        let prefix: String = row
            .get(3)
            .map_err(|e| Error::Internal(format!("Failed to get key_prefix: {}", e)))?;
        let key_hash: String = row
            .get(4)
            .map_err(|e| Error::Internal(format!("Failed to get key_hash: {}", e)))?;
        let scopes_str: String = row
            .get(5)
            .map_err(|e| Error::Internal(format!("Failed to get scopes: {}", e)))?;
        let rate_limit: Option<i64> = row.get(6).ok();
        let is_revoked: i64 = row
            .get(7)
            .map_err(|e| Error::Internal(format!("Failed to get is_revoked: {}", e)))?;
        let last_used_at_str: Option<String> = row.get(8).ok();
        let expires_at_str: Option<String> = row.get(9).ok();
        let created_at_str: String = row
            .get(10)
            .map_err(|e| Error::Internal(format!("Failed to get created_at: {}", e)))?;

        let scopes: Vec<String> = serde_json::from_str(&scopes_str).unwrap_or_default();
        let last_used_at = last_used_at_str
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc));
        let expires_at = expires_at_str
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc));
        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        Ok(ApiKey {
            id,
            user_id,
            name,
            prefix,
            key_hash,
            scopes,
            rate_limit: rate_limit.map(|r| r as u32),
            is_revoked: is_revoked != 0,
            last_used_at,
            expires_at,
            created_at,
        })
    }
}

#[cfg(feature = "turso")]
pub use turso_storage::TursoApiKeyStorage;

/// SurrealDB-based API key storage
#[cfg(feature = "surrealdb")]
pub mod surrealdb_storage {
    use super::*;
    use crate::surrealdb_backend::SurrealClient;
    use std::sync::Arc;

    /// SurrealDB-backed API key storage
    #[derive(Clone)]
    pub struct SurrealDbApiKeyStorage {
        client: Arc<SurrealClient>,
        generator: ApiKeyGenerator,
    }

    impl SurrealDbApiKeyStorage {
        /// Create a new SurrealDB API key storage
        pub fn new(client: Arc<SurrealClient>, api_key_prefix: impl Into<String>) -> Self {
            Self {
                client,
                generator: ApiKeyGenerator::new(api_key_prefix),
            }
        }
    }

    #[async_trait]
    impl ApiKeyStorage for SurrealDbApiKeyStorage {
        async fn get_by_key(&self, key: &str) -> Result<Option<ApiKey>, Error> {
            let lookup_prefix = ApiKeyGenerator::key_prefix_for_lookup(key)
                .ok_or_else(|| Error::ValidationError("Invalid API key format".to_string()))?;

            if let Some(api_key) = self.get_by_prefix(&lookup_prefix).await? {
                if self.generator.verify(key, &api_key.key_hash)? {
                    return Ok(Some(api_key));
                }
            }
            Ok(None)
        }

        async fn get_by_prefix(&self, prefix: &str) -> Result<Option<ApiKey>, Error> {
            let mut result = self
                .client
                .query("SELECT * FROM api_keys WHERE prefix = $prefix LIMIT 1")
                .bind(("prefix", prefix.to_string()))
                .await
                .map_err(|e| Error::Internal(format!("Failed to get API key: {}", e)))?;

            let api_key: Option<ApiKey> = result
                .take(0)
                .map_err(|e| Error::Internal(format!("Failed to parse API key: {}", e)))?;

            Ok(api_key)
        }

        async fn get_by_id(&self, id: &str) -> Result<Option<ApiKey>, Error> {
            let mut result = self
                .client
                .query("SELECT * FROM api_keys WHERE id = $id LIMIT 1")
                .bind(("id", id.to_string()))
                .await
                .map_err(|e| Error::Internal(format!("Failed to get API key: {}", e)))?;

            let api_key: Option<ApiKey> = result
                .take(0)
                .map_err(|e| Error::Internal(format!("Failed to parse API key: {}", e)))?;

            Ok(api_key)
        }

        async fn create(&self, api_key: &ApiKey) -> Result<(), Error> {
            self.client
                .query("CREATE api_keys CONTENT $data")
                .bind(("data", api_key.clone()))
                .await
                .map_err(|e| Error::Internal(format!("Failed to create API key: {}", e)))?;

            Ok(())
        }

        async fn update_last_used(&self, id: &str) -> Result<(), Error> {
            self.client
                .query("UPDATE api_keys SET last_used_at = time::now() WHERE id = $id")
                .bind(("id", id.to_string()))
                .await
                .map_err(|e| Error::Internal(format!("Failed to update last_used_at: {}", e)))?;

            Ok(())
        }

        async fn revoke(&self, id: &str) -> Result<(), Error> {
            self.client
                .query("UPDATE api_keys SET is_revoked = true WHERE id = $id")
                .bind(("id", id.to_string()))
                .await
                .map_err(|e| Error::Internal(format!("Failed to revoke API key: {}", e)))?;

            Ok(())
        }

        async fn list_by_user(&self, user_id: &str) -> Result<Vec<ApiKey>, Error> {
            let mut result = self
                .client
                .query("SELECT * FROM api_keys WHERE user_id = $user_id ORDER BY created_at DESC")
                .bind(("user_id", user_id.to_string()))
                .await
                .map_err(|e| Error::Internal(format!("Failed to list API keys: {}", e)))?;

            let keys: Vec<ApiKey> = result
                .take(0)
                .map_err(|e| Error::Internal(format!("Failed to parse API keys: {}", e)))?;

            Ok(keys)
        }

        async fn delete(&self, id: &str) -> Result<(), Error> {
            self.client
                .query("DELETE FROM api_keys WHERE id = $id")
                .bind(("id", id.to_string()))
                .await
                .map_err(|e| Error::Internal(format!("Failed to delete API key: {}", e)))?;

            Ok(())
        }
    }
}

#[cfg(feature = "surrealdb")]
pub use surrealdb_storage::SurrealDbApiKeyStorage;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_api_key() {
        let generator = ApiKeyGenerator::new("sk_live");
        let (key, hash) = generator.generate();

        assert!(key.starts_with("sk_live_"));
        assert!(hash.starts_with("$argon2id$"));

        // Key should be unique each time
        let (key2, _) = generator.generate();
        assert_ne!(key, key2);
    }

    #[test]
    fn test_verify_api_key() {
        let generator = ApiKeyGenerator::new("sk_test");
        let (key, hash) = generator.generate();

        assert!(generator.verify(&key, &hash).unwrap());
        assert!(!generator.verify("wrong_key", &hash).unwrap());
    }

    #[test]
    fn test_extract_prefix() {
        assert_eq!(
            ApiKeyGenerator::extract_prefix("sk_live_abc123"),
            Some("sk")
        );
        assert_eq!(
            ApiKeyGenerator::extract_prefix("acton_xyz789"),
            Some("acton")
        );
    }

    #[test]
    fn test_key_prefix_for_lookup() {
        let lookup = ApiKeyGenerator::key_prefix_for_lookup("sk_live_abcdefghijklmnop");
        assert_eq!(lookup, Some("sk_live_abcdefgh".to_string()));
    }

    #[test]
    fn test_api_key_validity() {
        let key = ApiKey {
            id: "1".to_string(),
            user_id: "user:123".to_string(),
            name: "Test Key".to_string(),
            prefix: "sk_live".to_string(),
            key_hash: "hash".to_string(),
            scopes: vec!["read".to_string(), "write".to_string()],
            rate_limit: None,
            is_revoked: false,
            last_used_at: None,
            expires_at: None,
            created_at: Utc::now(),
        };

        assert!(key.is_valid());
        assert!(key.has_scope("read"));
        assert!(key.has_scope("write"));
        assert!(!key.has_scope("admin"));
    }

    #[test]
    fn test_api_key_revoked() {
        let key = ApiKey {
            id: "1".to_string(),
            user_id: "user:123".to_string(),
            name: "Test Key".to_string(),
            prefix: "sk_live".to_string(),
            key_hash: "hash".to_string(),
            scopes: vec![],
            rate_limit: None,
            is_revoked: true,
            last_used_at: None,
            expires_at: None,
            created_at: Utc::now(),
        };

        assert!(!key.is_valid());
    }

    #[test]
    fn test_api_key_expired() {
        let key = ApiKey {
            id: "1".to_string(),
            user_id: "user:123".to_string(),
            name: "Test Key".to_string(),
            prefix: "sk_live".to_string(),
            key_hash: "hash".to_string(),
            scopes: vec![],
            rate_limit: None,
            is_revoked: false,
            last_used_at: None,
            expires_at: Some(Utc::now() - chrono::Duration::hours(1)),
            created_at: Utc::now(),
        };

        assert!(!key.is_valid());
    }

    #[test]
    fn test_base32_encode() {
        // Test with known values
        let bytes = [0x48, 0x65, 0x6c, 0x6c, 0x6f]; // "Hello"
        let encoded = base32_encode(&bytes);
        assert_eq!(encoded, "jbswy3dp"); // lowercase base32 of "Hello"
    }
}
