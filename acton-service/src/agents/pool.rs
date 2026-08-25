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
//!
//! ## Asking about connection state
//!
//! Every pool agent answers two requests:
//!
//! - [`GetPoolHealth`](super::messages::GetPoolHealth) samples the current
//!   state, which is `Connecting` while the first attempt is still in flight.
//! - [`WaitForPoolReady`](super::messages::WaitForPoolReady) parks until that
//!   attempt settles, then answers with the result.
//!
//! The distinction matters because of the spawn pattern above: the connection
//! result is produced on a spawned task, so it is not yet in the mailbox when
//! `spawn` returns. A sampling ask right after startup therefore races the
//! connection and reports `Connecting`; only `WaitForPoolReady` is a barrier.

// ============================================================================
// Shared health reporting
// ============================================================================

#[cfg(any(
    feature = "database",
    feature = "cache",
    feature = "events",
    feature = "turso",
    feature = "surrealdb",
    feature = "clickhouse",
    feature = "mssql"
))]
use super::messages::{GetPoolHealth, WaitForPoolReady};

/// Build the answer to a [`GetPoolHealth`](super::messages::GetPoolHealth) ask.
///
/// Kept as a free function so all six pool agents describe themselves the same
/// way, and so the mapping from agent state to health is testable without
/// standing up an actor.
#[cfg(any(
    feature = "database",
    feature = "cache",
    feature = "events",
    feature = "turso",
    feature = "surrealdb",
    feature = "clickhouse",
    feature = "mssql"
))]
fn pool_health(
    name: &str,
    connected: bool,
    connecting: bool,
    last_error: Option<&str>,
) -> super::messages::ComponentHealth {
    use super::messages::{ComponentHealth, HealthStatus};

    let (status, message) = if connected {
        (HealthStatus::Healthy, "connected".to_string())
    } else if connecting {
        (HealthStatus::Connecting, "connecting".to_string())
    } else {
        (
            HealthStatus::Unhealthy,
            last_error.unwrap_or("not connected").to_string(),
        )
    };

    ComponentHealth {
        name: name.to_string(),
        status,
        message,
    }
}

// ============================================================================
// Microsoft SQL Server Pool Agent
// ============================================================================

#[cfg(feature = "mssql")]
use super::messages::{MssqlPoolConnected, MssqlPoolConnectionFailed};
#[cfg(all(feature = "mssql", not(feature = "database")))]
use acton_reactive::prelude::*;
#[cfg(all(feature = "mssql", not(feature = "database")))]
use std::sync::Arc;
#[cfg(all(feature = "mssql", not(feature = "database")))]
use tokio::sync::RwLock;
#[cfg(all(feature = "mssql", not(feature = "database")))]
use tokio_util::sync::CancellationToken;

#[cfg(feature = "mssql")]
pub type SharedMssqlPool = Arc<RwLock<Option<crate::mssql::MssqlPool>>>;

#[cfg(feature = "mssql")]
#[derive(Debug, Default)]
pub struct MssqlPoolState {
    pub pool: Option<crate::mssql::MssqlPool>,
    pub config: Option<crate::config::DatabaseConfig>,
    pub connecting: bool,
    pub shared_pool: Option<SharedMssqlPool>,
    pub last_error: Option<String>,
    pub(crate) ready_waiters: Vec<OutboundEnvelope>,
    pub cancel_token: Option<CancellationToken>,
}

/// Agent-managed Microsoft SQL Server pool lifecycle.
#[cfg(feature = "mssql")]
pub struct MssqlPoolAgent;

#[cfg(feature = "mssql")]
impl MssqlPoolAgent {
    pub async fn spawn(
        runtime: &mut ActorRuntime,
        config: crate::config::DatabaseConfig,
        shared_pool: Option<SharedMssqlPool>,
    ) -> anyhow::Result<ActorHandle> {
        let mut agent = runtime.new_actor::<MssqlPoolState>();
        agent.model.config = Some(config);
        agent.model.connecting = true;
        agent.model.shared_pool = shared_pool;
        agent.model.cancel_token = Some(CancellationToken::new());
        agent.mutate_on::<MssqlPoolConnected>(|agent, envelope| {
            let pool = envelope.message().pool.clone();
            agent.model.pool = Some(pool.clone());
            agent.model.connecting = false;
            agent.model.last_error = None;
            let shared = agent.model.shared_pool.clone();
            let waiters = std::mem::take(&mut agent.model.ready_waiters);
            let health = pool_health("mssql", true, false, None);
            Reply::pending(async move {
                if let Some(shared) = shared {
                    *shared.write().await = Some(pool);
                }
                for waiter in waiters {
                    waiter.send(health.clone()).await;
                }
            })
        });
        agent.mutate_on::<MssqlPoolConnectionFailed>(|agent, envelope| {
            agent.model.connecting = false;
            agent.model.last_error = Some(envelope.message().error.clone());
            let waiters = std::mem::take(&mut agent.model.ready_waiters);
            let health = pool_health("mssql", false, false, agent.model.last_error.as_deref());
            Reply::pending(async move {
                for waiter in waiters {
                    waiter.send(health.clone()).await;
                }
            })
        });
        agent.mutate_on::<WaitForPoolReady>(|agent, envelope| {
            let reply = envelope.reply_envelope();
            if agent.model.connecting {
                agent.model.ready_waiters.push(reply);
                return Reply::ready();
            }
            let health = pool_health(
                "mssql",
                agent.model.pool.is_some(),
                false,
                agent.model.last_error.as_deref(),
            );
            Reply::pending(async move {
                reply.send(health).await;
            })
        });
        agent.act_on::<GetPoolHealth>(|agent, envelope| {
            let reply = envelope.reply_envelope();
            let health = pool_health(
                "mssql",
                agent.model.pool.is_some(),
                agent.model.connecting,
                agent.model.last_error.as_deref(),
            );
            Reply::pending(async move {
                reply.send(health).await;
            })
        });
        agent.after_start(|agent| {
            let config = agent.model.config.clone();
            let token = agent.model.cancel_token.clone();
            let handle = agent.handle().clone();
            if let Some(config) = config { tokio::spawn(async move {
                tokio::select! {
                    () = async { if let Some(token) = token { token.cancelled().await } } => handle.send(MssqlPoolConnectionFailed { error: "connection cancelled during shutdown".to_string() }).await,
                    result = crate::mssql::create_pool(&config) => match result {
                        Ok(pool) => handle.send(MssqlPoolConnected { pool }).await,
                        Err(error) => handle.send(MssqlPoolConnectionFailed { error: error.to_string() }).await,
                    }
                }
            }); }
            Reply::ready()
        });
        agent.before_stop(|agent| {
            if let Some(token) = &agent.model.cancel_token {
                token.cancel();
            }
            Reply::ready()
        });
        Ok(agent.start().await)
    }
}

// ============================================================================
// Database Pool Agent
// ============================================================================

#[cfg(feature = "database")]
use super::messages::{DatabasePoolConnected, DatabasePoolConnectionFailed};
#[cfg(feature = "database")]
use acton_reactive::prelude::*;
#[cfg(feature = "database")]
use std::sync::Arc;
#[cfg(feature = "database")]
use tokio::sync::RwLock;
#[cfg(feature = "database")]
use tokio_util::sync::CancellationToken;

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
    /// Reason the last connection attempt failed, reported via `GetPoolHealth`
    pub last_error: Option<String>,
    /// Callers parked in `WaitForPoolReady` until the first attempt settles
    pub(crate) ready_waiters: Vec<OutboundEnvelope>,
    /// Cancellation token for graceful shutdown during connection retries
    pub cancel_token: Option<CancellationToken>,
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
        runtime: &mut ActorRuntime,
        config: crate::config::DatabaseConfig,
        shared_pool: Option<SharedDbPool>,
    ) -> anyhow::Result<ActorHandle> {
        let mut agent = runtime.new_actor::<DatabasePoolState>();

        // Create cancellation token for graceful shutdown during connection retries
        let cancel_token = CancellationToken::new();

        // Initialize state before starting
        agent.model.config = Some(config);
        agent.model.connecting = true;
        agent.model.shared_pool = shared_pool;
        agent.model.cancel_token = Some(cancel_token);

        // Handle pool connected message (sent from spawned task)
        agent.mutate_on::<DatabasePoolConnected>(|agent, envelope| {
            let pool = envelope.message().pool.clone();
            agent.model.pool = Some(pool.clone());
            agent.model.connecting = false;
            agent.model.last_error = None;
            // Everyone parked in WaitForPoolReady is released here, in
            // the same handler that records the success.
            let health = pool_health("database", true, false, None);
            let waiters = std::mem::take(&mut agent.model.ready_waiters);

            // Update shared storage if configured
            let shared_pool = agent.model.shared_pool.clone();

            Reply::pending(async move {
                // Update shared storage for direct AppState access
                if let Some(shared) = shared_pool {
                    *shared.write().await = Some(pool);
                    tracing::info!("Database pool connected and stored in shared state");
                } else {
                    tracing::info!("Database pool connected (no shared state)");
                }

                for waiter in waiters {
                    waiter.send(health.clone()).await;
                }
            })
        });

        // Handle pool connection failed message
        agent.mutate_on::<DatabasePoolConnectionFailed>(|agent, envelope| {
            let error_msg = envelope.message().error.clone();
            agent.model.connecting = false;
            tracing::error!("Database pool connection failed: {}", error_msg);
            // Kept rather than only logged, so `GetPoolHealth` can say
            // why the pool is down instead of merely that it is.
            agent.model.last_error = Some(error_msg);

            let health = pool_health("database", false, false, agent.model.last_error.as_deref());
            let waiters = std::mem::take(&mut agent.model.ready_waiters);

            Reply::pending(async move {
                for waiter in waiters {
                    waiter.send(health.clone()).await;
                }
            })
        });

        // Park a caller until the first connection attempt settles. `mutate_on`
        // because parking writes the model, and because the answer must not
        // overtake the Connected/Failed message that produces it.
        agent.mutate_on::<WaitForPoolReady>(|agent, envelope| {
            let reply = envelope.reply_envelope();

            if agent.model.connecting {
                agent.model.ready_waiters.push(reply);
                return Reply::ready();
            }

            let health = pool_health(
                "database",
                agent.model.pool.is_some(),
                false,
                agent.model.last_error.as_deref(),
            );
            Reply::pending(async move {
                reply.send(health).await;
            })
        });

        // Answer health asks. Read-only, so `act_on`.
        agent.act_on::<GetPoolHealth>(|agent, envelope| {
            let reply = envelope.reply_envelope();
            let health = pool_health(
                "database",
                agent.model.pool.is_some(),
                agent.model.connecting,
                agent.model.last_error.as_deref(),
            );

            Reply::pending(async move {
                reply.send(health).await;
            })
        });

        // Initialize connection on startup using spawn pattern
        // NOTE: We spawn the task but DON'T await it here. The task sends messages
        // back to the agent when it completes. This allows before_stop to run
        // immediately when shutdown is requested, which cancels the token and
        // causes the spawned task to exit via tokio::select!.
        agent.after_start(|agent| {
            let config = agent.model.config.clone();
            let cancel_token = agent.model.cancel_token.clone();
            let self_handle = agent.handle().clone();

            if let Some(cfg) = config {
                tracing::info!("Database pool agent starting, connecting to database...");

                // Spawn the connection work - it will send a message back when done
                tokio::spawn(async move {
                    // Race connection against cancellation
                    tokio::select! {
                        biased;

                        // Cancellation branch - triggered by before_stop
                        () = async {
                            if let Some(ref token) = cancel_token {
                                token.cancelled().await;
                            } else {
                                // No token, never cancel via this branch
                                std::future::pending::<()>().await;
                            }
                        } => {
                            tracing::info!("Database connection cancelled during shutdown");
                            self_handle
                                .send(DatabasePoolConnectionFailed {
                                    error: "Connection cancelled during shutdown".to_string(),
                                })
                                .await;
                        }

                        // Connection branch
                        result = crate::database::create_pool(&cfg) => {
                            match result {
                                Ok(pool) => {
                                    self_handle.send(DatabasePoolConnected { pool }).await;
                                }
                                Err(e) => {
                                    self_handle
                                        .send(DatabasePoolConnectionFailed {
                                            error: e.to_string(),
                                        })
                                        .await;
                                }
                            }
                        }
                    }
                });
            }

            Reply::ready()
        });

        // Graceful cleanup on shutdown
        agent.before_stop(|agent| {
            let pool = agent.model.pool.clone();
            let cancel_token = agent.model.cancel_token.clone();
            Reply::pending(async move {
                // Cancel any ongoing connection retries first
                if let Some(token) = cancel_token {
                    token.cancel();
                    tracing::debug!("Database connection retry cancelled");
                }

                if let Some(p) = pool {
                    tracing::info!("Database pool agent stopping, closing connections...");
                    // sqlx 0.8's `Pool::close()` returns a `Send` but not `Sync`
                    // future. `before_stop` requires the awaited future to be
                    // `Send + Sync`, so we offload the non-Sync work onto a
                    // tokio task whose `JoinHandle` is itself `Send + Sync`.
                    let _ = tokio::spawn(async move { p.close().await }).await;
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
#[cfg(all(feature = "cache", not(feature = "database")))]
use acton_reactive::prelude::*;
#[cfg(all(feature = "cache", not(feature = "database")))]
use std::sync::Arc;
#[cfg(all(feature = "cache", not(feature = "database")))]
use tokio::sync::RwLock;

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
    /// Reason the last connection attempt failed, reported via `GetPoolHealth`
    pub last_error: Option<String>,
    /// Callers parked in `WaitForPoolReady` until the first attempt settles
    pub(crate) ready_waiters: Vec<OutboundEnvelope>,
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
        runtime: &mut ActorRuntime,
        config: crate::config::RedisConfig,
        shared_pool: Option<SharedRedisPool>,
    ) -> anyhow::Result<ActorHandle> {
        let mut agent = runtime.new_actor::<RedisPoolState>();

        // Initialize state before starting
        agent.model.config = Some(config);
        agent.model.connecting = true;
        agent.model.shared_pool = shared_pool;

        // Handle pool connected message
        agent.mutate_on::<RedisPoolConnected>(|agent, envelope| {
            let pool = envelope.message().pool.clone();
            agent.model.pool = Some(pool.clone());
            agent.model.connecting = false;
            agent.model.last_error = None;
            // Everyone parked in WaitForPoolReady is released here, in
            // the same handler that records the success.
            let health = pool_health("cache", true, false, None);
            let waiters = std::mem::take(&mut agent.model.ready_waiters);

            // Update shared storage if configured
            let shared_pool = agent.model.shared_pool.clone();

            Reply::pending(async move {
                // Update shared storage for direct AppState access
                if let Some(shared) = shared_pool {
                    *shared.write().await = Some(pool);
                    tracing::info!("Redis pool connected and stored in shared state");
                } else {
                    tracing::info!("Redis pool connected (no shared state)");
                }

                for waiter in waiters {
                    waiter.send(health.clone()).await;
                }
            })
        });

        // Handle pool connection failed message
        agent.mutate_on::<RedisPoolConnectionFailed>(|agent, envelope| {
            let error_msg = envelope.message().error.clone();
            agent.model.connecting = false;
            tracing::error!("Redis pool connection failed: {}", error_msg);
            // Kept rather than only logged, so `GetPoolHealth` can say
            // why the pool is down instead of merely that it is.
            agent.model.last_error = Some(error_msg);

            let health = pool_health("cache", false, false, agent.model.last_error.as_deref());
            let waiters = std::mem::take(&mut agent.model.ready_waiters);

            Reply::pending(async move {
                for waiter in waiters {
                    waiter.send(health.clone()).await;
                }
            })
        });

        // Park a caller until the first connection attempt settles. `mutate_on`
        // because parking writes the model, and because the answer must not
        // overtake the Connected/Failed message that produces it.
        agent.mutate_on::<WaitForPoolReady>(|agent, envelope| {
            let reply = envelope.reply_envelope();

            if agent.model.connecting {
                agent.model.ready_waiters.push(reply);
                return Reply::ready();
            }

            let health = pool_health(
                "cache",
                agent.model.pool.is_some(),
                false,
                agent.model.last_error.as_deref(),
            );
            Reply::pending(async move {
                reply.send(health).await;
            })
        });

        // Answer health asks. Read-only, so `act_on`.
        agent.act_on::<GetPoolHealth>(|agent, envelope| {
            let reply = envelope.reply_envelope();
            let health = pool_health(
                "cache",
                agent.model.pool.is_some(),
                agent.model.connecting,
                agent.model.last_error.as_deref(),
            );

            Reply::pending(async move {
                reply.send(health).await;
            })
        });

        // Initialize connection on startup
        agent.after_start(|agent| {
            let config = agent.model.config.clone();
            let self_handle = agent.handle().clone();

            Reply::pending(async move {
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
            Reply::pending(async move {
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
#[cfg(all(feature = "events", not(feature = "database"), not(feature = "cache")))]
use acton_reactive::prelude::*;
#[cfg(all(feature = "events", not(feature = "database"), not(feature = "cache")))]
use std::sync::Arc;
#[cfg(all(feature = "events", not(feature = "database"), not(feature = "cache")))]
use tokio::sync::RwLock;

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
    /// Reason the last connection attempt failed, reported via `GetPoolHealth`
    pub last_error: Option<String>,
    /// Callers parked in `WaitForPoolReady` until the first attempt settles
    pub(crate) ready_waiters: Vec<OutboundEnvelope>,
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
        runtime: &mut ActorRuntime,
        config: crate::config::NatsConfig,
        shared_client: Option<SharedNatsClient>,
    ) -> anyhow::Result<ActorHandle> {
        let mut agent = runtime.new_actor::<NatsPoolState>();

        // Initialize state before starting
        agent.model.config = Some(config);
        agent.model.connecting = true;
        agent.model.shared_client = shared_client;

        // Handle client connected message
        agent.mutate_on::<NatsClientConnected>(|agent, envelope| {
            let client = envelope.message().client.clone();
            agent.model.client = Some(client.clone());
            agent.model.connecting = false;
            agent.model.last_error = None;
            // Everyone parked in WaitForPoolReady is released here, in
            // the same handler that records the success.
            let health = pool_health("events", true, false, None);
            let waiters = std::mem::take(&mut agent.model.ready_waiters);

            // Update shared storage if configured
            let shared_client = agent.model.shared_client.clone();

            Reply::pending(async move {
                // Update shared storage for direct AppState access
                if let Some(shared) = shared_client {
                    *shared.write().await = Some(client);
                    tracing::info!("NATS client connected and stored in shared state");
                } else {
                    tracing::info!("NATS client connected (no shared state)");
                }

                for waiter in waiters {
                    waiter.send(health.clone()).await;
                }
            })
        });

        // Handle client connection failed message
        agent.mutate_on::<NatsClientConnectionFailed>(|agent, envelope| {
            let error_msg = envelope.message().error.clone();
            agent.model.connecting = false;
            tracing::error!("NATS client connection failed: {}", error_msg);
            // Kept rather than only logged, so `GetPoolHealth` can say
            // why the pool is down instead of merely that it is.
            agent.model.last_error = Some(error_msg);

            let health = pool_health("events", false, false, agent.model.last_error.as_deref());
            let waiters = std::mem::take(&mut agent.model.ready_waiters);

            Reply::pending(async move {
                for waiter in waiters {
                    waiter.send(health.clone()).await;
                }
            })
        });

        // Park a caller until the first connection attempt settles. `mutate_on`
        // because parking writes the model, and because the answer must not
        // overtake the Connected/Failed message that produces it.
        agent.mutate_on::<WaitForPoolReady>(|agent, envelope| {
            let reply = envelope.reply_envelope();

            if agent.model.connecting {
                agent.model.ready_waiters.push(reply);
                return Reply::ready();
            }

            let health = pool_health(
                "events",
                agent.model.client.is_some(),
                false,
                agent.model.last_error.as_deref(),
            );
            Reply::pending(async move {
                reply.send(health).await;
            })
        });

        // Answer health asks. Read-only, so `act_on`.
        agent.act_on::<GetPoolHealth>(|agent, envelope| {
            let reply = envelope.reply_envelope();
            let health = pool_health(
                "events",
                agent.model.client.is_some(),
                agent.model.connecting,
                agent.model.last_error.as_deref(),
            );

            Reply::pending(async move {
                reply.send(health).await;
            })
        });

        // Initialize connection on startup
        agent.after_start(|agent| {
            let config = agent.model.config.clone();
            let self_handle = agent.handle().clone();

            Reply::pending(async move {
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
            Reply::pending(async move {
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

// ============================================================================
// Turso Database Agent
// ============================================================================

#[cfg(feature = "turso")]
use super::messages::{TursoDbConnected, TursoDbConnectionFailed};
#[cfg(all(
    feature = "turso",
    not(feature = "database"),
    not(feature = "events"),
    not(feature = "cache")
))]
use acton_reactive::prelude::*;
#[cfg(all(
    feature = "turso",
    not(feature = "database"),
    not(feature = "cache"),
    not(feature = "events")
))]
use std::sync::Arc;
#[cfg(all(
    feature = "turso",
    not(feature = "database"),
    not(feature = "cache"),
    not(feature = "events")
))]
use tokio::sync::RwLock;
#[cfg(all(feature = "turso", not(feature = "database")))]
use tokio_util::sync::CancellationToken;

/// Shared database storage type for Turso/libsql connections
#[cfg(feature = "turso")]
pub type SharedTursoDb = Arc<RwLock<Option<Arc<libsql::Database>>>>;

/// State for the Turso database agent
#[cfg(feature = "turso")]
#[derive(Debug, Default)]
pub struct TursoDbState {
    /// The underlying libsql database (wrapped in Arc since Database doesn't implement Clone)
    pub db: Option<Arc<libsql::Database>>,
    /// Configuration for the Turso connection
    pub config: Option<crate::config::TursoConfig>,
    /// Whether the agent is currently attempting to connect
    pub connecting: bool,
    /// Shared storage that AppState reads from directly
    pub shared_db: Option<SharedTursoDb>,
    /// Reason the last connection attempt failed, reported via `GetPoolHealth`
    pub last_error: Option<String>,
    /// Callers parked in `WaitForPoolReady` until the first attempt settles
    pub(crate) ready_waiters: Vec<OutboundEnvelope>,
    /// Cancellation token for graceful shutdown during connection retries
    pub cancel_token: Option<CancellationToken>,
}

/// Agent-based Turso/libsql database manager
///
/// This agent manages a Turso database connection using message passing
/// instead of shared mutable state. Benefits include:
///
/// - **No lock contention**: Database access via shared state with minimal locking
/// - **Automatic connection**: Connection established on agent start
/// - **Graceful shutdown**: Database closed on agent stop
#[cfg(feature = "turso")]
pub struct TursoDbAgent;

#[cfg(feature = "turso")]
impl TursoDbAgent {
    /// Spawn a new Turso database agent with the given configuration
    ///
    /// The agent will immediately begin connecting to the database.
    ///
    /// # Arguments
    ///
    /// * `runtime` - The agent runtime to spawn into
    /// * `config` - Turso connection configuration
    /// * `shared_db` - Shared storage that will be updated when the database connects.
    ///   `AppState::turso()` reads the database directly from this storage.
    pub async fn spawn(
        runtime: &mut ActorRuntime,
        config: crate::config::TursoConfig,
        shared_db: Option<SharedTursoDb>,
    ) -> anyhow::Result<ActorHandle> {
        let mut agent = runtime.new_actor::<TursoDbState>();

        // Create cancellation token for graceful shutdown during connection retries
        let cancel_token = CancellationToken::new();

        // Initialize state before starting
        agent.model.config = Some(config);
        agent.model.connecting = true;
        agent.model.shared_db = shared_db;
        agent.model.cancel_token = Some(cancel_token);

        // Handle database connected message (sent from spawned task)
        agent.mutate_on::<TursoDbConnected>(|agent, envelope| {
            let db = envelope.message().db.clone();
            agent.model.db = Some(db.clone());
            agent.model.connecting = false;
            agent.model.last_error = None;
            // Everyone parked in WaitForPoolReady is released here, in
            // the same handler that records the success.
            let health = pool_health("turso", true, false, None);
            let waiters = std::mem::take(&mut agent.model.ready_waiters);

            // Update shared storage if configured
            let shared_db = agent.model.shared_db.clone();

            Reply::pending(async move {
                // Update shared storage for direct AppState access
                if let Some(shared) = shared_db {
                    *shared.write().await = Some(db.clone());
                    tracing::info!("Turso database connected and stored in shared state");
                } else {
                    tracing::info!("Turso database connected (no shared state)");
                }

                for waiter in waiters {
                    waiter.send(health.clone()).await;
                }
            })
        });

        // Handle database connection failed message
        agent.mutate_on::<TursoDbConnectionFailed>(|agent, envelope| {
            let error_msg = envelope.message().error.clone();
            agent.model.connecting = false;
            tracing::error!("Turso database connection failed: {}", error_msg);
            // Kept rather than only logged, so `GetPoolHealth` can say
            // why the pool is down instead of merely that it is.
            agent.model.last_error = Some(error_msg);

            let health = pool_health("turso", false, false, agent.model.last_error.as_deref());
            let waiters = std::mem::take(&mut agent.model.ready_waiters);

            Reply::pending(async move {
                for waiter in waiters {
                    waiter.send(health.clone()).await;
                }
            })
        });

        // Park a caller until the first connection attempt settles. `mutate_on`
        // because parking writes the model, and because the answer must not
        // overtake the Connected/Failed message that produces it.
        agent.mutate_on::<WaitForPoolReady>(|agent, envelope| {
            let reply = envelope.reply_envelope();

            if agent.model.connecting {
                agent.model.ready_waiters.push(reply);
                return Reply::ready();
            }

            let health = pool_health(
                "turso",
                agent.model.db.is_some(),
                false,
                agent.model.last_error.as_deref(),
            );
            Reply::pending(async move {
                reply.send(health).await;
            })
        });

        // Answer health asks. Read-only, so `act_on`.
        agent.act_on::<GetPoolHealth>(|agent, envelope| {
            let reply = envelope.reply_envelope();
            let health = pool_health(
                "turso",
                agent.model.db.is_some(),
                agent.model.connecting,
                agent.model.last_error.as_deref(),
            );

            Reply::pending(async move {
                reply.send(health).await;
            })
        });

        // Initialize connection on startup using spawn pattern
        // NOTE: We spawn the task but DON'T await it here. The task sends messages
        // back to the agent when it completes. This allows before_stop to run
        // immediately when shutdown is requested, which cancels the token and
        // causes the spawned task to exit via tokio::select!.
        agent.after_start(|agent| {
            let config = agent.model.config.clone();
            let cancel_token = agent.model.cancel_token.clone();
            let self_handle = agent.handle().clone();

            if let Some(cfg) = config {
                tracing::info!(
                    "Turso database agent starting, connecting to database (mode={:?})...",
                    cfg.mode
                );

                // Spawn the connection work - it will send a message back when done
                tokio::spawn(async move {
                    // Race connection against cancellation
                    tokio::select! {
                        biased;

                        // Cancellation branch - triggered by before_stop
                        () = async {
                            if let Some(ref token) = cancel_token {
                                token.cancelled().await;
                            } else {
                                // No token, never cancel via this branch
                                std::future::pending::<()>().await;
                            }
                        } => {
                            tracing::info!("Turso connection cancelled during shutdown");
                            self_handle
                                .send(TursoDbConnectionFailed {
                                    error: "Connection cancelled during shutdown".to_string(),
                                })
                                .await;
                        }

                        // Connection branch
                        result = crate::turso::create_database(&cfg) => {
                            match result {
                                Ok(db) => {
                                    self_handle.send(TursoDbConnected { db: Arc::new(db) }).await;
                                }
                                Err(e) => {
                                    self_handle
                                        .send(TursoDbConnectionFailed {
                                            error: e.to_string(),
                                        })
                                        .await;
                                }
                            }
                        }
                    }
                });
            }

            Reply::ready()
        });

        // Graceful cleanup on shutdown
        agent.before_stop(|agent| {
            let cancel_token = agent.model.cancel_token.clone();
            Reply::pending(async move {
                // Cancel any ongoing connection retries first
                if let Some(token) = cancel_token {
                    token.cancel();
                    tracing::debug!("Turso connection retry cancelled");
                }

                // Note: libsql::Database doesn't have an explicit close method,
                // dropping it will close the connection
                tracing::info!("Turso database agent stopping");
            })
        });

        let handle = agent.start().await;
        Ok(handle)
    }
}

// ============================================================================
// SurrealDB Agent
// ============================================================================

#[cfg(feature = "surrealdb")]
use super::messages::{SurrealDbConnected, SurrealDbConnectionFailed};
#[cfg(all(
    feature = "surrealdb",
    not(feature = "database"),
    not(feature = "events"),
    not(feature = "cache"),
    not(feature = "turso")
))]
use acton_reactive::prelude::*;
#[cfg(all(
    feature = "surrealdb",
    not(feature = "database"),
    not(feature = "cache"),
    not(feature = "events"),
    not(feature = "turso")
))]
use std::sync::Arc;
#[cfg(all(
    feature = "surrealdb",
    not(feature = "database"),
    not(feature = "cache"),
    not(feature = "events"),
    not(feature = "turso")
))]
use tokio::sync::RwLock;
#[cfg(all(
    feature = "surrealdb",
    not(feature = "database"),
    not(feature = "turso")
))]
use tokio_util::sync::CancellationToken;

/// Shared client storage type for SurrealDB connections
#[cfg(feature = "surrealdb")]
pub type SharedSurrealDb = Arc<RwLock<Option<Arc<crate::surrealdb_backend::SurrealClient>>>>;

/// State for the SurrealDB agent
#[cfg(feature = "surrealdb")]
#[derive(Debug, Default)]
pub struct SurrealDbState {
    /// The underlying SurrealDB client (wrapped in Arc since Surreal doesn't implement Clone)
    pub client: Option<Arc<crate::surrealdb_backend::SurrealClient>>,
    /// Configuration for the SurrealDB connection
    pub config: Option<crate::config::SurrealDbConfig>,
    /// Whether the agent is currently attempting to connect
    pub connecting: bool,
    /// Shared storage that AppState reads from directly
    pub shared_client: Option<SharedSurrealDb>,
    /// Reason the last connection attempt failed, reported via `GetPoolHealth`
    pub last_error: Option<String>,
    /// Callers parked in `WaitForPoolReady` until the first attempt settles
    pub(crate) ready_waiters: Vec<OutboundEnvelope>,
    /// Cancellation token for graceful shutdown during connection retries
    pub cancel_token: Option<CancellationToken>,
}

/// Agent-based SurrealDB client manager
///
/// This agent manages a SurrealDB connection using message passing
/// instead of shared mutable state. Benefits include:
///
/// - **No lock contention**: Client access via shared state with minimal locking
/// - **Automatic connection**: Connection established on agent start
/// - **Graceful shutdown**: Client closed on agent stop
#[cfg(feature = "surrealdb")]
pub struct SurrealDbAgent;

#[cfg(feature = "surrealdb")]
impl SurrealDbAgent {
    /// Spawn a new SurrealDB agent with the given configuration
    ///
    /// The agent will immediately begin connecting to the database.
    ///
    /// # Arguments
    ///
    /// * `runtime` - The agent runtime to spawn into
    /// * `config` - SurrealDB connection configuration
    /// * `shared_client` - Shared storage that will be updated when the client connects.
    ///   `AppState::surrealdb()` reads the client directly from this storage.
    pub async fn spawn(
        runtime: &mut ActorRuntime,
        config: crate::config::SurrealDbConfig,
        shared_client: Option<SharedSurrealDb>,
    ) -> anyhow::Result<ActorHandle> {
        let mut agent = runtime.new_actor::<SurrealDbState>();

        // Create cancellation token for graceful shutdown during connection retries
        let cancel_token = CancellationToken::new();

        // Initialize state before starting
        agent.model.config = Some(config);
        agent.model.connecting = true;
        agent.model.shared_client = shared_client;
        agent.model.cancel_token = Some(cancel_token);

        // Handle client connected message (sent from spawned task)
        agent.mutate_on::<SurrealDbConnected>(|agent, envelope| {
            let client = envelope.message().client.clone();
            agent.model.client = Some(client.clone());
            agent.model.connecting = false;
            agent.model.last_error = None;
            // Everyone parked in WaitForPoolReady is released here, in
            // the same handler that records the success.
            let health = pool_health("surrealdb", true, false, None);
            let waiters = std::mem::take(&mut agent.model.ready_waiters);

            // Update shared storage if configured
            let shared_client = agent.model.shared_client.clone();

            Reply::pending(async move {
                // Update shared storage for direct AppState access
                if let Some(shared) = shared_client {
                    *shared.write().await = Some(client);
                    tracing::info!("SurrealDB client connected and stored in shared state");
                } else {
                    tracing::info!("SurrealDB client connected (no shared state)");
                }

                for waiter in waiters {
                    waiter.send(health.clone()).await;
                }
            })
        });

        // Handle client connection failed message
        agent.mutate_on::<SurrealDbConnectionFailed>(|agent, envelope| {
            let error_msg = envelope.message().error.clone();
            agent.model.connecting = false;
            tracing::error!("SurrealDB client connection failed: {}", error_msg);
            // Kept rather than only logged, so `GetPoolHealth` can say
            // why the pool is down instead of merely that it is.
            agent.model.last_error = Some(error_msg);

            let health = pool_health("surrealdb", false, false, agent.model.last_error.as_deref());
            let waiters = std::mem::take(&mut agent.model.ready_waiters);

            Reply::pending(async move {
                for waiter in waiters {
                    waiter.send(health.clone()).await;
                }
            })
        });

        // Park a caller until the first connection attempt settles. `mutate_on`
        // because parking writes the model, and because the answer must not
        // overtake the Connected/Failed message that produces it.
        agent.mutate_on::<WaitForPoolReady>(|agent, envelope| {
            let reply = envelope.reply_envelope();

            if agent.model.connecting {
                agent.model.ready_waiters.push(reply);
                return Reply::ready();
            }

            let health = pool_health(
                "surrealdb",
                agent.model.client.is_some(),
                false,
                agent.model.last_error.as_deref(),
            );
            Reply::pending(async move {
                reply.send(health).await;
            })
        });

        // Answer health asks. Read-only, so `act_on`.
        agent.act_on::<GetPoolHealth>(|agent, envelope| {
            let reply = envelope.reply_envelope();
            let health = pool_health(
                "surrealdb",
                agent.model.client.is_some(),
                agent.model.connecting,
                agent.model.last_error.as_deref(),
            );

            Reply::pending(async move {
                reply.send(health).await;
            })
        });

        // Initialize connection on startup using spawn pattern
        // NOTE: We spawn the task but DON'T await it here. The task sends messages
        // back to the agent when it completes. This allows before_stop to run
        // immediately when shutdown is requested, which cancels the token and
        // causes the spawned task to exit via tokio::select!.
        agent.after_start(|agent| {
            let config = agent.model.config.clone();
            let cancel_token = agent.model.cancel_token.clone();
            let self_handle = agent.handle().clone();

            if let Some(cfg) = config {
                tracing::info!("SurrealDB agent starting, connecting to database...");

                // Spawn the connection work - it will send a message back when done
                tokio::spawn(async move {
                    // Race connection against cancellation
                    tokio::select! {
                        biased;

                        // Cancellation branch - triggered by before_stop
                        () = async {
                            if let Some(ref token) = cancel_token {
                                token.cancelled().await;
                            } else {
                                // No token, never cancel via this branch
                                std::future::pending::<()>().await;
                            }
                        } => {
                            tracing::info!("SurrealDB connection cancelled during shutdown");
                            self_handle
                                .send(SurrealDbConnectionFailed {
                                    error: "Connection cancelled during shutdown".to_string(),
                                })
                                .await;
                        }

                        // Connection branch
                        result = crate::surrealdb_backend::create_client(&cfg) => {
                            match result {
                                Ok(client) => {
                                    self_handle.send(SurrealDbConnected { client: Arc::new(client) }).await;
                                }
                                Err(e) => {
                                    self_handle
                                        .send(SurrealDbConnectionFailed {
                                            error: e.to_string(),
                                        })
                                        .await;
                                }
                            }
                        }
                    }
                });
            }

            Reply::ready()
        });

        // Graceful cleanup on shutdown
        agent.before_stop(|agent| {
            let cancel_token = agent.model.cancel_token.clone();
            Reply::pending(async move {
                // Cancel any ongoing connection retries first
                if let Some(token) = cancel_token {
                    token.cancel();
                    tracing::debug!("SurrealDB connection retry cancelled");
                }

                // Note: SurrealDB client doesn't have an explicit close method,
                // dropping it will close the connection
                tracing::info!("SurrealDB agent stopping");
            })
        });

        let handle = agent.start().await;
        Ok(handle)
    }
}

// ============================================================================
// ClickHouse Client Agent
// ============================================================================

#[cfg(feature = "clickhouse")]
use super::messages::{ClickHouseClientConnected, ClickHouseClientConnectionFailed};
#[cfg(all(
    feature = "clickhouse",
    not(feature = "database"),
    not(feature = "events"),
    not(feature = "cache"),
    not(feature = "turso"),
    not(feature = "surrealdb")
))]
use acton_reactive::prelude::*;
#[cfg(all(
    feature = "clickhouse",
    not(feature = "database"),
    not(feature = "cache"),
    not(feature = "events"),
    not(feature = "turso"),
    not(feature = "surrealdb")
))]
use std::sync::Arc;
#[cfg(all(
    feature = "clickhouse",
    not(feature = "database"),
    not(feature = "cache"),
    not(feature = "events"),
    not(feature = "turso"),
    not(feature = "surrealdb")
))]
use tokio::sync::RwLock;

/// Shared client storage type for ClickHouse connections
#[cfg(feature = "clickhouse")]
pub type SharedClickHouseClient = Arc<RwLock<Option<clickhouse::Client>>>;

/// State for the ClickHouse client agent
#[cfg(feature = "clickhouse")]
#[derive(Default)]
pub struct ClickHouseClientState {
    /// The underlying ClickHouse client
    pub client: Option<clickhouse::Client>,
    /// Configuration for the ClickHouse connection
    pub config: Option<crate::config::ClickHouseConfig>,
    /// Whether the agent is currently attempting to connect
    pub connecting: bool,
    /// Shared storage that AppState reads from directly
    pub shared_client: Option<SharedClickHouseClient>,
    /// Reason the last connection attempt failed, reported via `GetPoolHealth`
    pub last_error: Option<String>,
    /// Callers parked in `WaitForPoolReady` until the first attempt settles
    pub(crate) ready_waiters: Vec<OutboundEnvelope>,
}

#[cfg(feature = "clickhouse")]
impl std::fmt::Debug for ClickHouseClientState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClickHouseClientState")
            .field("config", &self.config)
            .field("connecting", &self.connecting)
            .field("has_client", &self.client.is_some())
            .finish()
    }
}

/// Agent-based ClickHouse client manager
///
/// Manages a ClickHouse HTTP client connection with automatic connection
/// and graceful shutdown. ClickHouse uses an HTTP-based protocol, so there
/// is no connection pool -- the client itself is lightweight and clonable.
#[cfg(feature = "clickhouse")]
pub struct ClickHousePoolAgent;

#[cfg(feature = "clickhouse")]
impl ClickHousePoolAgent {
    /// Spawn a new ClickHouse pool agent with the given configuration
    ///
    /// # Arguments
    ///
    /// * `runtime` - The agent runtime to spawn into
    /// * `config` - ClickHouse connection configuration
    /// * `shared_client` - Shared storage that will be updated when the client connects.
    pub async fn spawn(
        runtime: &mut ActorRuntime,
        config: crate::config::ClickHouseConfig,
        shared_client: Option<SharedClickHouseClient>,
    ) -> anyhow::Result<ActorHandle> {
        let mut agent = runtime.new_actor::<ClickHouseClientState>();

        // Initialize state before starting
        agent.model.config = Some(config);
        agent.model.connecting = true;
        agent.model.shared_client = shared_client;

        // Handle client connected message
        agent.mutate_on::<ClickHouseClientConnected>(|agent, envelope| {
            let client = envelope.message().client.clone();
            agent.model.client = Some(client.clone());
            agent.model.connecting = false;
            agent.model.last_error = None;
            // Everyone parked in WaitForPoolReady is released here, in
            // the same handler that records the success.
            let health = pool_health("clickhouse", true, false, None);
            let waiters = std::mem::take(&mut agent.model.ready_waiters);

            // Update shared storage if configured
            let shared_client = agent.model.shared_client.clone();

            Reply::pending(async move {
                if let Some(shared) = shared_client {
                    *shared.write().await = Some(client);
                    tracing::info!("ClickHouse client connected and stored in shared state");
                } else {
                    tracing::info!("ClickHouse client connected (no shared state)");
                }

                for waiter in waiters {
                    waiter.send(health.clone()).await;
                }
            })
        });

        // Handle client connection failed message
        agent.mutate_on::<ClickHouseClientConnectionFailed>(|agent, envelope| {
            let error_msg = envelope.message().error.clone();
            agent.model.connecting = false;
            tracing::error!("ClickHouse client connection failed: {}", error_msg);
            // Kept rather than only logged, so `GetPoolHealth` can say
            // why the pool is down instead of merely that it is.
            agent.model.last_error = Some(error_msg);

            let health = pool_health(
                "clickhouse",
                false,
                false,
                agent.model.last_error.as_deref(),
            );
            let waiters = std::mem::take(&mut agent.model.ready_waiters);

            Reply::pending(async move {
                for waiter in waiters {
                    waiter.send(health.clone()).await;
                }
            })
        });

        // Park a caller until the first connection attempt settles. `mutate_on`
        // because parking writes the model, and because the answer must not
        // overtake the Connected/Failed message that produces it.
        agent.mutate_on::<WaitForPoolReady>(|agent, envelope| {
            let reply = envelope.reply_envelope();

            if agent.model.connecting {
                agent.model.ready_waiters.push(reply);
                return Reply::ready();
            }

            let health = pool_health(
                "clickhouse",
                agent.model.client.is_some(),
                false,
                agent.model.last_error.as_deref(),
            );
            Reply::pending(async move {
                reply.send(health).await;
            })
        });

        // Answer health asks. Read-only, so `act_on`.
        agent.act_on::<GetPoolHealth>(|agent, envelope| {
            let reply = envelope.reply_envelope();
            let health = pool_health(
                "clickhouse",
                agent.model.client.is_some(),
                agent.model.connecting,
                agent.model.last_error.as_deref(),
            );

            Reply::pending(async move {
                reply.send(health).await;
            })
        });

        // Initialize connection on startup
        agent.after_start(|agent| {
            let config = agent.model.config.clone();
            let self_handle = agent.handle().clone();

            Reply::pending(async move {
                if let Some(cfg) = config {
                    tracing::info!("ClickHouse pool agent starting, connecting to ClickHouse...");

                    let result = tokio::spawn(async move {
                        crate::clickhouse_backend::create_client(&cfg).await
                    })
                    .await;

                    match result {
                        Ok(Ok(client)) => {
                            self_handle.send(ClickHouseClientConnected { client }).await;
                        }
                        Ok(Err(e)) => {
                            self_handle
                                .send(ClickHouseClientConnectionFailed {
                                    error: e.to_string(),
                                })
                                .await;
                        }
                        Err(e) => {
                            self_handle
                                .send(ClickHouseClientConnectionFailed {
                                    error: format!("Connection task panicked: {}", e),
                                })
                                .await;
                        }
                    }
                }
            })
        });

        // Cleanup on shutdown
        agent.before_stop(|agent| {
            let client = agent.model.client.clone();
            Reply::pending(async move {
                if client.is_some() {
                    tracing::info!("ClickHouse pool agent stopping, closing connection...");
                    drop(client);
                    tracing::info!("ClickHouse client closed");
                }
            })
        });

        let handle = agent.start().await;
        Ok(handle)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[cfg(feature = "turso")]
    mod turso_agent_tests {
        use super::*;
        use crate::agents::messages::HealthStatus;
        use crate::config::{TursoConfig, TursoMode};
        use std::path::PathBuf;

        /// Helper to create a temporary database path
        fn temp_db_path(name: &str) -> PathBuf {
            let mut path = std::env::temp_dir();
            path.push(format!(
                "turso_agent_test_{}_{}.db",
                name,
                std::process::id()
            ));
            path
        }

        /// Helper to clean up test database files
        fn cleanup_db(path: &PathBuf) {
            let _ = std::fs::remove_file(path);
            let _ = std::fs::remove_file(path.with_extension("db-wal"));
            let _ = std::fs::remove_file(path.with_extension("db-shm"));
        }

        #[tokio::test]
        async fn test_turso_agent_spawn_and_connect() {
            let db_path = temp_db_path("agent_spawn");

            let config = TursoConfig {
                mode: TursoMode::Local,
                path: Some(db_path.clone()),
                url: None,
                auth_token: None,
                sync_interval_secs: None,
                encryption_key: None,
                read_your_writes: true,
                max_retries: 0,
                retry_delay_secs: 1,
                optional: false,
                lazy_init: false,
            };

            // Create shared storage
            let shared_db: SharedTursoDb = Arc::new(RwLock::new(None));

            // Initialize agent runtime
            let mut runtime = acton_reactive::prelude::ActonApp::launch_async().await;

            // Spawn the agent
            let handle = TursoDbAgent::spawn(&mut runtime, config, Some(shared_db.clone()))
                .await
                .expect("Failed to spawn TursoDbAgent");

            // WaitForPoolReady is the barrier. A plain GetPoolHealth ask here
            // would race the connection: the agent dials on a spawned task, so
            // the result is not in the mailbox yet when spawn() returns, and
            // the ask would answer Connecting.
            let health = handle.ask(WaitForPoolReady).await.expect("readiness ask");
            assert_eq!(health.status, HealthStatus::Healthy, "{}", health.message);

            // Verify the database is available in shared storage
            let db_guard = shared_db.read().await;
            assert!(
                db_guard.is_some(),
                "Database should be available in shared storage"
            );

            // Test that we can use the database
            if let Some(ref db) = *db_guard {
                let conn = db.connect().expect("Failed to connect");
                conn.execute("CREATE TABLE test (id INTEGER PRIMARY KEY)", ())
                    .await
                    .expect("Failed to create table");
            }

            drop(db_guard);

            // Stop the agent
            let _ = handle.stop().await;

            // Shutdown runtime
            runtime
                .shutdown_all()
                .await
                .expect("Failed to shutdown runtime");

            cleanup_db(&db_path);
        }

        #[tokio::test]
        async fn test_turso_agent_graceful_shutdown() {
            let db_path = temp_db_path("agent_shutdown");

            let config = TursoConfig {
                mode: TursoMode::Local,
                path: Some(db_path.clone()),
                url: None,
                auth_token: None,
                sync_interval_secs: None,
                encryption_key: None,
                read_your_writes: true,
                max_retries: 0,
                retry_delay_secs: 1,
                optional: false,
                lazy_init: false,
            };

            let shared_db: SharedTursoDb = Arc::new(RwLock::new(None));
            let mut runtime = acton_reactive::prelude::ActonApp::launch_async().await;

            let handle = TursoDbAgent::spawn(&mut runtime, config, Some(shared_db.clone()))
                .await
                .expect("Failed to spawn TursoDbAgent");

            let health = handle.ask(WaitForPoolReady).await.expect("readiness ask");
            assert_eq!(health.status, HealthStatus::Healthy, "{}", health.message);

            // Verify connected
            assert!(shared_db.read().await.is_some());

            // Perform a write operation to ensure file is created (libsql defers file creation)
            {
                let db_guard = shared_db.read().await;
                if let Some(ref db) = *db_guard {
                    let conn = db.connect().expect("Failed to connect");
                    conn.execute("CREATE TABLE IF NOT EXISTS _check (id INTEGER)", ())
                        .await
                        .expect("Failed to create table");
                }
            }

            // Stop the agent gracefully
            let _ = handle.stop().await;

            // Shutdown runtime
            runtime
                .shutdown_all()
                .await
                .expect("Failed to shutdown runtime");

            // The file should still exist (database closed gracefully)
            assert!(
                db_path.exists(),
                "Database file should still exist after graceful shutdown"
            );

            cleanup_db(&db_path);
        }

        #[tokio::test]
        async fn test_turso_agent_without_shared_storage() {
            let db_path = temp_db_path("agent_no_shared");

            let config = TursoConfig {
                mode: TursoMode::Local,
                path: Some(db_path.clone()),
                url: None,
                auth_token: None,
                sync_interval_secs: None,
                encryption_key: None,
                read_your_writes: true,
                max_retries: 0,
                retry_delay_secs: 1,
                optional: false,
                lazy_init: false,
            };

            let mut runtime = acton_reactive::prelude::ActonApp::launch_async().await;

            // Spawn without shared storage (None)
            let handle = TursoDbAgent::spawn(&mut runtime, config, None)
                .await
                .expect("Failed to spawn TursoDbAgent");

            // Still healthy without shared storage; the agent owns the state
            // either way, and the shared handle is only a read-side convenience.
            let health = handle.ask(WaitForPoolReady).await.expect("readiness ask");
            assert_eq!(health.status, HealthStatus::Healthy, "{}", health.message);

            // Agent should still work, just won't update shared storage
            let _ = handle.stop().await;
            runtime
                .shutdown_all()
                .await
                .expect("Failed to shutdown runtime");

            cleanup_db(&db_path);
        }

        #[tokio::test]
        async fn test_turso_agent_missing_config_handling() {
            // Remote mode with missing URL should fail during database creation
            let config = TursoConfig {
                mode: TursoMode::Remote,
                path: None,
                url: None, // Missing required URL for Remote mode
                auth_token: Some("some-token".to_string()),
                sync_interval_secs: None,
                encryption_key: None,
                read_your_writes: true,
                max_retries: 0, // No retries
                retry_delay_secs: 1,
                optional: true, // Mark as optional so it doesn't fail hard
                lazy_init: false,
            };

            let shared_db: SharedTursoDb = Arc::new(RwLock::new(None));
            let mut runtime = acton_reactive::prelude::ActonApp::launch_async().await;

            let handle = TursoDbAgent::spawn(&mut runtime, config, Some(shared_db.clone()))
                .await
                .expect("Failed to spawn TursoDbAgent");

            // Waiting on the failure verdict rather than on the clock, and
            // asserting the agent reports *why* it is down.
            let health = handle.ask(WaitForPoolReady).await.expect("readiness ask");
            assert_eq!(health.status, HealthStatus::Unhealthy);
            assert!(
                !health.message.is_empty(),
                "an unhealthy pool should report a reason"
            );

            // Shared storage should remain None due to missing URL
            assert!(
                shared_db.read().await.is_none(),
                "Database should not be available when URL is missing"
            );

            let _ = handle.stop().await;
            runtime
                .shutdown_all()
                .await
                .expect("Failed to shutdown runtime");
        }

        #[tokio::test]
        async fn test_turso_agent_multiple_agents() {
            let db_path1 = temp_db_path("multi_agent_1");
            let db_path2 = temp_db_path("multi_agent_2");

            let config1 = TursoConfig {
                mode: TursoMode::Local,
                path: Some(db_path1.clone()),
                url: None,
                auth_token: None,
                sync_interval_secs: None,
                encryption_key: None,
                read_your_writes: true,
                max_retries: 0,
                retry_delay_secs: 1,
                optional: false,
                lazy_init: false,
            };

            let config2 = TursoConfig {
                mode: TursoMode::Local,
                path: Some(db_path2.clone()),
                url: None,
                auth_token: None,
                sync_interval_secs: None,
                encryption_key: None,
                read_your_writes: true,
                max_retries: 0,
                retry_delay_secs: 1,
                optional: false,
                lazy_init: false,
            };

            let shared_db1: SharedTursoDb = Arc::new(RwLock::new(None));
            let shared_db2: SharedTursoDb = Arc::new(RwLock::new(None));
            let mut runtime = acton_reactive::prelude::ActonApp::launch_async().await;

            // Spawn two agents
            let handle1 = TursoDbAgent::spawn(&mut runtime, config1, Some(shared_db1.clone()))
                .await
                .expect("Failed to spawn first agent");

            let handle2 = TursoDbAgent::spawn(&mut runtime, config2, Some(shared_db2.clone()))
                .await
                .expect("Failed to spawn second agent");

            for (n, h) in [(1, &handle1), (2, &handle2)] {
                let health = h.ask(WaitForPoolReady).await.expect("readiness ask");
                assert_eq!(
                    health.status,
                    HealthStatus::Healthy,
                    "agent {n}: {}",
                    health.message
                );
            }

            // Both should be connected
            assert!(
                shared_db1.read().await.is_some(),
                "First database should be available"
            );
            assert!(
                shared_db2.read().await.is_some(),
                "Second database should be available"
            );

            // Verify they are different databases by creating different tables
            {
                let db1 = shared_db1.read().await;
                let conn1 = db1.as_ref().unwrap().connect().unwrap();
                conn1
                    .execute("CREATE TABLE db1_table (id INTEGER)", ())
                    .await
                    .unwrap();
            }

            {
                let db2 = shared_db2.read().await;
                let conn2 = db2.as_ref().unwrap().connect().unwrap();
                conn2
                    .execute("CREATE TABLE db2_table (id INTEGER)", ())
                    .await
                    .unwrap();
            }

            let _ = handle1.stop().await;
            let _ = handle2.stop().await;
            runtime
                .shutdown_all()
                .await
                .expect("Failed to shutdown runtime");

            cleanup_db(&db_path1);
            cleanup_db(&db_path2);
        }
    }
}
