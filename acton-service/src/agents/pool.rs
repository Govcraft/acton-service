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
use tokio_util::sync::CancellationToken;
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
            })
        });

        // Handle pool connection failed message
        agent.mutate_on::<DatabasePoolConnectionFailed>(|agent, envelope| {
            let error_msg = envelope.message().error.clone();
            agent.model.connecting = false;
            tracing::error!("Database pool connection failed: {}", error_msg);

            Reply::ready()
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
            })
        });

        // Handle pool connection failed message
        agent.mutate_on::<RedisPoolConnectionFailed>(|agent, envelope| {
            let error_msg = envelope.message().error.clone();
            agent.model.connecting = false;
            tracing::error!("Redis pool connection failed: {}", error_msg);

            Reply::ready()
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
            })
        });

        // Handle client connection failed message
        agent.mutate_on::<NatsClientConnectionFailed>(|agent, envelope| {
            let error_msg = envelope.message().error.clone();
            agent.model.connecting = false;
            tracing::error!("NATS client connection failed: {}", error_msg);

            Reply::ready()
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

#[cfg(all(feature = "turso", not(feature = "database"), not(feature = "cache"), not(feature = "events")))]
use std::sync::Arc;
#[cfg(all(feature = "turso", not(feature = "database"), not(feature = "cache"), not(feature = "events")))]
use tokio::sync::RwLock;
#[cfg(all(feature = "turso", not(feature = "database")))]
use tokio_util::sync::CancellationToken;
#[cfg(all(feature = "turso", not(feature = "database"), not(feature = "events"), not(feature = "cache")))]
use acton_reactive::prelude::*;
#[cfg(feature = "turso")]
use super::messages::{TursoDbConnected, TursoDbConnectionFailed};

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
            })
        });

        // Handle database connection failed message
        agent.mutate_on::<TursoDbConnectionFailed>(|agent, envelope| {
            let error_msg = envelope.message().error.clone();
            agent.model.connecting = false;
            tracing::error!("Turso database connection failed: {}", error_msg);

            Reply::ready()
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
                tracing::info!("Turso database agent starting, connecting to database (mode={:?})...", cfg.mode);

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

#[cfg(all(feature = "surrealdb", not(feature = "database"), not(feature = "cache"), not(feature = "events"), not(feature = "turso")))]
use std::sync::Arc;
#[cfg(all(feature = "surrealdb", not(feature = "database"), not(feature = "cache"), not(feature = "events"), not(feature = "turso")))]
use tokio::sync::RwLock;
#[cfg(all(feature = "surrealdb", not(feature = "database"), not(feature = "turso")))]
use tokio_util::sync::CancellationToken;
#[cfg(all(feature = "surrealdb", not(feature = "database"), not(feature = "events"), not(feature = "cache"), not(feature = "turso")))]
use acton_reactive::prelude::*;
#[cfg(feature = "surrealdb")]
use super::messages::{SurrealDbConnected, SurrealDbConnectionFailed};

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
            })
        });

        // Handle client connection failed message
        agent.mutate_on::<SurrealDbConnectionFailed>(|agent, envelope| {
            let error_msg = envelope.message().error.clone();
            agent.model.connecting = false;
            tracing::error!("SurrealDB client connection failed: {}", error_msg);

            Reply::ready()
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
        use std::path::PathBuf;
        use crate::config::{TursoConfig, TursoMode};

        /// Helper to create a temporary database path
        fn temp_db_path(name: &str) -> PathBuf {
            let mut path = std::env::temp_dir();
            path.push(format!("turso_agent_test_{}_{}.db", name, std::process::id()));
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

            // Wait a bit for the connection to be established
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            // Verify the database is available in shared storage
            let db_guard = shared_db.read().await;
            assert!(db_guard.is_some(), "Database should be available in shared storage");

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
            runtime.shutdown_all().await.expect("Failed to shutdown runtime");

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

            // Wait for connection
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

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
            runtime.shutdown_all().await.expect("Failed to shutdown runtime");

            // The file should still exist (database closed gracefully)
            assert!(db_path.exists(), "Database file should still exist after graceful shutdown");

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

            // Wait for connection
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            // Agent should still work, just won't update shared storage
            let _ = handle.stop().await;
            runtime.shutdown_all().await.expect("Failed to shutdown runtime");

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

            // Wait for connection attempt to complete
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            // Shared storage should remain None due to missing URL
            assert!(
                shared_db.read().await.is_none(),
                "Database should not be available when URL is missing"
            );

            let _ = handle.stop().await;
            runtime.shutdown_all().await.expect("Failed to shutdown runtime");
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

            // Wait for connections
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            // Both should be connected
            assert!(shared_db1.read().await.is_some(), "First database should be available");
            assert!(shared_db2.read().await.is_some(), "Second database should be available");

            // Verify they are different databases by creating different tables
            {
                let db1 = shared_db1.read().await;
                let conn1 = db1.as_ref().unwrap().connect().unwrap();
                conn1.execute("CREATE TABLE db1_table (id INTEGER)", ())
                    .await
                    .unwrap();
            }

            {
                let db2 = shared_db2.read().await;
                let conn2 = db2.as_ref().unwrap().connect().unwrap();
                conn2.execute("CREATE TABLE db2_table (id INTEGER)", ())
                    .await
                    .unwrap();
            }

            let _ = handle1.stop().await;
            let _ = handle2.stop().await;
            runtime.shutdown_all().await.expect("Failed to shutdown runtime");

            cleanup_db(&db_path1);
            cleanup_db(&db_path2);
        }
    }
}
