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

/// Health status of a pool
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum HealthStatus {
    /// Pool is healthy and operational
    Healthy,
    /// Pool is degraded but operational
    Degraded,
    /// Pool is unhealthy/disconnected
    #[default]
    Unhealthy,
    /// Pool is in the process of connecting
    Connecting,
}

/// Broadcast event for pool health status updates
///
/// Pool agents broadcast this message via the `AgentBroker` whenever
/// their health status changes. The `HealthMonitorAgent` subscribes
/// to these updates to maintain aggregated health state.
#[derive(Clone, Debug, Default)]
pub struct PoolHealthUpdate {
    /// The type of pool (e.g., "database", "redis", "nats")
    pub pool_type: String,
    /// Current health status
    pub status: HealthStatus,
    /// Human-readable status message
    pub message: String,
}

impl PoolHealthUpdate {
    /// Create a healthy status update
    #[must_use]
    pub fn healthy(pool_type: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            pool_type: pool_type.into(),
            status: HealthStatus::Healthy,
            message: message.into(),
        }
    }

    /// Create an unhealthy status update
    #[must_use]
    pub fn unhealthy(pool_type: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            pool_type: pool_type.into(),
            status: HealthStatus::Unhealthy,
            message: message.into(),
        }
    }

    /// Create a connecting status update
    #[must_use]
    pub fn connecting(pool_type: impl Into<String>) -> Self {
        Self {
            pool_type: pool_type.into(),
            status: HealthStatus::Connecting,
            message: "Connecting...".to_string(),
        }
    }
}

/// Request for aggregated health status from the HealthMonitorAgent
///
/// Send this message to the HealthMonitorAgent to receive an
/// [`AggregatedHealthResponse`] with the current health of all pools.
#[derive(Clone, Debug, Default)]
pub struct GetAggregatedHealth;

/// Response containing aggregated health status from all pools
#[derive(Clone, Debug, Default)]
pub struct AggregatedHealthResponse {
    /// Overall health status (unhealthy if any component is unhealthy)
    pub overall_healthy: bool,
    /// Individual pool health statuses
    pub components: Vec<ComponentHealth>,
}

/// Health status of a single component/pool
#[derive(Clone, Debug, Default)]
pub struct ComponentHealth {
    /// Component name (e.g., "database", "redis", "nats")
    pub name: String,
    /// Health status
    pub status: HealthStatus,
    /// Status message
    pub message: String,
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

// =============================================================================
// JWT Revocation Agent messages
// =============================================================================

/// Message to revoke a JWT token
///
/// The agent will update its in-memory cache immediately and then
/// persist the revocation to Redis asynchronously (write-behind pattern).
#[cfg(feature = "cache")]
#[derive(Clone, Debug)]
pub struct RevokeToken {
    /// The JWT token ID (jti claim)
    pub token_id: String,
    /// When the token expires (revocation can be cleaned up after this)
    pub expires_at: std::time::SystemTime,
}

/// Internal message to trigger cleanup of expired revocations
#[cfg(feature = "cache")]
#[derive(Clone, Debug, Default)]
pub(crate) struct CleanupExpiredTokens;

// =============================================================================
// Background Worker Agent messages
// =============================================================================

/// Message to cancel a running background task
#[derive(Clone, Debug, Default)]
pub struct CancelTask {
    /// The task ID to cancel
    pub task_id: String,
}

/// Message to query the status of a specific task
#[derive(Clone, Debug, Default)]
pub struct GetTaskStatus {
    /// The task ID to query
    pub task_id: String,
}

/// Message to query the status of all tasks
#[derive(Clone, Debug, Default)]
pub struct GetAllTaskStatuses;

/// Response containing task status information
#[derive(Clone, Debug, Default)]
pub struct TaskStatusResponse {
    /// The task ID
    pub task_id: String,
    /// Current status of the task
    pub status: super::background_worker::TaskStatus,
}

// =============================================================================
// Background Worker broadcast events
// =============================================================================

/// Broadcast event when a task is submitted
#[derive(Clone, Debug, Default)]
pub struct TaskSubmitted {
    /// The task ID that was submitted
    pub task_id: String,
}

/// Broadcast event when a task completes successfully
#[derive(Clone, Debug, Default)]
pub struct TaskCompleted {
    /// The task ID that completed
    pub task_id: String,
}

/// Broadcast event when a task fails
#[derive(Clone, Debug, Default)]
pub struct TaskFailed {
    /// The task ID that failed
    pub task_id: String,
    /// Error message
    pub error: String,
}

/// Broadcast event when a task is cancelled
#[derive(Clone, Debug, Default)]
pub struct TaskCancelled {
    /// The task ID that was cancelled
    pub task_id: String,
}
