//! Connection pool health monitoring

use serde::{Deserialize, Serialize};

/// Database connection pool health metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg(feature = "database")]
pub struct DatabasePoolHealth {
    /// Total number of connections in the pool
    pub size: u32,

    /// Number of idle connections available
    pub idle: usize,

    /// Maximum pool size configured
    pub max_size: u32,

    /// Minimum pool size configured
    pub min_size: u32,

    /// Whether the pool is healthy
    pub healthy: bool,

    /// Pool utilization percentage (0-100)
    pub utilization_percent: f32,
}

#[cfg(feature = "database")]
impl DatabasePoolHealth {
    /// Create health metrics from a PostgreSQL pool
    pub fn from_pool(pool: &sqlx::PgPool, config: &crate::config::DatabaseConfig) -> Self {
        let size = pool.size();
        let idle = pool.num_idle();
        let max_size = config.max_connections;
        let min_size = config.min_connections;

        let utilization_percent = if max_size > 0 {
            ((size as f32 / max_size as f32) * 100.0).min(100.0)
        } else {
            0.0
        };

        // Pool is healthy if not at max capacity
        let healthy = size < max_size;

        Self {
            size,
            idle,
            max_size,
            min_size,
            healthy,
            utilization_percent,
        }
    }
}

/// Redis connection pool health metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg(feature = "cache")]
pub struct RedisPoolHealth {
    /// Maximum pool size configured
    pub max_size: usize,

    /// Whether the pool is available
    pub available: bool,

    /// Pool status description
    pub status: String,
}

#[cfg(feature = "cache")]
impl RedisPoolHealth {
    /// Create health metrics from a Redis pool
    pub fn from_pool(pool: &deadpool_redis::Pool, config: &crate::config::RedisConfig) -> Self {
        let max_size = config.max_connections;
        let status = pool.status();

        // Pool is available if it's not closed
        let available = status.size > 0 || status.available > 0;

        Self {
            max_size,
            available,
            status: format!("size={}, available={}", status.size, status.available),
        }
    }
}

/// NATS client health status
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg(feature = "events")]
pub struct NatsClientHealth {
    /// Whether the client is connected
    pub connected: bool,

    /// Server URL
    pub server_url: String,

    /// Client name if configured
    pub client_name: Option<String>,
}

#[cfg(feature = "events")]
impl NatsClientHealth {
    /// Create health status from a NATS client
    pub fn from_client(client: &async_nats::Client, config: &crate::config::NatsConfig) -> Self {
        Self {
            connected: client.connection_state() == async_nats::connection::State::Connected,
            server_url: config.url.clone(),
            client_name: config.name.clone(),
        }
    }
}

/// Overall pool health summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolHealthSummary {
    /// Database pool health
    #[cfg(feature = "database")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<DatabasePoolHealth>,

    /// Redis pool health
    #[cfg(feature = "cache")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redis: Option<RedisPoolHealth>,

    /// NATS client health
    #[cfg(feature = "events")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nats: Option<NatsClientHealth>,

    /// Overall healthy status
    pub healthy: bool,
}

impl PoolHealthSummary {
    /// Create a new pool health summary
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "database")]
            database: None,
            #[cfg(feature = "cache")]
            redis: None,
            #[cfg(feature = "events")]
            nats: None,
            healthy: true,
        }
    }

    /// Check if all pools are healthy
    pub fn is_healthy(&self) -> bool {
        let database_healthy = {
            #[cfg(feature = "database")]
            { self.database.as_ref().map_or(true, |db| db.healthy) }
            #[cfg(not(feature = "database"))]
            { true }
        };

        let cache_healthy = {
            #[cfg(feature = "cache")]
            { self.redis.as_ref().map_or(true, |redis| redis.available) }
            #[cfg(not(feature = "cache"))]
            { true }
        };

        let events_healthy = {
            #[cfg(feature = "events")]
            { self.nats.as_ref().map_or(true, |nats| nats.connected) }
            #[cfg(not(feature = "events"))]
            { true }
        };

        database_healthy && cache_healthy && events_healthy
    }
}

impl Default for PoolHealthSummary {
    fn default() -> Self {
        Self::new()
    }
}
