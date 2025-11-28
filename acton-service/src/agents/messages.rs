//! Agent message types for pool management
//!
//! These messages define the communication protocol between pool agents
//! and other components in the system.
//!
//! All messages derive `Clone` and `Debug` to satisfy the `ActonMessage` trait
//! requirements via blanket implementation.

/// Request to get the current connection pool
///
/// Send this message to a pool agent to retrieve the underlying
/// connection pool if available.
#[derive(Clone, Debug, Default)]
pub struct GetPool;

/// Request to check the health status of a pool
///
/// The agent will respond with a [`PoolHealthResponse`] containing
/// the current health status and any diagnostic information.
#[derive(Clone, Debug, Default)]
pub struct PoolHealthCheck;

/// Request to trigger a reconnection attempt
///
/// Use this message to force the pool agent to attempt reconnection
/// to the underlying service (database, Redis, NATS).
#[derive(Clone, Debug, Default)]
pub struct PoolReconnect;

/// Response to a [`PoolHealthCheck`] request
///
/// Contains health status information about the connection pool.
#[derive(Clone, Debug, Default)]
pub struct PoolHealthResponse {
    /// Whether the pool is currently healthy
    pub healthy: bool,
    /// Human-readable status message
    pub message: String,
    /// Number of active connections (if applicable)
    pub active_connections: Option<u32>,
    /// Number of idle connections (if applicable)
    pub idle_connections: Option<u32>,
}

impl PoolHealthResponse {
    /// Create a healthy response
    #[must_use]
    pub fn healthy(message: impl Into<String>) -> Self {
        Self {
            healthy: true,
            message: message.into(),
            active_connections: None,
            idle_connections: None,
        }
    }

    /// Create an unhealthy response
    #[must_use]
    pub fn unhealthy(message: impl Into<String>) -> Self {
        Self {
            healthy: false,
            message: message.into(),
            active_connections: None,
            idle_connections: None,
        }
    }

    /// Add connection count information
    #[must_use]
    pub fn with_connections(mut self, active: u32, idle: u32) -> Self {
        self.active_connections = Some(active);
        self.idle_connections = Some(idle);
        self
    }
}

/// Response indicating whether a pool is available
///
/// This is the response to a [`GetPool`] request.
#[derive(Clone, Debug)]
pub enum PoolResponse<P: Clone + Send + Sync + 'static> {
    /// Pool is available and ready for use
    Available(P),
    /// Pool is not currently connected
    NotConnected,
    /// Pool is connecting (lazy initialization in progress)
    Connecting,
}

impl<P: Clone + Send + Sync + 'static> Default for PoolResponse<P> {
    fn default() -> Self {
        Self::NotConnected
    }
}

/// Broadcast event indicating a pool is ready
///
/// This event is broadcast via the `AgentBroker` when a pool
/// successfully connects or reconnects.
#[derive(Clone, Debug, Default)]
pub struct PoolReady {
    /// The type of pool that is ready (e.g., "database", "redis", "nats")
    pub pool_type: String,
}

impl PoolReady {
    /// Create a new pool ready event
    #[must_use]
    pub fn new(pool_type: impl Into<String>) -> Self {
        Self {
            pool_type: pool_type.into(),
        }
    }

    /// Create a database pool ready event
    #[must_use]
    pub fn database() -> Self {
        Self::new("database")
    }

    /// Create a Redis pool ready event
    #[must_use]
    pub fn redis() -> Self {
        Self::new("redis")
    }

    /// Create a NATS pool ready event
    #[must_use]
    pub fn nats() -> Self {
        Self::new("nats")
    }
}

// =============================================================================
// Internal messages for pool connection state management
// These are sent by spawned connection tasks back to the agent
// =============================================================================

/// Internal message sent when a database pool connects successfully
#[cfg(feature = "database")]
#[derive(Clone, Debug)]
pub(crate) struct DatabasePoolConnected {
    pub pool: sqlx::PgPool,
}

/// Internal message sent when a database pool connection fails
#[cfg(feature = "database")]
#[derive(Clone, Debug, Default)]
pub(crate) struct DatabasePoolConnectionFailed {
    pub error: String,
}

/// Internal message sent when a Redis pool connects successfully
#[cfg(feature = "cache")]
#[derive(Clone, Debug)]
pub(crate) struct RedisPoolConnected {
    pub pool: deadpool_redis::Pool,
}

/// Internal message sent when a Redis pool connection fails
#[cfg(feature = "cache")]
#[derive(Clone, Debug, Default)]
pub(crate) struct RedisPoolConnectionFailed {
    pub error: String,
}

/// Internal message sent when a NATS client connects successfully
#[cfg(feature = "events")]
#[derive(Clone, Debug)]
pub(crate) struct NatsClientConnected {
    pub client: async_nats::Client,
}

/// Internal message sent when a NATS client connection fails
#[cfg(feature = "events")]
#[derive(Clone, Debug, Default)]
pub(crate) struct NatsClientConnectionFailed {
    pub error: String,
}
