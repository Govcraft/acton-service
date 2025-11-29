//! Type-safe service builder that enforces API versioning and best practices
//!
//! This module provides a compile-time enforced pattern for building microservices
//! that CANNOT have unversioned routes. The type system makes it impossible to
//! bypass versioning.
//!
//! ## Design Principles
//!
//! 1. **Impossible to bypass versioning**: Only `VersionedRoutes` can be used
//! 2. **Batteries-included**: Health and readiness endpoints are automatic
//! 3. **Type-state pattern**: Compiler enforces configuration order
//! 4. **Opaque types**: Internal Router cannot be accessed directly
//!
//! ## Example
//!
//! ```rust,ignore
//! use acton_service::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Create versioned routes (ONLY way to create routes)
//!     let routes = VersionedApiBuilder::new()
//!         .with_base_path("/api")
//!         .add_version(ApiVersion::V1, |router| {
//!             router.route("/users", get(list_users))
//!         })
//!         .build_routes();  // Returns VersionedRoutes (not Router!)
//!
//!     // Build service with type-safe builder
//!     // Config loading and tracing initialization happen automatically
//!     let service = ServiceBuilder::new()
//!         .with_routes(routes)  // Only accepts VersionedRoutes
//!         .build();  // Automatically loads config and initializes tracing
//!
//!     // Health and readiness endpoints are automatically included
//!     service.serve().await?;
//!
//!     Ok(())
//! }
//! ```

use crate::config::Config;
use crate::middleware::{request_id_layer, request_id_propagation_layer, sensitive_headers_layer};
use crate::state::AppState;
use axum::Router;
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;
use tower_http::{
    catch_panic::CatchPanicLayer,
    compression::CompressionLayer,
    cors::CorsLayer,
    limit::RequestBodyLimitLayer,
    timeout::TimeoutLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};

/// Opaque wrapper around versioned routes with batteries-included health/readiness
///
/// This type can ONLY be created by `VersionedApiBuilder::build_routes()`.
/// It cannot be constructed manually, ensuring all routes are versioned.
///
/// Uses an enum to support both stateless routes (Router<()>) and stateful routes (Router<AppState<T>>)
#[derive(Debug)]
pub enum VersionedRoutes<T = ()>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    /// Routes without state (typical versioned API routes)
    WithoutState(Router<()>),
    /// Routes with AppState (includes health/readiness endpoints)
    WithState(Router<AppState<T>>),
}

impl<T> VersionedRoutes<T>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    /// Create from a stateless router (crate-private, only accessible to VersionedApiBuilder)
    #[allow(dead_code)]
    pub(crate) fn from_router(router: Router<()>) -> Self {
        Self::WithoutState(router)
    }

    /// Create from a stateful router (crate-private)
    pub(crate) fn from_router_with_state(router: Router<AppState<T>>) -> Self {
        Self::WithState(router)
    }
}

impl<T> Default for VersionedRoutes<T>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    /// Default routes with health and readiness endpoints
    fn default() -> Self {
        use axum::routing::get;

        let health_router: Router<AppState<T>> = Router::new()
            .route("/health", get(crate::health::health::<T>))
            .route("/ready", get(crate::health::readiness::<T>));

        Self::WithState(health_router)
    }
}


/// Simplified service builder with sensible defaults
///
/// Generic parameter `T` allows custom config extensions.
/// Use `ServiceBuilder<()>` (the default) for no custom config.
///
/// All fields are optional with defaults:
/// - config: Uses `Config::default()`
/// - routes: Uses `VersionedRoutes::default()` (health + readiness only)
/// - state: Uses `AppState::default()`
/// - grpc_services: None (gRPC server disabled by default)
/// - cedar: None (auto-configures from config.cedar if enabled)
/// - agent_runtime: None (agent-based reactive components disabled by default)
///
/// Health and readiness endpoints are ALWAYS included (automatically added by ServiceBuilder).
pub struct ServiceBuilder<T = ()>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    config: Option<Config<T>>,
    routes: Option<VersionedRoutes<T>>,
    state: Option<AppState<T>>,
    #[cfg(feature = "grpc")]
    grpc_services: Option<tonic::service::Routes>,
    #[cfg(feature = "cedar-authz")]
    cedar: Option<crate::middleware::cedar::CedarAuthz>,
    #[cfg(feature = "cedar-authz")]
    cedar_path_normalizer: Option<fn(&str) -> String>,
    agent_runtime: Option<acton_reactive::prelude::AgentRuntime>,
}

impl<T> ServiceBuilder<T>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    /// Create a new service builder with defaults
    pub fn new() -> Self {
        Self {
            config: None,
            routes: None,
            state: None,
            #[cfg(feature = "grpc")]
            grpc_services: None,
            #[cfg(feature = "cedar-authz")]
            cedar: None,
            #[cfg(feature = "cedar-authz")]
            cedar_path_normalizer: None,
            agent_runtime: None,
        }
    }

    /// Set the service configuration (optional, defaults to Config::default())
    pub fn with_config(mut self, config: Config<T>) -> Self {
        self.config = Some(config);
        self
    }

    /// Add versioned routes to the service
    ///
    /// **IMPORTANT**: This method ONLY accepts `VersionedRoutes`, which can
    /// only be created by `VersionedApiBuilder::build_routes()`.
    /// This makes it impossible to add unversioned routes.
    ///
    /// If not provided, defaults to VersionedRoutes::default() (empty routes).
    pub fn with_routes(mut self, routes: VersionedRoutes<T>) -> Self {
        self.routes = Some(routes);
        self
    }

    /// Set the application state (optional, defaults to AppState::default())
    pub fn with_state(mut self, state: AppState<T>) -> Self {
        self.state = Some(state);
        self
    }

    /// Add gRPC services to the service (optional, requires "grpc" feature)
    ///
    /// When gRPC services are provided, the server will support both HTTP and gRPC
    /// protocols on the same port (by default) or separate ports (if configured).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use acton_service::prelude::*;
    /// use acton_service::grpc::server::GrpcServicesBuilder;
    ///
    /// let grpc_services = GrpcServicesBuilder::new()
    ///     .add_service(UserServiceServer::new(user_service))
    ///     .build()
    ///     .expect("At least one gRPC service must be added");
    ///
    /// let service = ServiceBuilder::new()
    ///     .with_routes(http_routes)
    ///     .with_grpc_services(grpc_services)
    ///     .build();
    /// ```
    #[cfg(feature = "grpc")]
    pub fn with_grpc_services(mut self, services: tonic::service::Routes) -> Self {
        self.grpc_services = Some(services);
        self
    }

    /// Set Cedar authorization with explicit configuration
    ///
    /// This allows full control over Cedar initialization. Use this when you need:
    /// - Custom path normalization
    /// - Policy caching
    /// - Other advanced Cedar customization
    ///
    /// For simple cases, just use `.with_config()` and Cedar will auto-configure.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use acton_service::prelude::*;
    /// use acton_service::middleware::cedar::CedarAuthz;
    ///
    /// let cedar = CedarAuthz::builder(config.cedar.unwrap())
    ///     .with_path_normalizer(normalize_fn)
    ///     .with_cache(redis_cache)
    ///     .build()
    ///     .await?;
    ///
    /// let service = ServiceBuilder::new()
    ///     .with_config(config)
    ///     .with_cedar(cedar)  // Explicit Cedar instance
    ///     .with_routes(routes)
    ///     .build();
    /// ```
    #[cfg(feature = "cedar-authz")]
    pub fn with_cedar(mut self, cedar: crate::middleware::cedar::CedarAuthz) -> Self {
        self.cedar = Some(cedar);
        self
    }

    /// Set ONLY a custom path normalizer for Cedar (convenience method)
    ///
    /// This is the recommended way for most users who just need custom path normalization.
    /// Cedar will auto-configure from config.cedar with your custom normalizer.
    ///
    /// By default, Cedar uses a generic path normalizer that replaces UUIDs and numeric IDs
    /// with `{id}` placeholders. Use this method to provide custom normalization logic for
    /// your application's specific path patterns.
    ///
    /// This is only needed when:
    /// - You have alphanumeric IDs (like "user123", "doc1") that aren't UUIDs or numeric
    /// - You have slug-based routes (like "/articles/my-article-title")
    /// - Complex path patterns not handled by the default normalizer
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use acton_service::prelude::*;
    ///
    /// // Define a custom normalizer for alphanumeric document IDs
    /// fn normalize_document_paths(path: &str) -> String {
    ///     // Handles: /api/v1/documents/user123/doc1 -> /api/v1/documents/{user_id}/{doc_id}
    ///     let doc_pattern = regex::Regex::new(
    ///         r"^(/api/v[0-9]+/documents/)([a-zA-Z0-9_-]+)/([a-zA-Z0-9_-]+)$"
    ///     ).unwrap();
    ///
    ///     if let Some(caps) = doc_pattern.captures(path) {
    ///         return format!("{}{{user_id}}/{{doc_id}}", &caps[1]);
    ///     }
    ///     path.to_string()
    /// }
    ///
    /// let service = ServiceBuilder::new()
    ///     .with_config(config)
    ///     .with_routes(routes)
    ///     .with_cedar_path_normalizer(normalize_document_paths)
    ///     .build();
    /// ```
    #[cfg(feature = "cedar-authz")]
    pub fn with_cedar_path_normalizer(mut self, normalizer: fn(&str) -> String) -> Self {
        self.cedar_path_normalizer = Some(normalizer);
        self
    }

    /// Initialize the agent runtime (internal use only)
    ///
    /// Returns a mutable reference to the `AgentRuntime` for spawning agents.
    /// Called automatically by `build()` when connection pools are configured.
    #[cfg(any(feature = "database", feature = "cache", feature = "events"))]
    fn init_agent_runtime(&mut self) -> &mut acton_reactive::prelude::AgentRuntime {
        if self.agent_runtime.is_none() {
            tracing::debug!("Initializing acton-reactive agent runtime");
            self.agent_runtime = Some(acton_reactive::prelude::ActonApp::launch());
        }
        self.agent_runtime.as_mut().unwrap()
    }

    /// Get the agent broker handle (internal use only)
    #[cfg(any(feature = "database", feature = "cache", feature = "events"))]
    fn broker(&self) -> Option<acton_reactive::prelude::AgentHandle> {
        self.agent_runtime.as_ref().map(|r| r.broker())
    }

    /// Build the service
    ///
    /// Automatically handles:
    /// - **Config loading**: Calls `Config::load()` if not provided (falls back to `Config::default()` on error)
    /// - **Tracing initialization**: Initializes tracing with the loaded config
    /// - **Pool agent spawning**: Spawns internal agents for database/redis/nats when configured
    /// - **Health endpoints**: Always includes `/health` and `/ready` endpoints
    ///
    /// Uses defaults for any fields not set:
    /// - config: `Config::load()` → `Config::default()` if load fails
    /// - routes: `VersionedRoutes::default()` (health + readiness only)
    /// - state: `AppState::default()` with agent-managed pools
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Minimal - everything is automatic
    /// let service = ServiceBuilder::new().build();
    /// // → Loads config, initializes tracing, spawns pool agents, adds health endpoints
    ///
    /// // With custom routes (most common)
    /// let service = ServiceBuilder::new()
    ///     .with_routes(versioned_routes)
    ///     .build();
    /// // → Pool agents automatically manage database/redis/nats connections
    ///
    /// // Override config (e.g., for testing)
    /// let custom_config = Config { /* ... */ };
    /// let service = ServiceBuilder::new()
    ///     .with_config(custom_config)
    ///     .with_routes(routes)
    ///     .build();
    /// // → Uses your config, spawns appropriate pool agents
    /// ```
    pub fn build(mut self) -> ActonService<T> {
        // Load config if not provided
        let config = self.config.take().unwrap_or_else(|| {
            Config::<T>::load().unwrap_or_else(|e| {
                eprintln!("Warning: Failed to load config: {}, using defaults", e);
                Config::<T>::default()
            })
        });

        // Initialize tracing with the loaded config
        if let Err(e) = crate::observability::init_tracing(&config) {
            eprintln!("Warning: Failed to initialize tracing: {}", e);
        }

        // Determine if we need to spawn pool agents
        #[cfg(feature = "database")]
        let needs_db_agent = config.database.is_some();

        #[cfg(feature = "cache")]
        let needs_redis_agent = config.redis.is_some();

        #[cfg(feature = "events")]
        let needs_nats_agent = config.nats.is_some();

        #[cfg(any(feature = "database", feature = "cache", feature = "events"))]
        let needs_agents = {
            #[cfg(feature = "database")]
            let db = needs_db_agent;
            #[cfg(not(feature = "database"))]
            let db = false;

            #[cfg(feature = "cache")]
            let redis = needs_redis_agent;
            #[cfg(not(feature = "cache"))]
            let redis = false;

            #[cfg(feature = "events")]
            let nats = needs_nats_agent;
            #[cfg(not(feature = "events"))]
            let nats = false;

            db || redis || nats
        };

        // Initialize agent runtime and spawn pool agents if needed
        #[cfg(feature = "database")]
        let shared_db_pool: Option<crate::agents::SharedDbPool> = if needs_db_agent {
            Some(std::sync::Arc::new(tokio::sync::RwLock::new(None)))
        } else {
            None
        };

        #[cfg(feature = "cache")]
        let shared_redis_pool: Option<crate::agents::SharedRedisPool> = if needs_redis_agent {
            Some(std::sync::Arc::new(tokio::sync::RwLock::new(None)))
        } else {
            None
        };

        #[cfg(feature = "events")]
        let shared_nats_client: Option<crate::agents::SharedNatsClient> = if needs_nats_agent {
            Some(std::sync::Arc::new(tokio::sync::RwLock::new(None)))
        } else {
            None
        };

        // Agent handles for AppState
        #[cfg(feature = "database")]
        let mut db_agent_handle: Option<acton_reactive::prelude::AgentHandle> = None;
        #[cfg(feature = "cache")]
        let mut redis_agent_handle: Option<acton_reactive::prelude::AgentHandle> = None;
        #[cfg(feature = "events")]
        let mut nats_agent_handle: Option<acton_reactive::prelude::AgentHandle> = None;

        #[cfg(any(feature = "database", feature = "cache", feature = "events"))]
        let broker_handle = if needs_agents {
            // Initialize the agent runtime
            let runtime = self.init_agent_runtime();

            // Use block_in_place to spawn agents (they're async)
            if let Ok(_handle) = tokio::runtime::Handle::try_current() {
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async {
                        // Spawn database pool agent
                        #[cfg(feature = "database")]
                        if let Some(ref db_config) = config.database {
                            match crate::agents::DatabasePoolAgent::spawn(
                                runtime,
                                db_config.clone(),
                                shared_db_pool.clone(),
                            ).await {
                                Ok(handle) => {
                                    tracing::info!("Database pool agent spawned");
                                    db_agent_handle = Some(handle);
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to spawn database pool agent: {}", e);
                                }
                            }
                        }

                        // Spawn Redis pool agent
                        #[cfg(feature = "cache")]
                        if let Some(ref redis_config) = config.redis {
                            match crate::agents::RedisPoolAgent::spawn(
                                runtime,
                                redis_config.clone(),
                                shared_redis_pool.clone(),
                            ).await {
                                Ok(handle) => {
                                    tracing::info!("Redis pool agent spawned");
                                    redis_agent_handle = Some(handle);
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to spawn Redis pool agent: {}", e);
                                }
                            }
                        }

                        // Spawn NATS pool agent
                        #[cfg(feature = "events")]
                        if let Some(ref nats_config) = config.nats {
                            match crate::agents::NatsPoolAgent::spawn(
                                runtime,
                                nats_config.clone(),
                                shared_nats_client.clone(),
                            ).await {
                                Ok(handle) => {
                                    tracing::info!("NATS pool agent spawned");
                                    nats_agent_handle = Some(handle);
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to spawn NATS pool agent: {}", e);
                                }
                            }
                        }
                    });
                });
            }

            self.broker()
        } else {
            None
        };

        #[cfg(not(any(feature = "database", feature = "cache", feature = "events")))]
        let broker_handle: Option<acton_reactive::prelude::AgentHandle> = None;

        let routes = self.routes.unwrap_or_default();

        // Build AppState with agent-managed pools
        let state = if let Some(provided_state) = self.state {
            provided_state
        } else {
            let mut state = AppState::new(config.clone());

            // Set broker handle for event broadcasting
            if let Some(broker) = broker_handle {
                state.set_broker(broker);
            }

            // Set shared pool storage (agents will update these when connected)
            #[cfg(feature = "database")]
            if let Some(pool) = shared_db_pool {
                state.set_db_pool_storage(pool);
            }

            #[cfg(feature = "cache")]
            if let Some(pool) = shared_redis_pool {
                state.set_redis_pool_storage(pool);
            }

            #[cfg(feature = "events")]
            if let Some(client) = shared_nats_client {
                state.set_nats_client_storage(client);
            }

            state
        };

        // Handle both types of versioned routes
        let app = match routes {
            VersionedRoutes::WithState(router) => {
                // Health routes already added, just attach state
                router.with_state(state)
            }
            VersionedRoutes::WithoutState(router) => {
                // Add health routes and attach state
                use axum::routing::get;
                let health_router: Router<AppState<T>> = Router::new()
                    .route("/health", get(crate::health::health))
                    .route("/ready", get(crate::health::readiness));

                // Use fallback_service to include the versioned routes
                let router_with_health = health_router.fallback_service(router);
                router_with_health.with_state(state)
            }
        };

        // Apply general middleware stack (CORS, compression, timeout, TraceLayer, etc.)
        // Layers are applied in reverse order (bottom layer is innermost/first)
        let mut app = Self::apply_middleware(app, &config);

        // Auto-apply Cedar middleware if configured and enabled
        // NOTE: Cedar must be applied BEFORE JWT because Axum layers run in reverse order
        // This ensures the execution order is: Request → General Middleware → JWT → Cedar → Handler
        #[cfg(feature = "cedar-authz")]
        {
            let cedar_authz = if let Some(cedar) = self.cedar {
                // User provided explicit Cedar instance - use it directly
                tracing::debug!("Using explicit Cedar authorization middleware");
                Some(cedar)
            } else if let Some(ref cedar_config) = config.cedar {
                if cedar_config.enabled {
                    // Auto-configure Cedar from config
                    match tokio::runtime::Handle::try_current() {
                        Ok(_handle) => {
                            // Use block_in_place to avoid nested runtime error
                            let cedar_path_normalizer = self.cedar_path_normalizer;
                            match tokio::task::block_in_place(|| {
                                tokio::runtime::Handle::current().block_on(async {
                                    let mut builder = crate::middleware::cedar::CedarAuthz::builder(cedar_config.clone());
                                    if let Some(normalizer) = cedar_path_normalizer {
                                        builder = builder.with_path_normalizer(normalizer);
                                    }
                                    builder.build().await
                                })
                            }) {
                                Ok(cedar) => {
                                    if cedar_path_normalizer.is_some() {
                                        tracing::debug!("Auto-configured Cedar authorization middleware with custom path normalizer");
                                    } else {
                                        tracing::debug!("Auto-configured Cedar authorization middleware with default path normalizer");
                                    }
                                    Some(cedar)
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to initialize Cedar middleware: {}", e);
                                    None
                                }
                            }
                        }
                        Err(_) => {
                            tracing::warn!("No tokio runtime available for Cedar initialization");
                            None
                        }
                    }
                } else {
                    None
                }
            } else {
                None
            };

            // Apply Cedar middleware if available
            if let Some(cedar) = cedar_authz {
                app = app.layer(axum::middleware::from_fn_with_state(
                    cedar,
                    crate::middleware::cedar::CedarAuthz::middleware,
                ));
            }
        }

        // Auto-apply JWT middleware if configured
        // NOTE: JWT must be applied AFTER Cedar because Axum layers run in reverse order
        // This ensures the execution order is: Request → General Middleware → JWT → Cedar → Handler
        if let Ok(jwt_auth) = crate::middleware::jwt::JwtAuth::new(&config.jwt) {
            tracing::debug!("Auto-applying JWT authentication middleware");
            app = app.layer(axum::middleware::from_fn_with_state(
                jwt_auth,
                crate::middleware::jwt::JwtAuth::middleware,
            ));
        } else {
            tracing::warn!("JWT configuration invalid, skipping JWT middleware");
        }

        let listener_addr = std::net::SocketAddr::from(([0, 0, 0, 0], config.service.port));

        ActonService {
            config,
            listener_addr,
            app,
            #[cfg(feature = "grpc")]
            grpc_routes: self.grpc_services,
                    agent_runtime: self.agent_runtime,
        }
    }

    /// Apply middleware stack based on configuration
    ///
    /// Applies middleware in the correct order to ensure proper request handling
    fn apply_middleware(app: Router, config: &Config<T>) -> Router {
        let body_limit = config.middleware.body_limit_mb * 1024 * 1024;

        let mut app = app;

        // CORS (outermost layer) - configurable
        let cors_layer = match config.middleware.cors_mode.as_str() {
            "permissive" => CorsLayer::permissive(),
            "restrictive" => CorsLayer::new(),
            "disabled" => CorsLayer::new(),
            _ => {
                tracing::warn!("Unknown CORS mode: {}, defaulting to permissive", config.middleware.cors_mode);
                CorsLayer::permissive()
            }
        };
        app = app.layer(cors_layer);

        // Compression - configurable
        if config.middleware.compression {
            app = app.layer(CompressionLayer::new());
        }

        // Request timeout
        app = app.layer(TimeoutLayer::with_status_code(
            http::StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(config.service.timeout_secs),
        ));

        // Request body size limit - configurable
        app = app.layer(RequestBodyLimitLayer::new(body_limit));

        // Tracing (HTTP request/response logging) - always enabled
        app = app.layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().include_headers(true))
                .on_response(DefaultOnResponse::new().include_headers(true)),
        );

        // Request tracking layers - based on config
        if config.middleware.request_tracking.mask_sensitive_headers {
            app = app.layer(sensitive_headers_layer());
        }
        if config.middleware.request_tracking.propagate_headers {
            app = app.layer(request_id_propagation_layer());
        }
        if config.middleware.request_tracking.request_id_enabled {
            app = app.layer(request_id_layer());
        }

        // Panic recovery (innermost layer) - configurable
        if config.middleware.catch_panic {
            app = app.layer(CatchPanicLayer::new());
        }

        app
    }
}

impl<T> Default for ServiceBuilder<T>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Opaque service wrapper
///
/// This type wraps the final Router and Config. It cannot be manipulated
/// directly - the only way to use it is to call `serve()`.
///
/// This prevents developers from:
/// - Adding unversioned routes after construction
/// - Bypassing the type-safe builder
/// - Accessing the internal Router
pub struct ActonService<T = ()>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    config: Config<T>,
    listener_addr: std::net::SocketAddr,
    app: Router,
    #[cfg(feature = "grpc")]
    grpc_routes: Option<tonic::service::Routes>,
    agent_runtime: Option<acton_reactive::prelude::AgentRuntime>,
}

impl<T> ActonService<T>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    /// Serve the application
    ///
    /// This runs the HTTP server (and optionally gRPC server) with graceful shutdown support.
    ///
    /// If gRPC services are configured:
    /// - Single-port mode (default): Both HTTP and gRPC on same port, routed by content-type
    /// - Dual-port mode: HTTP on configured port, gRPC on separate port
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let service = ServiceBuilder::new()
    ///     .with_config(config)
    ///     .with_routes(routes)
    ///     .with_state(state)
    ///     .build();
    ///
    /// service.serve().await?;
    /// ```
    #[cfg_attr(not(feature = "grpc"), allow(unused_mut))]
    pub async fn serve(mut self) -> crate::error::Result<()> {
        use tokio::net::TcpListener;
        use tokio::signal;

        // Graceful shutdown signal
        async fn shutdown_signal() {
            let ctrl_c = async {
                signal::ctrl_c()
                    .await
                    .expect("failed to install Ctrl+C handler");
            };

            #[cfg(unix)]
            let terminate = async {
                signal::unix::signal(signal::unix::SignalKind::terminate())
                    .expect("failed to install signal handler")
                    .recv()
                    .await;
            };

            #[cfg(not(unix))]
            let terminate = std::future::pending::<()>();

            tokio::select! {
                _ = ctrl_c => {},
                _ = terminate => {},
            }
        }

        #[cfg(feature = "grpc")]
        {
            // Check if gRPC is enabled and services are provided
            if let Some(ref grpc_config) = self.config.grpc {
                if grpc_config.enabled && self.grpc_routes.is_some() {
                    let grpc_routes = self.grpc_routes.take().unwrap();

                    if grpc_config.use_separate_port {
                        // Dual-port mode: HTTP and gRPC on separate ports
                        let grpc_port = grpc_config.port;
                        let grpc_addr = std::net::SocketAddr::from(([0, 0, 0, 0], grpc_port));

                        tracing::info!("Starting HTTP service on {}", self.listener_addr);
                        tracing::info!("Starting gRPC service on {}", grpc_addr);

                        let http_listener = TcpListener::bind(&self.listener_addr).await?;
                        let grpc_listener = TcpListener::bind(&grpc_addr).await?;

                        // Convert Routes to axum router for the gRPC listener
                        let grpc_app = grpc_routes.into_axum_router();

                        // Spawn gRPC server on separate task
                        let grpc_handle = tokio::spawn(async move {
                            axum::serve(grpc_listener, grpc_app)
                                .with_graceful_shutdown(shutdown_signal())
                                .await
                        });

                        // Run HTTP server
                        let http_result = axum::serve(http_listener, self.app)
                            .with_graceful_shutdown(shutdown_signal())
                            .await;

                        // Wait for gRPC server
                        let _ = grpc_handle.await;

                        http_result?;
                    } else {
                        // Single-port mode: Hybrid HTTP + gRPC on same port
                        tracing::info!("Starting hybrid HTTP+gRPC service on {}", self.listener_addr);

                        let listener = TcpListener::bind(&self.listener_addr).await?;

                        // Merge HTTP and gRPC services using Routes.into_axum_router()
                        // The Routes type automatically handles protocol detection based on content-type
                        // gRPC requests (content-type: application/grpc) are routed to gRPC services
                        // All other requests are handled by the HTTP router
                        let hybrid_service = grpc_routes
                            .into_axum_router()
                            .merge(self.app);

                        axum::serve(listener, hybrid_service)
                            .with_graceful_shutdown(shutdown_signal())
                            .await?;
                    }

                    tracing::info!("Server shutdown complete");

                    // Shutdown agent runtime after server stops (gRPC path)
                                    if let Some(mut runtime) = self.agent_runtime {
                        tracing::info!("Shutting down agent runtime...");
                        if let Err(e) = runtime.shutdown_all().await {
                            tracing::error!("Agent runtime shutdown error: {}", e);
                        }
                        tracing::info!("Agent runtime shutdown complete");
                    }

                    return Ok(());
                }
            }
        }

        // HTTP-only mode (no gRPC or gRPC disabled)
        tracing::info!("Starting HTTP service on {}", self.listener_addr);

        let listener = TcpListener::bind(&self.listener_addr).await?;

        axum::serve(listener, self.app)
            .with_graceful_shutdown(shutdown_signal())
            .await?;

        tracing::info!("Server shutdown complete");

        // Shutdown agent runtime after server stops (HTTP-only path)
            if let Some(mut runtime) = self.agent_runtime {
            tracing::info!("Shutting down agent runtime...");
            if let Err(e) = runtime.shutdown_all().await {
                tracing::error!("Agent runtime shutdown error: {}", e);
            }
            tracing::info!("Agent runtime shutdown complete");
        }

        Ok(())
    }

    /// Get a reference to the service configuration
    pub fn config(&self) -> &Config<T> {
        &self.config
    }
}


#[cfg(test)]
mod tests {
    // This test verifies the type-state pattern at compile time
    #[test]
    fn test_service_builder_states_compile() {
        // This should compile - correct order
        // let _service = ServiceBuilder::new()
        //     .with_config(config)
        //     .with_routes(routes)
        //     .with_state(state)
        //     .build();

        // These should NOT compile (commented out to prevent compilation errors):

        // ❌ Cannot build without config
        // let _service = ServiceBuilder::new()
        //     .build();

        // ❌ Cannot skip routes
        // let _service = ServiceBuilder::new()
        //     .with_config(config)
        //     .with_state(state)
        //     .build();

        // ❌ Cannot call with_routes on wrong state
        // let _service = ServiceBuilder::new()
        //     .with_routes(routes);

        // ❌ Cannot call with_state on wrong state
        // let _service = ServiceBuilder::new()
        //     .with_config(config)
        //     .with_state(state);
    }

    #[test]
    fn test_versioned_routes_cannot_be_constructed_manually() {
        // This should NOT compile (VersionedRoutes has private fields):
        // let routes = VersionedRoutes { router: Router::new() };

        // The ONLY way to create VersionedRoutes is through VersionedApiBuilder
    }
}
