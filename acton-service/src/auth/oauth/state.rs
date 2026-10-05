//! OAuth state management for CSRF protection
//!
//! Manages OAuth state values to prevent CSRF attacks during the
//! authorization flow.

use std::collections::{BTreeSet, HashMap};
use std::num::NonZeroUsize;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

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
    let bytes: [u8; 32] = rand::random();
    base64_url_encode(&bytes)
}

/// Base64 URL-safe encoding without padding
fn base64_url_encode(bytes: &[u8]) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.encode(bytes)
}

/// In-memory, single-use OAuth state storage for a single service process.
///
/// No `cache` feature or background runtime is needed. Share one manager between
/// login and callback handlers, usually through `Arc`. For multiple processes,
/// use `RedisOAuthStateManager` instead (requires `cache`).
///
/// TTL starts when a state is stored, using a monotonic clock. `StateData::created_at`
/// is metadata and does not affect validity, just as Redis TTL starts on storage.
/// Expired states are swept on each operation. At capacity, the oldest pending
/// login is evicted; its callback will fail and the user must start again.
#[derive(Debug)]
pub struct MemoryOAuthStateManager {
    states: Mutex<MemoryStates>,
    ttl: Duration,
    max_entries: NonZeroUsize,
}

#[derive(Debug)]
struct MemoryStateEntry {
    data: StateData,
    stored_at: Instant,
}

#[derive(Debug, Default)]
struct MemoryStates {
    entries: HashMap<String, MemoryStateEntry>,
    // Both indexes are updated under one lock, including after consumption.
    oldest: BTreeSet<(Instant, String)>,
}

impl MemoryStates {
    fn remove(&mut self, state: &str) -> Option<MemoryStateEntry> {
        let entry = self.entries.remove(state)?;
        self.oldest.remove(&(entry.stored_at, state.to_string()));
        Some(entry)
    }

    fn sweep(&mut self, now: Instant, ttl: Duration) {
        while let Some((stored_at, state)) = self.oldest.first() {
            if now.saturating_duration_since(*stored_at) < ttl {
                break;
            }
            let state = state.clone();
            self.remove(&state);
        }
    }

    fn insert(&mut self, data: &StateData, now: Instant, capacity: NonZeroUsize) -> String {
        if self.entries.len() >= capacity.get() {
            if let Some((_, state)) = self.oldest.first() {
                let state = state.clone();
                self.remove(&state);
            }
        }
        // Even a random collision must not replace a different pending login.
        let state = loop {
            let candidate = generate_state();
            if !self.entries.contains_key(&candidate) {
                break candidate;
            }
        };
        self.entries.insert(
            state.clone(),
            MemoryStateEntry {
                data: data.clone(),
                stored_at: now,
            },
        );
        self.oldest.insert((now, state.clone()));
        state
    }
}

impl MemoryOAuthStateManager {
    /// Create a state manager with the supplied TTL and a 10,000-entry limit.
    ///
    /// A zero TTL makes every state immediately expire.
    #[must_use]
    pub fn new(ttl_secs: u64) -> Self {
        Self::with_max_entries(
            ttl_secs,
            NonZeroUsize::new(10_000).unwrap_or(NonZeroUsize::MIN),
        )
    }

    /// Create a manager with a nonzero maximum number of pending states.
    #[must_use]
    pub fn with_max_entries(ttl_secs: u64, max_entries: NonZeroUsize) -> Self {
        Self {
            states: Mutex::new(MemoryStates::default()),
            ttl: Duration::from_secs(ttl_secs),
            max_entries,
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, MemoryStates>, Error> {
        self.states
            .lock()
            .map_err(|_| Error::Internal("OAuth state storage lock poisoned".to_string()))
    }

    fn create_at(&self, data: &StateData, now: Instant) -> Result<String, Error> {
        let mut states = self.lock()?;
        states.sweep(now, self.ttl);
        Ok(states.insert(data, now, self.max_entries))
    }

    fn validate_at(&self, state: &str, now: Instant) -> Result<StateData, Error> {
        let mut states = self.lock()?;
        states.sweep(now, self.ttl);
        states
            .remove(state)
            .map(|entry| entry.data)
            .ok_or_else(|| Error::BadRequest("Invalid or expired OAuth state".to_string()))
    }
}

#[async_trait]
impl OAuthStateManager for MemoryOAuthStateManager {
    async fn create_state(&self, data: &StateData) -> Result<String, Error> {
        self.create_at(data, Instant::now())
    }

    async fn validate_state(&self, state: &str) -> Result<StateData, Error> {
        self.validate_at(state, Instant::now())
    }
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

            let mut conn =
                self.pool.get().await.map_err(|e| {
                    Error::Internal(format!("Failed to get Redis connection: {}", e))
                })?;

            conn.set_ex::<_, _, ()>(&key, data_json, self.ttl_secs)
                .await
                .map_err(|e| Error::Internal(format!("Failed to store OAuth state: {}", e)))?;

            Ok(state)
        }

        async fn validate_state(&self, state: &str) -> Result<StateData, Error> {
            use deadpool_redis::redis::AsyncCommands;

            let key = self.state_key(state);

            let mut conn =
                self.pool.get().await.map_err(|e| {
                    Error::Internal(format!("Failed to get Redis connection: {}", e))
                })?;

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

    fn state_data() -> StateData {
        StateData {
            provider: "google".to_string(),
            redirect_uri: Some("/dashboard".to_string()),
            created_at: 0,
            extra: Some(serde_json::json!({"intent": "login"})),
        }
    }

    fn assert_invalid(result: Result<StateData, Error>) {
        assert!(matches!(result, Err(Error::BadRequest(ref message))
            if message == "Invalid or expired OAuth state"));
    }

    #[tokio::test]
    async fn memory_state_is_preserved_and_consumed_once() {
        let manager = MemoryOAuthStateManager::new(600);
        let data = state_data();
        let state = manager.create_state(&data).await.expect("state stored");
        let consumed = manager.validate_state(&state).await.expect("valid state");
        assert_eq!(consumed.provider, data.provider);
        assert_eq!(consumed.redirect_uri, data.redirect_uri);
        assert_eq!(consumed.created_at, data.created_at);
        assert_eq!(consumed.extra, data.extra);
        assert_invalid(manager.validate_state(&state).await);
        assert_invalid(manager.validate_state("unknown state").await);
    }

    #[test]
    fn memory_state_expires_at_the_ttl_boundary_and_sweeps_other_states() {
        let manager = MemoryOAuthStateManager::new(10);
        let stored_at = Instant::now();
        let expired = manager.create_at(&state_data(), stored_at).expect("first");
        let also_expired = manager.create_at(&state_data(), stored_at).expect("second");
        assert_invalid(manager.validate_at(&expired, stored_at + Duration::from_secs(10)));
        assert_invalid(manager.validate_at(&also_expired, stored_at + Duration::from_secs(10)));
        let states = manager.lock().expect("storage available");
        assert!(states.entries.is_empty());
        assert!(states.oldest.is_empty());
    }

    #[test]
    fn memory_state_is_valid_just_before_expiry_regardless_of_metadata_timestamp() {
        let manager = MemoryOAuthStateManager::new(10);
        let stored_at = Instant::now();
        let mut data = state_data();
        data.created_at = i64::MIN;
        let state = manager.create_at(&data, stored_at).expect("state stored");
        let consumed = manager
            .validate_at(
                &state,
                stored_at + Duration::from_secs(10) - Duration::from_nanos(1),
            )
            .expect("valid before deadline");
        assert_eq!(consumed.created_at, i64::MIN);
    }

    #[tokio::test]
    async fn zero_ttl_state_is_never_accepted() {
        let manager = MemoryOAuthStateManager::new(0);
        let state = manager
            .create_state(&state_data())
            .await
            .expect("state stored");
        assert_invalid(manager.validate_state(&state).await);
    }

    #[test]
    fn full_memory_store_evicts_the_oldest_pending_login() {
        let manager =
            MemoryOAuthStateManager::with_max_entries(600, NonZeroUsize::new(2).expect("nonzero"));
        let start = Instant::now();
        let oldest = manager
            .create_at(&state_data(), start)
            .expect("oldest stored");
        let middle = manager
            .create_at(&state_data(), start + Duration::from_secs(1))
            .expect("middle stored");
        let newest = manager
            .create_at(&state_data(), start + Duration::from_secs(2))
            .expect("newest stored");
        let now = start + Duration::from_secs(3);
        assert_invalid(manager.validate_at(&oldest, now));
        assert!(manager.validate_at(&middle, now).is_ok());
        assert!(manager.validate_at(&newest, now).is_ok());
    }

    #[test]
    fn expiry_is_swept_before_capacity_eviction() {
        let manager =
            MemoryOAuthStateManager::with_max_entries(10, NonZeroUsize::new(2).expect("nonzero"));
        let start = Instant::now();
        let expired = manager
            .create_at(&state_data(), start)
            .expect("oldest stored");
        let pending = manager
            .create_at(&state_data(), start + Duration::from_secs(9))
            .expect("pending stored");
        let now = start + Duration::from_secs(10);
        let newest = manager
            .create_at(&state_data(), now)
            .expect("newest stored");
        assert_invalid(manager.validate_at(&expired, now));
        assert!(manager.validate_at(&pending, now).is_ok());
        assert!(manager.validate_at(&newest, now).is_ok());
    }

    #[tokio::test]
    async fn repeated_consumption_does_not_leave_an_unbounded_expiry_index() {
        let manager = MemoryOAuthStateManager::with_max_entries(600, NonZeroUsize::MIN);
        for _ in 0..100 {
            let state = manager.create_state(&state_data()).await.expect("stored");
            manager.validate_state(&state).await.expect("consumed");
        }
        let states = manager.lock().expect("storage available");
        assert!(states.entries.is_empty());
        assert!(states.oldest.is_empty());
    }

    #[test]
    fn concurrent_validation_has_exactly_one_winner() {
        let manager = MemoryOAuthStateManager::new(600);
        let now = Instant::now();
        let state = manager.create_at(&state_data(), now).expect("stored");
        let barrier = std::sync::Barrier::new(8);
        let successes = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    let manager = &manager;
                    let state = &state;
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        manager.validate_at(state, now).is_ok()
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("validator completed"))
                .filter(|succeeded| *succeeded)
                .count()
        });
        assert_eq!(successes, 1);
    }

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
