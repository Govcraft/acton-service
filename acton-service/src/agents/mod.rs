//! Internal agent-based components for acton-service
//!
//! This module provides reactive, actor-based connection pool management
//! and background task execution. Pool agents and health monitoring are
//! used internally by the framework - users interact with the simpler
//! `AppState` API.
//!
//! # User-Facing Types
//!
//! The following types are exported for user use:
//! - [`BackgroundWorker`]: Managed alternative to `tokio::spawn` with tracking
//! - [`TaskStatus`]: Status of background tasks
//!
//! # Internal Types
//!
//! Pool agents, health monitoring, and JWT revocation are internal
//! implementation details reserved for future internal framework use.
//! They are not currently wired up to the AppState/ServiceBuilder flow.

mod background_worker;

// Internal modules - reserved for future internal use
// These will eventually replace Arc<RwLock<Option<T>>> patterns in AppState
#[allow(dead_code)]
mod health;
#[cfg(feature = "cache")]
#[allow(dead_code)]
mod jwt_revocation;
#[allow(dead_code)]
mod messages;
#[allow(dead_code)]
mod pool;

// ============================================================================
// Public exports (user-facing)
// ============================================================================

// BackgroundWorker is a useful utility for managed background tasks
pub use background_worker::{BackgroundWorker, TaskStatus};
