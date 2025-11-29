//! Internal agent-based components for acton-service
//!
//! This module provides reactive, actor-based connection pool management
//! using the [`acton_reactive`] framework. Agents are an **internal
//! implementation detail** - users interact with `AppState` and `ServiceBuilder`,
//! not with agents directly.
//!
//! ## How It Works
//!
//! When you call `ServiceBuilder::build()`, the framework:
//!
//! 1. Spawns pool agents internally to manage connections
//! 2. Agents handle reconnection, health monitoring, graceful shutdown
//! 3. Pools are made available via `state.db()`, `state.redis()`, etc.
//!
//! Users don't need to know agents exist - they just work behind the scenes.

mod background_worker;
mod messages;
mod pool;

// ============================================================================
// Public exports - types users may actually need
// ============================================================================

// Background worker - users can use this for managed background tasks
pub use background_worker::{BackgroundWorker, TaskStatus};

// Health status types - users may want to check aggregated health
pub use messages::{AggregatedHealthResponse, ComponentHealth, HealthStatus};

// Task status response for BackgroundWorker users
pub use messages::TaskStatusResponse;

// ============================================================================
// Internal exports - pool agents and shared storage for ServiceBuilder
// ============================================================================

// Pool agents - spawned by ServiceBuilder::build()
#[cfg(feature = "database")]
pub(crate) use pool::{DatabasePoolAgent, SharedDbPool};

#[cfg(feature = "cache")]
pub(crate) use pool::{RedisPoolAgent, SharedRedisPool};

#[cfg(feature = "events")]
pub(crate) use pool::{NatsPoolAgent, SharedNatsClient};
