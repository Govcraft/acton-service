//! Application state management

use std::sync::Arc;

#[cfg(any(feature = "database", feature = "cache", feature = "events"))]
use tokio::sync::RwLock;

#[cfg(feature = "database")]
use sqlx::PgPool;

#[cfg(feature = "cache")]
use deadpool_redis::Pool as RedisPool;

#[cfg(feature = "events")]
use async_nats::Client as NatsClient;

use crate::{config::Config, error::Result};

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    config: Arc<Config>,

    #[cfg(feature = "database")]
    db_pool: Arc<RwLock<Option<PgPool>>>,

    #[cfg(feature = "cache")]
    redis_pool: Arc<RwLock<Option<RedisPool>>>,

    #[cfg(feature = "events")]
    nats_client: Arc<RwLock<Option<NatsClient>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            config: Arc::new(Config::default()),
            #[cfg(feature = "database")]
            db_pool: Arc::new(RwLock::new(None)),
            #[cfg(feature = "cache")]
            redis_pool: Arc::new(RwLock::new(None)),
            #[cfg(feature = "events")]
            nats_client: Arc::new(RwLock::new(None)),
        }
    }
}

impl AppState {
    /// Create a new AppState with the given configuration
    ///
    /// This creates an AppState with no connection pools initialized.
    /// For lazy initialization of connections, use `AppStateBuilder` instead.
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(config),
            #[cfg(feature = "database")]
            db_pool: Arc::new(RwLock::new(None)),
            #[cfg(feature = "cache")]
            redis_pool: Arc::new(RwLock::new(None)),
            #[cfg(feature = "events")]
            nats_client: Arc::new(RwLock::new(None)),
        }
    }

    /// Create a new builder for AppState
    pub fn builder() -> AppStateBuilder {
        AppStateBuilder::new()
    }

    /// Get the configuration
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Get the database pool (async to handle RwLock)
    ///
    /// Returns a cloned PgPool if available. PgPool uses Arc internally,
    /// so cloning is cheap.
    #[cfg(feature = "database")]
    pub async fn db(&self) -> Option<PgPool> {
        self.db_pool.read().await.clone()
    }

    /// Get direct access to the database pool RwLock
    ///
    /// Use this if you need to check availability without acquiring the pool
    #[cfg(feature = "database")]
    pub fn db_lock(&self) -> &Arc<RwLock<Option<PgPool>>> {
        &self.db_pool
    }

    /// Get the Redis pool (async to handle RwLock)
    ///
    /// Returns a cloned RedisPool if available. RedisPool uses Arc internally,
    /// so cloning is cheap.
    #[cfg(feature = "cache")]
    pub async fn redis(&self) -> Option<RedisPool> {
        self.redis_pool.read().await.clone()
    }

    /// Get direct access to the Redis pool RwLock
    #[cfg(feature = "cache")]
    pub fn redis_lock(&self) -> &Arc<RwLock<Option<RedisPool>>> {
        &self.redis_pool
    }

    /// Get the NATS client (async to handle RwLock)
    ///
    /// Returns a cloned NatsClient if available. NatsClient uses Arc internally,
    /// so cloning is cheap.
    #[cfg(feature = "events")]
    pub async fn nats(&self) -> Option<NatsClient> {
        self.nats_client.read().await.clone()
    }

    /// Get direct access to the NATS client RwLock
    #[cfg(feature = "events")]
    pub fn nats_lock(&self) -> &Arc<RwLock<Option<NatsClient>>> {
        &self.nats_client
    }
}

/// Builder for AppState
pub struct AppStateBuilder {
    config: Option<Config>,
    enable_tracing: bool,

    #[cfg(feature = "database")]
    db_pool: Option<PgPool>,

    #[cfg(feature = "cache")]
    redis_pool: Option<RedisPool>,

    #[cfg(feature = "events")]
    nats_client: Option<NatsClient>,
}

impl AppStateBuilder {
    /// Create a new builder with sensible defaults
    ///
    /// By default:
    /// - Config will be loaded from `Config::default()` if not provided
    /// - Tracing will be auto-initialized if not already set up
    pub fn new() -> Self {
        Self {
            config: None,
            enable_tracing: true,
            #[cfg(feature = "database")]
            db_pool: None,
            #[cfg(feature = "cache")]
            redis_pool: None,
            #[cfg(feature = "events")]
            nats_client: None,
        }
    }

    /// Set the configuration
    pub fn config(mut self, config: Config) -> Self {
        self.config = Some(config);
        self
    }

    /// Set the database pool
    #[cfg(feature = "database")]
    pub fn db_pool(mut self, pool: PgPool) -> Self {
        self.db_pool = Some(pool);
        self
    }

    /// Set the Redis pool
    #[cfg(feature = "cache")]
    pub fn redis_pool(mut self, pool: RedisPool) -> Self {
        self.redis_pool = Some(pool);
        self
    }

    /// Set the NATS client
    #[cfg(feature = "events")]
    pub fn nats_client(mut self, client: NatsClient) -> Self {
        self.nats_client = Some(client);
        self
    }

    /// Enable automatic tracing initialization (default: enabled)
    ///
    /// When enabled, the builder will automatically set up tracing with sensible
    /// defaults if it hasn't been initialized already. This is the default behavior.
    pub fn with_tracing(mut self) -> Self {
        self.enable_tracing = true;
        self
    }

    /// Disable automatic tracing initialization
    ///
    /// Use this if you want to set up tracing manually or if your application
    /// already has tracing configured before calling `build()`.
    pub fn without_tracing(mut self) -> Self {
        self.enable_tracing = false;
        self
    }

    /// Initialize tracing with sensible defaults
    ///
    /// This is called automatically during `build()` unless disabled with `without_tracing()`.
    /// It's safe to call multiple times - subsequent calls are no-ops.
    fn init_tracing() {
        use std::sync::Once;
        static INIT: Once = Once::new();

        INIT.call_once(|| {
            tracing_subscriber::fmt()
                .with_max_level(tracing::Level::INFO)
                .with_target(false)
                .init();
            tracing::debug!("Tracing initialized with default configuration");
        });
    }

    /// Build the AppState, initializing connection pools as needed
    ///
    /// This will:
    /// - Use provided config or load `Config::default()` if not set
    /// - Initialize tracing with sensible defaults (unless disabled or already initialized)
    /// - Set up database, cache, and event connections based on config
    pub async fn build(self) -> Result<AppState> {
        // Initialize tracing if enabled and not already set up
        if self.enable_tracing {
            Self::init_tracing();
        }

        // Use provided config or default
        let config = self.config.unwrap_or_default();

        #[cfg(feature = "database")]
        let db_pool = if let Some(pool) = self.db_pool {
            // Pool was provided explicitly
            Arc::new(RwLock::new(Some(pool)))
        } else if let Some(db_config) = &config.database {
            if db_config.lazy_init {
                // Lazy initialization: start with None and connect in background
                let pool_lock = Arc::new(RwLock::new(None));
                let pool_clone = pool_lock.clone();
                let db_config_clone = db_config.clone();

                tokio::spawn(async move {
                    tracing::info!("Initiating lazy database connection...");
                    match crate::database::create_pool(&db_config_clone).await {
                        Ok(pool) => {
                            *pool_clone.write().await = Some(pool);
                            tracing::info!("Lazy database connection established successfully");
                        }
                        Err(e) => {
                            if db_config_clone.optional {
                                tracing::warn!("Optional database connection failed: {}. Service will continue without database.", e);
                            } else {
                                tracing::error!("Required database connection failed: {}. Service is degraded.", e);
                            }
                        }
                    }
                });

                pool_lock
            } else {
                // Eager initialization: connect now
                match crate::database::create_pool(db_config).await {
                    Ok(pool) => Arc::new(RwLock::new(Some(pool))),
                    Err(e) => {
                        if db_config.optional {
                            tracing::warn!("Optional database connection failed: {}. Service starting without database.", e);
                            Arc::new(RwLock::new(None))
                        } else {
                            // Non-optional, fail fast
                            return Err(e);
                        }
                    }
                }
            }
        } else {
            // No database configuration
            Arc::new(RwLock::new(None))
        };

        #[cfg(feature = "cache")]
        let redis_pool = if let Some(pool) = self.redis_pool {
            // Pool was provided explicitly
            Arc::new(RwLock::new(Some(pool)))
        } else if let Some(redis_config) = &config.redis {
            if redis_config.lazy_init {
                // Lazy initialization: start with None and connect in background
                let pool_lock = Arc::new(RwLock::new(None));
                let pool_clone = pool_lock.clone();
                let redis_config_clone = redis_config.clone();

                tokio::spawn(async move {
                    tracing::info!("Initiating lazy Redis connection...");
                    match crate::cache::create_pool(&redis_config_clone).await {
                        Ok(pool) => {
                            *pool_clone.write().await = Some(pool);
                            tracing::info!("Lazy Redis connection established successfully");
                        }
                        Err(e) => {
                            if redis_config_clone.optional {
                                tracing::warn!("Optional Redis connection failed: {}. Service will continue without Redis.", e);
                            } else {
                                tracing::error!("Required Redis connection failed: {}. Service is degraded.", e);
                            }
                        }
                    }
                });

                pool_lock
            } else {
                // Eager initialization: connect now
                match crate::cache::create_pool(redis_config).await {
                    Ok(pool) => Arc::new(RwLock::new(Some(pool))),
                    Err(e) => {
                        if redis_config.optional {
                            tracing::warn!("Optional Redis connection failed: {}. Service starting without Redis.", e);
                            Arc::new(RwLock::new(None))
                        } else {
                            // Non-optional, fail fast
                            return Err(e);
                        }
                    }
                }
            }
        } else {
            // No Redis configuration
            Arc::new(RwLock::new(None))
        };

        #[cfg(feature = "events")]
        let nats_client = if let Some(client) = self.nats_client {
            // Client was provided explicitly
            Arc::new(RwLock::new(Some(client)))
        } else if let Some(nats_config) = &config.nats {
            if nats_config.lazy_init {
                // Lazy initialization: start with None and connect in background
                let client_lock = Arc::new(RwLock::new(None));
                let client_clone = client_lock.clone();
                let nats_config_clone = nats_config.clone();

                tokio::spawn(async move {
                    tracing::info!("Initiating lazy NATS connection...");
                    match crate::events::create_client(&nats_config_clone).await {
                        Ok(client) => {
                            *client_clone.write().await = Some(client);
                            tracing::info!("Lazy NATS connection established successfully");
                        }
                        Err(e) => {
                            if nats_config_clone.optional {
                                tracing::warn!("Optional NATS connection failed: {}. Service will continue without NATS.", e);
                            } else {
                                tracing::error!("Required NATS connection failed: {}. Service is degraded.", e);
                            }
                        }
                    }
                });

                client_lock
            } else {
                // Eager initialization: connect now
                match crate::events::create_client(nats_config).await {
                    Ok(client) => Arc::new(RwLock::new(Some(client))),
                    Err(e) => {
                        if nats_config.optional {
                            tracing::warn!("Optional NATS connection failed: {}. Service starting without NATS.", e);
                            Arc::new(RwLock::new(None))
                        } else {
                            // Non-optional, fail fast
                            return Err(e);
                        }
                    }
                }
            }
        } else {
            // No NATS configuration
            Arc::new(RwLock::new(None))
        };

        Ok(AppState {
            config: Arc::new(config),
            #[cfg(feature = "database")]
            db_pool,
            #[cfg(feature = "cache")]
            redis_pool,
            #[cfg(feature = "events")]
            nats_client,
        })
    }
}

impl Default for AppStateBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_state_builder() {
        let config = Config::default();
        let builder = AppStateBuilder::new()
            .config(config)
            .without_tracing(); // Disable tracing in tests to avoid global subscriber conflicts

        // This should succeed even without connection pools
        let state = builder.build().await.unwrap();
        assert_eq!(state.config().service.name, "acton-service");
    }

    #[tokio::test]
    async fn test_state_builder_defaults() {
        // Test that config defaults work
        let state = AppStateBuilder::new()
            .without_tracing() // Disable tracing in tests
            .build()
            .await
            .unwrap();

        assert_eq!(state.config().service.name, "acton-service");
    }
}
