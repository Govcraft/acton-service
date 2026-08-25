//! Agent message types for pool management
//!
//! These messages define the communication protocol between pool agents
//! and other components in the system.
//!
//! All messages derive `Clone` and `Debug` to satisfy the `ActonMessage` trait
//! requirements via blanket implementation.

use acton_reactive::prelude::Request;

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

/// Ask a pool agent for its current connection health
///
/// Answers with whatever the agent knows right now, which for a pool that is
/// still dialling is [`HealthStatus::Connecting`]. Use
/// [`WaitForPoolReady`] to wait for the outcome instead of sampling it.
#[derive(Clone, Debug, Default)]
pub struct GetPoolHealth;

impl Request for GetPoolHealth {
    type Response = ComponentHealth;
}

/// Ask a pool agent to answer once its first connection attempt has settled
///
/// Pool agents dial on a spawned task and message themselves with the result,
/// so that result is not in the mailbox yet when `spawn` returns — a plain
/// [`GetPoolHealth`] ask right after startup races it and reports
/// [`HealthStatus::Connecting`]. This request is parked by the agent until the
/// attempt succeeds or fails, then answered with the resulting health. That
/// makes "wait until the pool is up" a barrier rather than a guessed duration.
#[derive(Clone, Debug, Default)]
pub struct WaitForPoolReady;

impl Request for WaitForPoolReady {
    type Response = ComponentHealth;
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

/// Internal message sent when the SQL Server pool connects successfully.
#[cfg(feature = "mssql")]
#[derive(Clone, Debug)]
pub(crate) struct MssqlPoolConnected {
    pub pool: crate::mssql::MssqlPool,
}

/// Internal message sent when the SQL Server pool connection fails.
#[cfg(feature = "mssql")]
#[derive(Clone, Debug, Default)]
pub(crate) struct MssqlPoolConnectionFailed {
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

/// Internal message sent when a SurrealDB client connects successfully
#[cfg(feature = "surrealdb")]
#[derive(Clone, Debug)]
pub(crate) struct SurrealDbConnected {
    pub client: std::sync::Arc<crate::surrealdb_backend::SurrealClient>,
}

/// Internal message sent when a SurrealDB client connection fails
#[cfg(feature = "surrealdb")]
#[derive(Clone, Debug, Default)]
pub(crate) struct SurrealDbConnectionFailed {
    pub error: String,
}

/// Internal message sent when a ClickHouse client connects successfully
#[cfg(feature = "clickhouse")]
#[derive(Clone)]
pub(crate) struct ClickHouseClientConnected {
    pub client: clickhouse::Client,
}

#[cfg(feature = "clickhouse")]
impl std::fmt::Debug for ClickHouseClientConnected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClickHouseClientConnected").finish()
    }
}

/// Internal message sent when a ClickHouse client connection fails
#[cfg(feature = "clickhouse")]
#[derive(Clone, Debug, Default)]
pub(crate) struct ClickHouseClientConnectionFailed {
    pub error: String,
}

// =============================================================================
// Background Worker Agent messages
// =============================================================================

/// Internal message: a task has been accepted and should be tracked
///
/// Sent before the task is spawned so the agent can never receive a
/// [`TaskCompleted`] for an ID it has not registered.
#[derive(Clone, Debug, Default)]
pub(crate) struct RegisterTask {
    /// The task ID being registered
    pub task_id: String,
    /// Token that cancels this task specifically
    pub cancellation_token: tokio_util::sync::CancellationToken,
}

/// Internal message: a task reached a terminal state
///
/// Sent by the spawned task itself, so status has exactly one writer.
#[derive(Clone, Debug, Default)]
pub(crate) struct TaskCompleted {
    /// The task ID that finished
    pub task_id: String,
    /// The status it finished in
    pub status: super::background_worker::TaskStatus,
}

/// Message to cancel a running background task
#[derive(Clone, Debug, Default)]
pub struct CancelTask {
    /// The task ID to cancel
    pub task_id: String,
}

/// Message asking to be told when a task reaches a terminal state
#[derive(Clone, Debug, Default)]
pub struct WaitForTask {
    /// The task ID to wait on
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

/// Message asking the worker to drop records for tasks that have finished
#[derive(Clone, Debug, Default)]
pub struct CleanupFinishedTasks;

/// Response containing task status information
#[derive(Clone, Debug, Default)]
pub struct TaskStatusResponse {
    /// The task ID
    pub task_id: String,
    /// Current status of the task
    pub status: super::background_worker::TaskStatus,
}

/// Lets callers read a single task's status with
/// [`ask`](acton_reactive::prelude::ActorHandleInterface::ask). `None` means no
/// such task is tracked, which is distinct from a task sitting in
/// [`TaskStatus::Pending`](super::background_worker::TaskStatus::Pending).
///
/// ```rust,ignore
/// let response = worker.handle().ask(GetTaskStatus { task_id: "job".into() }).await?;
/// ```
impl Request for GetTaskStatus {
    type Response = Option<TaskStatusResponse>;
}

/// Lets callers read every tracked task's status with
/// [`ask`](acton_reactive::prelude::ActorHandleInterface::ask).
impl Request for GetAllTaskStatuses {
    type Response = Vec<TaskStatusResponse>;
}

/// Resolves once the named task reaches a terminal state, or immediately with
/// `None` if no such task is tracked. The agent parks the reply envelope until
/// the task reports in, so callers wait rather than poll.
impl Request for WaitForTask {
    type Response = Option<TaskStatusResponse>;
}

/// Answers with the number of finished task records dropped.
impl Request for CleanupFinishedTasks {
    type Response = usize;
}
