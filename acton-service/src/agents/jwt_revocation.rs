//! JWT Revocation Agent for in-memory caching with write-behind Redis persistence
//!
//! This module provides a reactive agent for managing JWT token revocations with:
//!
//! - **Fast in-memory reads**: Direct cache access without message passing overhead
//! - **Write-behind persistence**: Asynchronous persistence to Redis via agent
//! - **TTL management**: Automatic cleanup of expired revocations
//!
//! # Architecture
//!
//! The `JwtRevocationService` wraps both an `AgentHandle` and a shared cache:
//!
//! - **Reads** (`is_revoked`): Direct cache access via `Arc<RwLock<HashMap>>`
//! - **Writes** (`revoke_token`): Message passing to agent for write-behind persistence
//!
//! This hybrid approach provides the fast reads needed for middleware while
//! maintaining the benefits of agent-based write coordination.
//!
//! # Example
//!
//! ```rust,ignore
//! use acton_service::agents::prelude::*;
//!
//! // Create the service with the agent runtime
//! let runtime = builder.with_agent_runtime();
//! let revocation_service = JwtRevocationService::spawn(runtime, config).await?;
//!
//! // In JWT middleware - fast in-memory check
//! if revocation_service.is_revoked(&token.jti).await {
//!     return Err(Error::TokenRevoked);
//! }
//!
//! // In logout handler - write-behind to Redis
//! revocation_service.revoke_token(token.jti, token.exp).await;
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use acton_reactive::prelude::*;
use tokio::sync::RwLock;

use super::messages::{CleanupExpiredTokens, RevokeToken};

/// State for the JWT revocation agent
#[derive(Debug)]
pub struct JwtRevocationState {
    /// In-memory cache of revoked tokens: token_id -> expiration_time
    pub cache: Arc<RwLock<HashMap<String, SystemTime>>>,
    /// Redis pool for persistence
    pub pool: Option<deadpool_redis::Pool>,
    /// Interval for cleanup of expired tokens
    pub cleanup_interval: Duration,
}

impl Default for JwtRevocationState {
    fn default() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            pool: None,
            cleanup_interval: Duration::from_secs(300), // 5 minutes
        }
    }
}

/// Service wrapper providing both direct cache access and agent handle
///
/// This pattern enables:
/// - Fast in-memory reads without message passing overhead
/// - Write-behind persistence to Redis via agent messaging
#[derive(Clone)]
pub struct JwtRevocationService {
    /// Handle for sending messages to the agent (used for writes)
    agent_handle: AgentHandle,
    /// Shared cache for direct reads
    cache: Arc<RwLock<HashMap<String, SystemTime>>>,
}

impl JwtRevocationService {
    /// Spawn a new JWT revocation service
    ///
    /// The service will:
    /// 1. Load existing revocations from Redis on startup
    /// 2. Spawn a background task for periodic cleanup of expired tokens
    /// 3. Provide fast in-memory reads and write-behind persistence
    pub async fn spawn(
        runtime: &mut AgentRuntime,
        pool: deadpool_redis::Pool,
    ) -> anyhow::Result<Self> {
        let cache = Arc::new(RwLock::new(HashMap::new()));
        let cache_for_service = cache.clone();

        let mut agent = runtime.new_agent::<JwtRevocationState>();

        // Initialize state
        agent.model.cache = cache.clone();
        agent.model.pool = Some(pool);

        // Handle token revocation - update cache and spawn Redis persistence
        // Uses tokio::spawn for Redis operations since they're not Sync
        agent.mutate_on::<RevokeToken>(|agent, envelope| {
            let msg = envelope.message().clone();
            let cache = agent.model.cache.clone();
            let pool = agent.model.pool.clone();

            // Update cache synchronously, spawn Redis write
            AgentReply::from_async(async move {
                // Update in-memory cache immediately
                {
                    let mut cache_write = cache.write().await;
                    cache_write.insert(msg.token_id.clone(), msg.expires_at);
                }

                // Spawn a separate task for Redis persistence (fire-and-forget)
                // This avoids the Sync requirement in the handler
                if let Some(pool) = pool {
                    let token_id = msg.token_id;
                    let expires_at = msg.expires_at;
                    tokio::spawn(async move {
                        if let Err(e) = persist_revocation_to_redis(&pool, &token_id, expires_at).await {
                            tracing::error!(
                                token_id = %token_id,
                                error = %e,
                                "Failed to persist token revocation to Redis"
                            );
                        }
                    });
                }
            })
        });

        // Handle cleanup of expired tokens (cache-only, no Redis needed)
        agent.mutate_on::<CleanupExpiredTokens>(|agent, _envelope| {
            let cache = agent.model.cache.clone();

            AgentReply::from_async(async move {
                let now = SystemTime::now();
                let mut cache_write = cache.write().await;
                let initial_count = cache_write.len();

                cache_write.retain(|_, expiration| *expiration > now);

                let removed = initial_count - cache_write.len();
                if removed > 0 {
                    tracing::debug!(
                        removed,
                        remaining = cache_write.len(),
                        "Cleaned up expired token revocations"
                    );
                }
            })
        });

        // Load existing revocations from Redis on startup
        // Uses tokio::spawn for Redis loading since it's not Sync
        agent.after_start(|agent| {
            let cache = agent.model.cache.clone();
            let pool = agent.model.pool.clone();
            let self_handle = agent.handle().clone();
            let cleanup_interval = agent.model.cleanup_interval;

            // Spawn the Redis loading in a separate task
            tokio::spawn(async move {
                // Load existing revocations from Redis
                if let Some(pool) = &pool {
                    match load_revocations_from_redis(pool).await {
                        Ok(revocations) => {
                            let count = revocations.len();
                            let mut cache_write = cache.write().await;
                            cache_write.extend(revocations);
                            tracing::info!(
                                count,
                                "Loaded existing token revocations from Redis"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                "Failed to load token revocations from Redis, starting with empty cache"
                            );
                        }
                    }
                }

                // Spawn background cleanup task
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(cleanup_interval);
                    loop {
                        interval.tick().await;
                        self_handle.send(CleanupExpiredTokens).await;
                    }
                });

                tracing::info!("JWT revocation agent started");
            });

            AgentReply::immediate()
        });

        // Log shutdown
        agent.before_stop(|agent| {
            let cache = agent.model.cache.clone();

            AgentReply::from_async(async move {
                let count = cache.read().await.len();
                tracing::info!(
                    count,
                    "JWT revocation agent stopping with {} cached revocations",
                    count
                );
            })
        });

        let handle = agent.start().await;

        Ok(Self {
            agent_handle: handle,
            cache: cache_for_service,
        })
    }

    /// Check if a token is revoked (fast in-memory lookup)
    ///
    /// This method directly accesses the shared cache without message passing,
    /// making it suitable for hot-path middleware checks.
    pub async fn is_revoked(&self, token_id: &str) -> bool {
        let cache = self.cache.read().await;
        if let Some(expiration) = cache.get(token_id) {
            // Token is revoked if expiration is in the future
            SystemTime::now() < *expiration
        } else {
            false
        }
    }

    /// Revoke a token (write-behind to Redis)
    ///
    /// Updates the in-memory cache immediately and persists to Redis
    /// asynchronously via the agent.
    pub async fn revoke_token(&self, token_id: String, expires_at: SystemTime) {
        self.agent_handle
            .send(RevokeToken {
                token_id,
                expires_at,
            })
            .await;
    }

    /// Get the current count of cached revocations
    pub async fn cached_count(&self) -> usize {
        self.cache.read().await.len()
    }
}

/// Persist a revocation to Redis (standalone function for use in spawned tasks)
async fn persist_revocation_to_redis(
    pool: &deadpool_redis::Pool,
    token_id: &str,
    expires_at: SystemTime,
) -> anyhow::Result<()> {
    use deadpool_redis::redis::AsyncCommands;

    let mut conn = pool.get().await?;
    let key = format!("jwt:revoked:{}", token_id);

    // Calculate TTL in seconds
    let ttl_secs = expires_at
        .duration_since(SystemTime::now())
        .unwrap_or(Duration::ZERO)
        .as_secs();

    if ttl_secs > 0 {
        conn.set_ex::<_, _, ()>(&key, "1", ttl_secs).await?;
    }

    Ok(())
}

/// Load existing revocations from Redis (standalone function for use in spawned tasks)
async fn load_revocations_from_redis(
    pool: &deadpool_redis::Pool,
) -> anyhow::Result<HashMap<String, SystemTime>> {
    use deadpool_redis::redis::AsyncCommands;

    let mut conn = pool.get().await?;
    let pattern = "jwt:revoked:*";
    let keys: Vec<String> = conn.keys(pattern).await?;

    let mut revocations = HashMap::new();
    let now = SystemTime::now();

    for key in keys {
        // Get TTL for each key
        let ttl: i64 = conn.ttl(&key).await?;
        if ttl > 0 {
            let token_id = key.strip_prefix("jwt:revoked:").unwrap_or(&key);
            let expires_at = now + Duration::from_secs(ttl as u64);
            revocations.insert(token_id.to_string(), expires_at);
        }
    }

    Ok(revocations)
}
