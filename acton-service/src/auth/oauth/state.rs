//! OAuth state management for CSRF protection
//!
//! Manages OAuth state values to prevent CSRF attacks during the
//! authorization flow.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::Error;

/// Data stored with OAuth state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateData {
    /// Provider name
    pub provider: String,

    /// Original redirect URI (where to send user after auth)
    pub redirect_uri: Option<String>,

    /// When this state was created (Unix timestamp)
    pub created_at: i64,

    /// Additional custom data
    pub extra: Option<serde_json::Value>,
}

/// OAuth state manager trait
///
/// Implementations store and validate OAuth state values for CSRF protection.
#[async_trait]
pub trait OAuthStateManager: Send + Sync {
    /// Create and store a new state value
    ///
    /// Returns the state string to include in the authorization URL.
    async fn create_state(&self, data: &StateData) -> Result<String, Error>;

    /// Validate and consume a state value
    ///
    /// Returns the associated data if valid, or an error if the state
    /// is invalid, expired, or already used.
    async fn validate_state(&self, state: &str) -> Result<StateData, Error>;
}

/// Generate a cryptographically secure random state value
pub fn generate_state() -> String {
    use rand::Rng;
    let bytes: [u8; 32] = rand::rng().random();
    base64_url_encode(&bytes)
}

/// Base64 URL-safe encoding without padding
fn base64_url_encode(bytes: &[u8]) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.encode(bytes)
}

// Redis state manager implementation
#[cfg(feature = "cache")]
mod redis_impl {
    use super::*;
    use deadpool_redis::Pool as RedisPool;

    /// Redis-backed OAuth state manager
    ///
    /// Stores OAuth state values in Redis with automatic TTL expiration.
    #[derive(Clone)]
    pub struct RedisOAuthStateManager {
        pool: RedisPool,
        key_prefix: String,
        ttl_secs: u64,
    }

    impl RedisOAuthStateManager {
        /// Create a new Redis OAuth state manager
        ///
        /// # Arguments
        ///
        /// * `pool` - Redis connection pool
        /// * `ttl_secs` - Time-to-live for state values (default: 600 = 10 minutes)
        pub fn new(pool: RedisPool, ttl_secs: u64) -> Self {
            Self {
                pool,
                key_prefix: "oauth:state:".to_string(),
                ttl_secs,
            }
        }

        /// Create with custom key prefix
        pub fn with_prefix(pool: RedisPool, ttl_secs: u64, prefix: impl Into<String>) -> Self {
            Self {
                pool,
                key_prefix: prefix.into(),
                ttl_secs,
            }
        }

        fn state_key(&self, state: &str) -> String {
            format!("{}{}", self.key_prefix, state)
        }
    }

    #[async_trait]
    impl OAuthStateManager for RedisOAuthStateManager {
        async fn create_state(&self, data: &StateData) -> Result<String, Error> {
            use deadpool_redis::redis::AsyncCommands;

            let state = generate_state();
            let key = self.state_key(&state);

            let data_json = serde_json::to_string(data)
                .map_err(|e| Error::Internal(format!("Failed to serialize state data: {}", e)))?;

            let mut conn = self
                .pool
                .get()
                .await
                .map_err(|e| Error::Internal(format!("Failed to get Redis connection: {}", e)))?;

            conn.set_ex::<_, _, ()>(&key, data_json, self.ttl_secs)
                .await
                .map_err(|e| Error::Internal(format!("Failed to store OAuth state: {}", e)))?;

            Ok(state)
        }

        async fn validate_state(&self, state: &str) -> Result<StateData, Error> {
            use deadpool_redis::redis::AsyncCommands;

            let key = self.state_key(state);

            let mut conn = self
                .pool
                .get()
                .await
                .map_err(|e| Error::Internal(format!("Failed to get Redis connection: {}", e)))?;

            // Get and delete atomically (GETDEL command)
            let data_json: Option<String> = conn
                .get_del(&key)
                .await
                .map_err(|e| Error::Internal(format!("Failed to retrieve OAuth state: {}", e)))?;

            match data_json {
                Some(json) => {
                    let data: StateData = serde_json::from_str(&json).map_err(|e| {
                        Error::Internal(format!("Failed to deserialize state data: {}", e))
                    })?;
                    Ok(data)
                }
                None => Err(Error::BadRequest(
                    "Invalid or expired OAuth state".to_string(),
                )),
            }
        }
    }
}

#[cfg(feature = "cache")]
pub use redis_impl::RedisOAuthStateManager;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_state_uniqueness() {
        let state1 = generate_state();
        let state2 = generate_state();
        assert_ne!(state1, state2);
        // Base64 URL-safe encoding of 32 bytes = 43 chars (without padding)
        assert_eq!(state1.len(), 43);
    }

    #[test]
    fn test_state_data_serialization() {
        let data = StateData {
            provider: "google".to_string(),
            redirect_uri: Some("https://example.com".to_string()),
            created_at: 1234567890,
            extra: Some(serde_json::json!({"foo": "bar"})),
        };

        let json = serde_json::to_string(&data).unwrap();
        let parsed: StateData = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.provider, "google");
        assert_eq!(parsed.redirect_uri, Some("https://example.com".to_string()));
        assert_eq!(parsed.created_at, 1234567890);
    }
}
