//! Agent-based components for acton-service
//!
//! This module provides reactive, actor-based connection pool management
//! using the [`acton_reactive`] framework. These agents are the default
//! way to manage database, cache, and event connections in acton-service.
//!
//! ## Benefits
//!
//! - **No lock contention**: Pool access via async messages, not `Arc<RwLock>`
//! - **Automatic reconnection**: Built-in retry with exponential backoff
//! - **Health monitoring**: Aggregated health via `HealthMonitorAgent`
//! - **Graceful shutdown**: Coordinated cleanup via agent lifecycle hooks
//! - **Event broadcasting**: Notify subscribers of pool state changes
//!
//! ## Core Agents
//!
//! - [`DatabasePoolAgent`]: PostgreSQL connection pool management
//! - [`RedisPoolAgent`]: Redis connection pool management
//! - [`NatsPoolAgent`]: NATS client management
//! - [`HealthMonitorAgent`]: Aggregates health from all pool agents
//! - [`BackgroundWorker`]: Managed background task execution
//!
//! ## Usage
//!
//! Agents are automatically spawned when using `ServiceBuilder::build_with_agents()`:
//!
//! ```rust,ignore
//! use acton_service::prelude::*;
//!
//! let service = ServiceBuilder::new()
//!     .with_config(config)
//!     .with_routes(routes)
//!     .build_with_agents()
//!     .await?;
//!
//! service.serve().await?;
//! ```
//!
//! Pool access in handlers remains the same:
//!
//! ```rust,ignore
//! async fn handler(State(state): State<AppState<()>>) -> impl IntoResponse {
//!     // Automatically uses agent-based pool access
//!     if let Some(pool) = state.db().await {
//!         // Use pool
//!     }
//! }
//! ```

mod background_worker;
mod health;
mod messages;
mod pool;

#[cfg(feature = "cache")]
mod jwt_revocation;

// ============================================================================
// Public exports
// ============================================================================

// Background worker for managed task execution
pub use background_worker::{BackgroundWorker, BackgroundWorkerState, TaskStatus};

// Health monitoring agent
pub use health::{HealthMonitorAgent, HealthMonitorState};

// Pool agents and shared storage types
#[cfg(feature = "database")]
pub use pool::{DatabasePoolAgent, DatabasePoolState, SharedDbPool};

#[cfg(feature = "cache")]
pub use pool::{RedisPoolAgent, RedisPoolState, SharedRedisPool};

#[cfg(feature = "events")]
pub use pool::{NatsPoolAgent, NatsPoolState, SharedNatsClient};

// JWT revocation service
#[cfg(feature = "cache")]
pub use jwt_revocation::{JwtRevocationService, JwtRevocationState};

// Message types for agent communication
pub use messages::{
    AggregatedHealthResponse, CancelTask, ComponentHealth, GetAggregatedHealth, GetAllTaskStatuses,
    GetPool, GetTaskStatus, HealthStatus, PoolHealthCheck, PoolHealthResponse, PoolHealthUpdate,
    PoolReady, PoolReconnect, PoolResponse, TaskCancelled, TaskCompleted, TaskFailed,
    TaskStatusResponse, TaskSubmitted,
};

#[cfg(feature = "cache")]
pub use messages::RevokeToken;

pub mod prelude {
    //! Prelude module for convenient imports
    //!
    //! Convenient re-exports for agent-based components.
    //! This prelude includes the core acton-reactive types along with
    //! acton-service specific agent types.

    // Re-export core acton-reactive types
    pub use acton_reactive::prelude::*;

    // Re-export all agent types
    pub use super::BackgroundWorker;
    pub use super::BackgroundWorkerState;
    pub use super::HealthMonitorAgent;
    pub use super::HealthMonitorState;
    pub use super::TaskStatus;

    #[cfg(feature = "database")]
    pub use super::{DatabasePoolAgent, DatabasePoolState, SharedDbPool};

    #[cfg(feature = "cache")]
    pub use super::{RedisPoolAgent, RedisPoolState, SharedRedisPool};

    #[cfg(feature = "events")]
    pub use super::{NatsPoolAgent, NatsPoolState, SharedNatsClient};

    #[cfg(feature = "cache")]
    pub use super::{JwtRevocationService, JwtRevocationState};

    // Re-export all message types
    pub use super::{
        AggregatedHealthResponse, CancelTask, ComponentHealth, GetAggregatedHealth,
        GetAllTaskStatuses, GetPool, GetTaskStatus, HealthStatus, PoolHealthCheck,
        PoolHealthResponse, PoolHealthUpdate, PoolReady, PoolReconnect, PoolResponse,
        TaskCancelled, TaskCompleted, TaskFailed, TaskStatusResponse, TaskSubmitted,
    };

    #[cfg(feature = "cache")]
    pub use super::RevokeToken;
}
