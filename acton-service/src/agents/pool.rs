//! Pool agent implementations for reactive connection management
//!
//! These agents provide message-passing based alternatives to traditional
//! `Arc<RwLock<Option<Pool>>>` patterns, eliminating lock contention and
//! enabling graceful lifecycle management.
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

use acton_reactive::prelude::*;

use super::messages::{GetPool, PoolHealthCheck, PoolHealthResponse, PoolReady, PoolReconnect};

/// Maximum number of reconnection attempts before giving up
const MAX_RECONNECT_ATTEMPTS: u32 = 10;

// ============================================================================
// Database Pool Agent
// ============================================================================

#[cfg(feature = "database")]
use super::messages::{DatabasePoolConnected, DatabasePoolConnectionFailed};

/// State for the database pool agent
#[cfg(feature = "database")]
#[derive(Debug, Default)]
pub struct DatabasePoolState {
    /// The underlying PostgreSQL connection pool
    pub pool: Option<sqlx::PgPool>,
    /// Configuration for the database connection
    pub config: Option<crate::config::DatabaseConfig>,
    /// Number of reconnection attempts since last successful connection
    pub reconnect_attempts: u32,
    /// Whether the agent is currently attempting to connect
    pub connecting: bool,
}

/// Agent-based PostgreSQL connection pool manager
///
/// This agent manages a database connection pool using message passing
/// instead of shared mutable state. Benefits include:
///
/// - **No lock contention**: Pool access via async messages
/// - **Automatic reconnection**: Built-in retry with exponential backoff
/// - **Health monitoring**: Query health status via messages
/// - **Graceful shutdown**: Pool closed on agent stop
#[cfg(feature = "database")]
pub struct DatabasePoolAgent;

#[cfg(feature = "database")]
impl DatabasePoolAgent {
    /// Spawn a new database pool agent with the given configuration
    ///
    /// The agent will immediately begin connecting to the database.
    /// Subscribe to [`PoolReady`] events to be notified when the pool is available.
    pub async fn spawn(
        runtime: &mut AgentRuntime,
        config: crate::config::DatabaseConfig,
    ) -> anyhow::Result<AgentHandle> {
        let mut agent = runtime.new_agent::<DatabasePoolState>();

        // Initialize state before starting
        agent.model.config = Some(config);
        agent.model.connecting = true;

        // Handle GetPool requests - respond with current pool state
        agent.act_on::<GetPool>(|agent, envelope| {
            let pool = agent.model.pool.clone();
            let connecting = agent.model.connecting;
            let reply_envelope = envelope.reply_envelope();

            AgentReply::from_async(async move {
                use super::messages::PoolResponse;
                let response = if let Some(p) = pool {
                    PoolResponse::Available(p)
                } else if connecting {
                    PoolResponse::Connecting
                } else {
                    PoolResponse::NotConnected
                };
                reply_envelope.send(response).await;
            })
        });

        // Handle health check requests
        agent.act_on::<PoolHealthCheck>(|agent, envelope| {
            let pool = agent.model.pool.clone();
            let config = agent.model.config.clone();
            let reply_envelope = envelope.reply_envelope();

            AgentReply::from_async(async move {
                let response = if let Some(ref p) = pool {
                    let size = p.size();
                    let idle = p.num_idle() as u32;
                    let max = config.map(|c| c.max_connections).unwrap_or(10);
                    let active = size.saturating_sub(idle);

                    PoolHealthResponse::healthy(format!(
                        "Connected: {}/{} active",
                        active, max
                    ))
                    .with_connections(active, idle)
                } else {
                    PoolHealthResponse::unhealthy("Not connected")
                };
                reply_envelope.send(response).await;
            })
        });

        // Handle pool connected message (sent from spawned task)
        agent.mutate_on::<DatabasePoolConnected>(|agent, envelope| {
            agent.model.pool = Some(envelope.message().pool.clone());
            agent.model.connecting = false;
            agent.model.reconnect_attempts = 0;
            tracing::info!("Database pool connected and stored in agent state");

            let broker = agent.broker().clone();
            AgentReply::from_async(async move {
                broker.broadcast(PoolReady::database()).await;
            })
        });

        // Handle pool connection failed message
        agent.mutate_on::<DatabasePoolConnectionFailed>(|agent, envelope| {
            agent.model.connecting = false;
            tracing::error!("Database pool connection failed: {}", envelope.message().error);
            AgentReply::immediate()
        });

        // Handle reconnection requests
        agent.mutate_on::<PoolReconnect>(|agent, _envelope| {
            let config = agent.model.config.clone();
            let current_attempts = agent.model.reconnect_attempts;

            if current_attempts >= MAX_RECONNECT_ATTEMPTS {
                tracing::warn!(
                    "Database pool reconnect: max attempts ({}) reached",
                    MAX_RECONNECT_ATTEMPTS
                );
                return AgentReply::immediate();
            }

            agent.model.connecting = true;
            agent.model.reconnect_attempts = current_attempts + 1;
            let self_handle = agent.handle().clone();

            AgentReply::from_async(async move {
                if let Some(cfg) = config {
                    tracing::info!(
                        "Database pool reconnecting (attempt {})",
                        current_attempts + 1
                    );

                    // Spawn the non-Sync connection work
                    let result = tokio::spawn(async move {
                        crate::database::create_pool(&cfg).await
                    })
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

        // Initialize connection on startup using spawn pattern
        agent.after_start(|agent| {
            let config = agent.model.config.clone();
            let self_handle = agent.handle().clone();

            AgentReply::from_async(async move {
                if let Some(cfg) = config {
                    tracing::info!("Database pool agent starting, connecting to database...");

                    // Spawn the non-Sync connection work
                    let result = tokio::spawn(async move {
                        crate::database::create_pool(&cfg).await
                    })
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

#[cfg(feature = "cache")]
use super::messages::{RedisPoolConnected, RedisPoolConnectionFailed};

/// State for the Redis pool agent
#[cfg(feature = "cache")]
#[derive(Debug, Default)]
pub struct RedisPoolState {
    /// The underlying Redis connection pool
    pub pool: Option<deadpool_redis::Pool>,
    /// Configuration for the Redis connection
    pub config: Option<crate::config::RedisConfig>,
    /// Number of reconnection attempts since last successful connection
    pub reconnect_attempts: u32,
    /// Whether the agent is currently attempting to connect
    pub connecting: bool,
}

/// Agent-based Redis connection pool manager
///
/// Similar to [`DatabasePoolAgent`], this agent manages a Redis connection
/// pool using message passing instead of shared mutable state.
#[cfg(feature = "cache")]
pub struct RedisPoolAgent;

#[cfg(feature = "cache")]
impl RedisPoolAgent {
    /// Spawn a new Redis pool agent with the given configuration
    pub async fn spawn(
        runtime: &mut AgentRuntime,
        config: crate::config::RedisConfig,
    ) -> anyhow::Result<AgentHandle> {
        let mut agent = runtime.new_agent::<RedisPoolState>();

        // Initialize state before starting
        agent.model.config = Some(config);
        agent.model.connecting = true;

        // Handle GetPool requests
        agent.act_on::<GetPool>(|agent, envelope| {
            let pool = agent.model.pool.clone();
            let connecting = agent.model.connecting;
            let reply_envelope = envelope.reply_envelope();

            AgentReply::from_async(async move {
                use super::messages::PoolResponse;
                let response = if let Some(p) = pool {
                    PoolResponse::Available(p)
                } else if connecting {
                    PoolResponse::Connecting
                } else {
                    PoolResponse::NotConnected
                };
                reply_envelope.send(response).await;
            })
        });

        // Handle health check requests
        agent.act_on::<PoolHealthCheck>(|agent, envelope| {
            let pool = agent.model.pool.clone();
            let config = agent.model.config.clone();
            let reply_envelope = envelope.reply_envelope();

            AgentReply::from_async(async move {
                let response = if let Some(ref p) = pool {
                    let status = p.status();
                    let max = config.map(|c| c.max_connections).unwrap_or(10);
                    let available = status.available;
                    let active = max.saturating_sub(available);

                    PoolHealthResponse::healthy(format!(
                        "Connected: {}/{} available",
                        available, max
                    ))
                    .with_connections(active as u32, available as u32)
                } else {
                    PoolHealthResponse::unhealthy("Not connected")
                };
                reply_envelope.send(response).await;
            })
        });

        // Handle pool connected message
        agent.mutate_on::<RedisPoolConnected>(|agent, envelope| {
            agent.model.pool = Some(envelope.message().pool.clone());
            agent.model.connecting = false;
            agent.model.reconnect_attempts = 0;
            tracing::info!("Redis pool connected and stored in agent state");

            let broker = agent.broker().clone();
            AgentReply::from_async(async move {
                broker.broadcast(PoolReady::redis()).await;
            })
        });

        // Handle pool connection failed message
        agent.mutate_on::<RedisPoolConnectionFailed>(|agent, envelope| {
            agent.model.connecting = false;
            tracing::error!("Redis pool connection failed: {}", envelope.message().error);
            AgentReply::immediate()
        });

        // Handle reconnection requests
        agent.mutate_on::<PoolReconnect>(|agent, _envelope| {
            let config = agent.model.config.clone();
            let current_attempts = agent.model.reconnect_attempts;

            if current_attempts >= MAX_RECONNECT_ATTEMPTS {
                tracing::warn!(
                    "Redis pool reconnect: max attempts ({}) reached",
                    MAX_RECONNECT_ATTEMPTS
                );
                return AgentReply::immediate();
            }

            agent.model.connecting = true;
            agent.model.reconnect_attempts = current_attempts + 1;
            let self_handle = agent.handle().clone();

            AgentReply::from_async(async move {
                if let Some(cfg) = config {
                    tracing::info!("Redis pool reconnecting (attempt {})", current_attempts + 1);

                    let result = tokio::spawn(async move {
                        crate::cache::create_pool(&cfg).await
                    })
                    .await;

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

        // Initialize connection on startup
        agent.after_start(|agent| {
            let config = agent.model.config.clone();
            let self_handle = agent.handle().clone();

            AgentReply::from_async(async move {
                if let Some(cfg) = config {
                    tracing::info!("Redis pool agent starting, connecting to Redis...");

                    let result = tokio::spawn(async move {
                        crate::cache::create_pool(&cfg).await
                    })
                    .await;

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

#[cfg(feature = "events")]
use super::messages::{NatsClientConnected, NatsClientConnectionFailed};

/// State for the NATS pool agent
#[cfg(feature = "events")]
#[derive(Debug, Default)]
pub struct NatsPoolState {
    /// The underlying NATS client
    pub client: Option<async_nats::Client>,
    /// Configuration for the NATS connection
    pub config: Option<crate::config::NatsConfig>,
    /// Number of reconnection attempts since last successful connection
    pub reconnect_attempts: u32,
    /// Whether the agent is currently attempting to connect
    pub connecting: bool,
}

/// Agent-based NATS client manager
///
/// Manages a NATS client connection using message passing.
#[cfg(feature = "events")]
pub struct NatsPoolAgent;

#[cfg(feature = "events")]
impl NatsPoolAgent {
    /// Spawn a new NATS pool agent with the given configuration
    pub async fn spawn(
        runtime: &mut AgentRuntime,
        config: crate::config::NatsConfig,
    ) -> anyhow::Result<AgentHandle> {
        let mut agent = runtime.new_agent::<NatsPoolState>();

        // Initialize state before starting
        agent.model.config = Some(config);
        agent.model.connecting = true;

        // Handle GetPool requests
        agent.act_on::<GetPool>(|agent, envelope| {
            let client = agent.model.client.clone();
            let connecting = agent.model.connecting;
            let reply_envelope = envelope.reply_envelope();

            AgentReply::from_async(async move {
                use super::messages::PoolResponse;
                let response = if let Some(c) = client {
                    PoolResponse::Available(c)
                } else if connecting {
                    PoolResponse::Connecting
                } else {
                    PoolResponse::NotConnected
                };
                reply_envelope.send(response).await;
            })
        });

        // Handle health check requests
        agent.act_on::<PoolHealthCheck>(|agent, envelope| {
            let client = agent.model.client.clone();
            let reply_envelope = envelope.reply_envelope();

            AgentReply::from_async(async move {
                let response = if let Some(ref c) = client {
                    let state = c.connection_state();
                    match state {
                        async_nats::connection::State::Connected => {
                            PoolHealthResponse::healthy("Connected")
                        }
                        async_nats::connection::State::Disconnected => {
                            PoolHealthResponse::unhealthy("Disconnected")
                        }
                        async_nats::connection::State::Pending => {
                            PoolHealthResponse::unhealthy("Pending connection")
                        }
                    }
                } else {
                    PoolHealthResponse::unhealthy("Not connected")
                };
                reply_envelope.send(response).await;
            })
        });

        // Handle client connected message
        agent.mutate_on::<NatsClientConnected>(|agent, envelope| {
            agent.model.client = Some(envelope.message().client.clone());
            agent.model.connecting = false;
            agent.model.reconnect_attempts = 0;
            tracing::info!("NATS client connected and stored in agent state");

            let broker = agent.broker().clone();
            AgentReply::from_async(async move {
                broker.broadcast(PoolReady::nats()).await;
            })
        });

        // Handle client connection failed message
        agent.mutate_on::<NatsClientConnectionFailed>(|agent, envelope| {
            agent.model.connecting = false;
            tracing::error!("NATS client connection failed: {}", envelope.message().error);
            AgentReply::immediate()
        });

        // Handle reconnection requests
        agent.mutate_on::<PoolReconnect>(|agent, _envelope| {
            let config = agent.model.config.clone();
            let current_attempts = agent.model.reconnect_attempts;

            if current_attempts >= MAX_RECONNECT_ATTEMPTS {
                tracing::warn!(
                    "NATS client reconnect: max attempts ({}) reached",
                    MAX_RECONNECT_ATTEMPTS
                );
                return AgentReply::immediate();
            }

            agent.model.connecting = true;
            agent.model.reconnect_attempts = current_attempts + 1;
            let self_handle = agent.handle().clone();

            AgentReply::from_async(async move {
                if let Some(cfg) = config {
                    tracing::info!("NATS client reconnecting (attempt {})", current_attempts + 1);

                    let result = tokio::spawn(async move {
                        crate::events::create_client(&cfg).await
                    })
                    .await;

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

        // Initialize connection on startup
        agent.after_start(|agent| {
            let config = agent.model.config.clone();
            let self_handle = agent.handle().clone();

            AgentReply::from_async(async move {
                if let Some(cfg) = config {
                    tracing::info!("NATS pool agent starting, connecting to NATS...");

                    let result = tokio::spawn(async move {
                        crate::events::create_client(&cfg).await
                    })
                    .await;

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
