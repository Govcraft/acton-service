//! Pool agent implementations for reactive connection management
//!
//! These agents manage connection pools using the actor pattern, providing
//! automatic reconnection, health monitoring, and graceful shutdown.
//!
//! ## Shared State Architecture
//!
//! Pool agents receive a shared `Arc<RwLock<Option<Pool>>>` reference during spawn.
//! When the pool connects, the agent updates this shared storage, allowing
//! `AppState::db()` etc. to access pools directly without message passing overhead.
//!
//! ## Pattern: Spawn and Send Message
//!
//! Because acton-reactive requires `Send + Sync` futures for handlers, but
//! database/cache/event connection futures are typically only `Send`, we use
//! the "spawn and send message to self" pattern:
//!
//! 1. Spawn the non-Sync connection work with `tokio::spawn`
//! 2. Send a message to self when the connection completes
//! 3. Handle that message in a `mutate_on` handler to update agent state

// ============================================================================
// Database Pool Agent
// ============================================================================

#[cfg(feature = "database")]
use std::sync::Arc;
#[cfg(feature = "database")]
use tokio::sync::RwLock;
#[cfg(feature = "database")]
use acton_reactive::prelude::*;
#[cfg(feature = "database")]
use super::messages::{DatabasePoolConnected, DatabasePoolConnectionFailed};

/// Shared pool storage type for database connections
#[cfg(feature = "database")]
pub type SharedDbPool = Arc<RwLock<Option<sqlx::PgPool>>>;

/// State for the database pool agent
#[cfg(feature = "database")]
#[derive(Debug, Default)]
pub struct DatabasePoolState {
    /// The underlying PostgreSQL connection pool
    pub pool: Option<sqlx::PgPool>,
    /// Configuration for the database connection
    pub config: Option<crate::config::DatabaseConfig>,
    /// Whether the agent is currently attempting to connect
    pub connecting: bool,
    /// Shared storage that AppState reads from directly
    pub shared_pool: Option<SharedDbPool>,
}

/// Agent-based PostgreSQL connection pool manager
///
/// This agent manages a database connection pool using message passing
/// instead of shared mutable state. Benefits include:
///
/// - **No lock contention**: Pool access via shared state with minimal locking
/// - **Automatic connection**: Connection established on agent start
/// - **Health monitoring**: Broadcasts health status via message broker
/// - **Graceful shutdown**: Pool closed on agent stop
#[cfg(feature = "database")]
pub struct DatabasePoolAgent;

#[cfg(feature = "database")]
impl DatabasePoolAgent {
    /// Spawn a new database pool agent with the given configuration
    ///
    /// The agent will immediately begin connecting to the database.
    /// Subscribe to [`PoolReady`] events to be notified when the pool is available.
    ///
    /// # Arguments
    ///
    /// * `runtime` - The agent runtime to spawn into
    /// * `config` - Database connection configuration
    /// * `shared_pool` - Shared storage that will be updated when the pool connects.
    ///   `AppState::db()` reads the pool directly from this storage.
    pub async fn spawn(
        runtime: &mut AgentRuntime,
        config: crate::config::DatabaseConfig,
        shared_pool: Option<SharedDbPool>,
    ) -> anyhow::Result<AgentHandle> {
        let mut agent = runtime.new_agent::<DatabasePoolState>();

        // Initialize state before starting
        agent.model.config = Some(config);
        agent.model.connecting = true;
        agent.model.shared_pool = shared_pool;

        // Handle pool connected message (sent from spawned task)
        agent.mutate_on::<DatabasePoolConnected>(|agent, envelope| {
            let pool = envelope.message().pool.clone();
            agent.model.pool = Some(pool.clone());
            agent.model.connecting = false;

            // Update shared storage if configured
            let shared_pool = agent.model.shared_pool.clone();

            AgentReply::from_async(async move {
                // Update shared storage for direct AppState access
                if let Some(shared) = shared_pool {
                    *shared.write().await = Some(pool);
                    tracing::info!("Database pool connected and stored in shared state");
                } else {
                    tracing::info!("Database pool connected (no shared state)");
                }
            })
        });

        // Handle pool connection failed message
        agent.mutate_on::<DatabasePoolConnectionFailed>(|agent, envelope| {
            let error_msg = envelope.message().error.clone();
            agent.model.connecting = false;
            tracing::error!("Database pool connection failed: {}", error_msg);

            AgentReply::immediate()
        });

        // Initialize connection on startup using spawn pattern
        agent.after_start(|agent| {
            let config = agent.model.config.clone();
            let self_handle = agent.handle().clone();

            AgentReply::from_async(async move {
                if let Some(cfg) = config {
                    tracing::info!("Database pool agent starting, connecting to database...");

                    // Spawn the non-Sync connection work
                    let result = tokio::spawn(async move { crate::database::create_pool(&cfg).await })
                        .await;

                    match result {
                        Ok(Ok(pool)) => {
                            self_handle.send(DatabasePoolConnected { pool }).await;
                        }
                        Ok(Err(e)) => {
                            self_handle
                                .send(DatabasePoolConnectionFailed {
                                    error: e.to_string(),
                                })
                                .await;
                        }
                        Err(e) => {
                            self_handle
                                .send(DatabasePoolConnectionFailed {
                                    error: format!("Connection task panicked: {}", e),
                                })
                                .await;
                        }
                    }
                }
            })
        });

        // Graceful cleanup on shutdown
        agent.before_stop(|agent| {
            let pool = agent.model.pool.clone();
            AgentReply::from_async(async move {
                if let Some(p) = pool {
                    tracing::info!("Database pool agent stopping, closing connections...");
                    p.close().await;
                    tracing::info!("Database pool closed");
                }
            })
        });

        let handle = agent.start().await;
        Ok(handle)
    }
}

// ============================================================================
// Redis Pool Agent
// ============================================================================

#[cfg(all(feature = "cache", not(feature = "database")))]
use std::sync::Arc;
#[cfg(all(feature = "cache", not(feature = "database")))]
use tokio::sync::RwLock;
#[cfg(all(feature = "cache", not(feature = "database")))]
use acton_reactive::prelude::*;
#[cfg(feature = "cache")]
use super::messages::{RedisPoolConnected, RedisPoolConnectionFailed};

/// Shared pool storage type for Redis connections
#[cfg(feature = "cache")]
pub type SharedRedisPool = Arc<RwLock<Option<deadpool_redis::Pool>>>;

/// State for the Redis pool agent
#[cfg(feature = "cache")]
#[derive(Debug, Default)]
pub struct RedisPoolState {
    /// The underlying Redis connection pool
    pub pool: Option<deadpool_redis::Pool>,
    /// Configuration for the Redis connection
    pub config: Option<crate::config::RedisConfig>,
    /// Whether the agent is currently attempting to connect
    pub connecting: bool,
    /// Shared storage that AppState reads from directly
    pub shared_pool: Option<SharedRedisPool>,
}

/// Agent-based Redis connection pool manager
///
/// Similar to [`DatabasePoolAgent`], this agent manages a Redis connection
/// pool with automatic connection and graceful shutdown.
#[cfg(feature = "cache")]
pub struct RedisPoolAgent;

#[cfg(feature = "cache")]
impl RedisPoolAgent {
    /// Spawn a new Redis pool agent with the given configuration
    ///
    /// # Arguments
    ///
    /// * `runtime` - The agent runtime to spawn into
    /// * `config` - Redis connection configuration
    /// * `shared_pool` - Shared storage that will be updated when the pool connects.
    pub async fn spawn(
        runtime: &mut AgentRuntime,
        config: crate::config::RedisConfig,
        shared_pool: Option<SharedRedisPool>,
    ) -> anyhow::Result<AgentHandle> {
        let mut agent = runtime.new_agent::<RedisPoolState>();

        // Initialize state before starting
        agent.model.config = Some(config);
        agent.model.connecting = true;
        agent.model.shared_pool = shared_pool;

        // Handle pool connected message
        agent.mutate_on::<RedisPoolConnected>(|agent, envelope| {
            let pool = envelope.message().pool.clone();
            agent.model.pool = Some(pool.clone());
            agent.model.connecting = false;

            // Update shared storage if configured
            let shared_pool = agent.model.shared_pool.clone();

            AgentReply::from_async(async move {
                // Update shared storage for direct AppState access
                if let Some(shared) = shared_pool {
                    *shared.write().await = Some(pool);
                    tracing::info!("Redis pool connected and stored in shared state");
                } else {
                    tracing::info!("Redis pool connected (no shared state)");
                }
            })
        });

        // Handle pool connection failed message
        agent.mutate_on::<RedisPoolConnectionFailed>(|agent, envelope| {
            let error_msg = envelope.message().error.clone();
            agent.model.connecting = false;
            tracing::error!("Redis pool connection failed: {}", error_msg);

            AgentReply::immediate()
        });

        // Initialize connection on startup
        agent.after_start(|agent| {
            let config = agent.model.config.clone();
            let self_handle = agent.handle().clone();

            AgentReply::from_async(async move {
                if let Some(cfg) = config {
                    tracing::info!("Redis pool agent starting, connecting to Redis...");

                    let result =
                        tokio::spawn(async move { crate::cache::create_pool(&cfg).await }).await;

                    match result {
                        Ok(Ok(pool)) => {
                            self_handle.send(RedisPoolConnected { pool }).await;
                        }
                        Ok(Err(e)) => {
                            self_handle
                                .send(RedisPoolConnectionFailed {
                                    error: e.to_string(),
                                })
                                .await;
                        }
                        Err(e) => {
                            self_handle
                                .send(RedisPoolConnectionFailed {
                                    error: format!("Connection task panicked: {}", e),
                                })
                                .await;
                        }
                    }
                }
            })
        });

        // Cleanup on shutdown
        agent.before_stop(|_agent| {
            AgentReply::from_async(async move {
                tracing::info!("Redis pool agent stopping");
            })
        });

        let handle = agent.start().await;
        Ok(handle)
    }
}

// ============================================================================
// NATS Pool Agent
// ============================================================================

#[cfg(all(feature = "events", not(feature = "database"), not(feature = "cache")))]
use std::sync::Arc;
#[cfg(all(feature = "events", not(feature = "database"), not(feature = "cache")))]
use tokio::sync::RwLock;
#[cfg(all(feature = "events", not(feature = "database"), not(feature = "cache")))]
use acton_reactive::prelude::*;
#[cfg(feature = "events")]
use super::messages::{NatsClientConnected, NatsClientConnectionFailed};

/// Shared client storage type for NATS connections
#[cfg(feature = "events")]
pub type SharedNatsClient = Arc<RwLock<Option<async_nats::Client>>>;

/// State for the NATS pool agent
#[cfg(feature = "events")]
#[derive(Debug, Default)]
pub struct NatsPoolState {
    /// The underlying NATS client
    pub client: Option<async_nats::Client>,
    /// Configuration for the NATS connection
    pub config: Option<crate::config::NatsConfig>,
    /// Whether the agent is currently attempting to connect
    pub connecting: bool,
    /// Shared storage that AppState reads from directly
    pub shared_client: Option<SharedNatsClient>,
}

/// Agent-based NATS client manager
///
/// Manages a NATS client connection with automatic connection and graceful shutdown.
#[cfg(feature = "events")]
pub struct NatsPoolAgent;

#[cfg(feature = "events")]
impl NatsPoolAgent {
    /// Spawn a new NATS pool agent with the given configuration
    ///
    /// # Arguments
    ///
    /// * `runtime` - The agent runtime to spawn into
    /// * `config` - NATS connection configuration
    /// * `shared_client` - Shared storage that will be updated when the client connects.
    pub async fn spawn(
        runtime: &mut AgentRuntime,
        config: crate::config::NatsConfig,
        shared_client: Option<SharedNatsClient>,
    ) -> anyhow::Result<AgentHandle> {
        let mut agent = runtime.new_agent::<NatsPoolState>();

        // Initialize state before starting
        agent.model.config = Some(config);
        agent.model.connecting = true;
        agent.model.shared_client = shared_client;

        // Handle client connected message
        agent.mutate_on::<NatsClientConnected>(|agent, envelope| {
            let client = envelope.message().client.clone();
            agent.model.client = Some(client.clone());
            agent.model.connecting = false;

            // Update shared storage if configured
            let shared_client = agent.model.shared_client.clone();

            AgentReply::from_async(async move {
                // Update shared storage for direct AppState access
                if let Some(shared) = shared_client {
                    *shared.write().await = Some(client);
                    tracing::info!("NATS client connected and stored in shared state");
                } else {
                    tracing::info!("NATS client connected (no shared state)");
                }
            })
        });

        // Handle client connection failed message
        agent.mutate_on::<NatsClientConnectionFailed>(|agent, envelope| {
            let error_msg = envelope.message().error.clone();
            agent.model.connecting = false;
            tracing::error!("NATS client connection failed: {}", error_msg);

            AgentReply::immediate()
        });

        // Initialize connection on startup
        agent.after_start(|agent| {
            let config = agent.model.config.clone();
            let self_handle = agent.handle().clone();

            AgentReply::from_async(async move {
                if let Some(cfg) = config {
                    tracing::info!("NATS pool agent starting, connecting to NATS...");

                    let result =
                        tokio::spawn(async move { crate::events::create_client(&cfg).await }).await;

                    match result {
                        Ok(Ok(client)) => {
                            self_handle.send(NatsClientConnected { client }).await;
                        }
                        Ok(Err(e)) => {
                            self_handle
                                .send(NatsClientConnectionFailed {
                                    error: e.to_string(),
                                })
                                .await;
                        }
                        Err(e) => {
                            self_handle
                                .send(NatsClientConnectionFailed {
                                    error: format!("Connection task panicked: {}", e),
                                })
                                .await;
                        }
                    }
                }
            })
        });

        // Close client on shutdown
        agent.before_stop(|agent| {
            let client = agent.model.client.clone();
            AgentReply::from_async(async move {
                if let Some(c) = client {
                    tracing::info!("NATS pool agent stopping, closing connection...");
                    drop(c);
                    tracing::info!("NATS client closed");
                }
            })
        });

        let handle = agent.start().await;
        Ok(handle)
    }
}
