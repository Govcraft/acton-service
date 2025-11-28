//! Agent-based components for acton-service
//!
//! This module provides reactive, actor-based alternatives to traditional
//! connection pool management patterns. Built on [`acton_reactive`], these
//! agents offer:
//!
//! - **Elimination of lock contention**: No more `Arc<RwLock<Option<T>>>` patterns
//! - **Automatic reconnection**: Built-in retry logic with state tracking
//! - **Health monitoring**: Agent-based health checks via message passing
//! - **Graceful shutdown**: Coordinated via agent lifecycle hooks
//! - **Event broadcasting**: Notify other agents of pool state changes via broker
//!
//! # Feature Flag
//!
//! This module requires the `acton-reactive` feature to be enabled:
//!
//! ```toml
//! [dependencies]
//! acton-service = { version = "0.7", features = ["acton-reactive"] }
//! ```
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use acton_service::agents::prelude::*;
//! use acton_service::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Initialize the agent runtime
//!     let runtime = ActonApp::launch();
//!
//!     // The runtime can be used to create pool agents
//!     // that manage connections reactively
//!
//!     runtime.shutdown_all().await?;
//!     Ok(())
//! }
//! ```

mod health;
#[cfg(feature = "cache")]
mod jwt_revocation;
mod messages;
mod pool;

pub mod prelude {
    //! Convenient re-exports for agent-based components
    //!
    //! This prelude includes the core acton-reactive types along with
    //! acton-service specific agent types.

    // Re-export core acton-reactive types
    pub use acton_reactive::prelude::*;

    // Re-export agent messages
    pub use super::messages::{
        AggregatedHealthResponse, ComponentHealth, GetAggregatedHealth, GetPool, HealthStatus,
        PoolHealthCheck, PoolHealthResponse, PoolHealthUpdate, PoolReady, PoolReconnect,
        PoolResponse,
    };

    #[cfg(feature = "cache")]
    pub use super::messages::RevokeToken;

    // Re-export health monitor agent
    pub use super::health::{HealthMonitorAgent, HealthMonitorState};

    // Re-export pool agent types
    #[cfg(feature = "database")]
    pub use super::pool::{DatabasePoolAgent, DatabasePoolState};

    #[cfg(feature = "cache")]
    pub use super::pool::{RedisPoolAgent, RedisPoolState};

    #[cfg(feature = "events")]
    pub use super::pool::{NatsPoolAgent, NatsPoolState};

    // Re-export JWT revocation service
    #[cfg(feature = "cache")]
    pub use super::jwt_revocation::{JwtRevocationService, JwtRevocationState};
}

// Re-export messages at module level
pub use messages::*;

// Re-export health monitor at module level
pub use health::{HealthMonitorAgent, HealthMonitorState};

// Re-export pool agents at module level
#[cfg(feature = "database")]
pub use pool::{DatabasePoolAgent, DatabasePoolState};

#[cfg(feature = "cache")]
pub use pool::{RedisPoolAgent, RedisPoolState};

#[cfg(feature = "events")]
pub use pool::{NatsPoolAgent, NatsPoolState};

// Re-export JWT revocation service at module level
#[cfg(feature = "cache")]
pub use jwt_revocation::{JwtRevocationService, JwtRevocationState};
