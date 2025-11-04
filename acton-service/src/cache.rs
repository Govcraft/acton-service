//! Redis connection pool management

#[cfg(feature = "cache")]
use deadpool_redis::{Config as DeadpoolConfig, Pool, Runtime};
use std::time::Duration;

use crate::{config::RedisConfig, error::Result};

/// Create a Redis connection pool with retry logic
///
/// This is an internal function used by AppStateBuilder.
/// It will retry connection attempts based on the configuration.
#[cfg(feature = "cache")]
pub(crate) async fn create_pool(config: &RedisConfig) -> Result<Pool> {
    create_pool_with_retries(config, config.max_retries).await
}

/// Create a Redis connection pool with configurable retries
///
/// Uses exponential backoff strategy for retries
#[cfg(feature = "cache")]
async fn create_pool_with_retries(config: &RedisConfig, max_retries: u32) -> Result<Pool> {
    let mut attempt = 0;
    let base_delay = Duration::from_secs(config.retry_delay_secs);

    loop {
        match try_create_pool(config).await {
            Ok(pool) => {
                if attempt > 0 {
                    tracing::info!(
                        "Redis connection established after {} attempt(s)",
                        attempt + 1
                    );
                } else {
                    tracing::info!(
                        "Redis connection pool created: max_connections={}",
                        config.max_connections
                    );
                }
                return Ok(pool);
            }
            Err(e) => {
                attempt += 1;

                if attempt > max_retries {
                    tracing::error!(
                        "Failed to connect to Redis after {} attempts: {}",
                        max_retries + 1,
                        e
                    );
                    return Err(e);
                }

                // Calculate exponential backoff
                let delay_multiplier = 2_u32.pow(attempt.saturating_sub(1));
                let delay = base_delay * delay_multiplier;

                tracing::warn!(
                    "Redis connection attempt {} failed: {}. Retrying in {:?}...",
                    attempt,
                    e,
                    delay
                );

                tokio::time::sleep(delay).await;
            }
        }
    }
}

/// Attempt to create a Redis pool (single try)
#[cfg(feature = "cache")]
async fn try_create_pool(config: &RedisConfig) -> Result<Pool> {
    let cfg = DeadpoolConfig::from_url(&config.url);

    let pool = cfg
        .builder()
        .map_err(|e| crate::error::Error::Internal(format!("Failed to build Redis pool: {}", e)))?
        .max_size(config.max_connections)
        .runtime(Runtime::Tokio1)
        .build()
        .map_err(|e| crate::error::Error::Internal(format!("Failed to create Redis pool: {}", e)))?;

    // Test the connection
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::Error::Internal(format!("Failed to get Redis connection: {}", e)))?;
    drop(conn);

    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redis_config() {
        let config = RedisConfig {
            url: "redis://localhost:6379".to_string(),
            max_connections: 20,
            connection_timeout_secs: 10,
            max_retries: 5,
            retry_delay_secs: 2,
            optional: false,
            lazy_init: true,
        };

        assert_eq!(config.max_connections, 20);
        assert_eq!(config.max_retries, 5);
        assert!(config.lazy_init);
    }
}
