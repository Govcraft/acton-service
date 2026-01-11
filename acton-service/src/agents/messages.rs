//! Agent message types for pool management
//!
//! These messages define the communication protocol between pool agents
//! and other components in the system.
//!
//! All messages derive `Clone` and `Debug` to satisfy the `ActonMessage` trait
//! requirements via blanket implementation.

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

/// Internal message sent when a Turso database connects successfully
#[cfg(feature = "turso")]
#[derive(Clone, Debug)]
pub(crate) struct TursoDbConnected {
    pub db: std::sync::Arc<libsql::Database>,
}

/// Internal message sent when a Turso database connection fails
#[cfg(feature = "turso")]
#[derive(Clone, Debug, Default)]
pub(crate) struct TursoDbConnectionFailed {
    pub error: String,
}

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
