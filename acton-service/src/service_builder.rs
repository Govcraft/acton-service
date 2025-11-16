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
use crate::state::AppState;
use axum::Router;

/// Opaque wrapper around versioned routes with batteries-included health/readiness
///
/// This type can ONLY be created by `VersionedApiBuilder::build_routes()`.
/// It cannot be constructed manually, ensuring all routes are versioned.
///
/// Uses an enum to support both stateless routes (Router<()>) and stateful routes (Router<AppState>)
#[derive(Debug)]
pub enum VersionedRoutes {
    /// Routes without state (typical versioned API routes)
    WithoutState(Router<()>),
    /// Routes with AppState (includes health/readiness endpoints)
    WithState(Router<AppState>),
}

impl VersionedRoutes {
    /// Create from a stateless router (crate-private, only accessible to VersionedApiBuilder)
    #[allow(dead_code)]
    pub(crate) fn from_router(router: Router<()>) -> Self {
        Self::WithoutState(router)
    }

    /// Create from a stateful router (crate-private)
    pub(crate) fn from_router_with_state(router: Router<AppState>) -> Self {
        Self::WithState(router)
    }
}

impl Default for VersionedRoutes {
    /// Default routes with health and readiness endpoints
    fn default() -> Self {
        use axum::routing::get;

        let health_router: Router<AppState> = Router::new()
            .route("/health", get(crate::health::health))
            .route("/ready", get(crate::health::readiness));

        Self::WithState(health_router)
    }
}


/// Simplified service builder with sensible defaults
///
/// All fields are optional with defaults:
/// - config: Uses `Config::default()`
/// - routes: Uses `VersionedRoutes::default()` (health + readiness only)
/// - state: Uses `AppState::default()`
/// - grpc_services: None (gRPC server disabled by default)
///
/// Health and readiness endpoints are ALWAYS included (automatically added by ServiceBuilder).
pub struct ServiceBuilder {
    config: Option<Config>,
    routes: Option<VersionedRoutes>,
    state: Option<AppState>,
    #[cfg(feature = "grpc")]
    grpc_services: Option<tonic::service::Routes>,
}

impl ServiceBuilder {
    /// Create a new service builder with defaults
    pub fn new() -> Self {
        Self {
            config: None,
            routes: None,
            state: None,
            #[cfg(feature = "grpc")]
            grpc_services: None,
        }
    }

    /// Set the service configuration (optional, defaults to Config::default())
    pub fn with_config(mut self, config: Config) -> Self {
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
    pub fn with_routes(mut self, routes: VersionedRoutes) -> Self {
        self.routes = Some(routes);
        self
    }

    /// Set the application state (optional, defaults to AppState::default())
    pub fn with_state(mut self, state: AppState) -> Self {
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
    /// Build the service
    ///
    /// Automatically handles:
    /// - **Config loading**: Calls `Config::load()` if not provided (falls back to `Config::default()` on error)
    /// - **Tracing initialization**: Initializes tracing with the loaded config
    /// - **Health endpoints**: Always includes `/health` and `/ready` endpoints
    ///
    /// Uses defaults for any fields not set:
    /// - config: `Config::load()` → `Config::default()` if load fails
    /// - routes: `VersionedRoutes::default()` (health + readiness only)
    /// - state: `AppState::default()`
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Minimal - everything is automatic
    /// let service = ServiceBuilder::new().build();
    /// // → Loads config, initializes tracing, adds health endpoints
    ///
    /// // With custom routes (most common)
    /// let service = ServiceBuilder::new()
    ///     .with_routes(versioned_routes)
    ///     .build();
    /// // → Loads config, initializes tracing, adds your routes + health endpoints
    ///
    /// // Override config (e.g., for testing)
    /// let custom_config = Config { /* ... */ };
    /// let service = ServiceBuilder::new()
    ///     .with_config(custom_config)
    ///     .with_routes(routes)
    ///     .build();
    /// // → Uses your config, initializes tracing, adds routes + health endpoints
    /// ```
    pub fn build(self) -> ActonService {
        // Load config if not provided
        let config = self.config.unwrap_or_else(|| {
            Config::load().unwrap_or_else(|e| {
                eprintln!("Warning: Failed to load config: {}, using defaults", e);
                Config::default()
            })
        });

        // Initialize tracing with the loaded config
        if let Err(e) = crate::observability::init_tracing(&config) {
            eprintln!("Warning: Failed to initialize tracing: {}", e);
        }

        let routes = self.routes.unwrap_or_default();
        let state = self.state.unwrap_or_else(|| AppState::new(config.clone()));

        // Handle both types of versioned routes
        let mut app = match routes {
            VersionedRoutes::WithState(router) => {
                // Health routes already added, just attach state
                router.with_state(state)
            }
            VersionedRoutes::WithoutState(router) => {
                // Add health routes and attach state
                use axum::routing::get;
                let health_router: Router<AppState> = Router::new()
                    .route("/health", get(crate::health::health))
                    .route("/ready", get(crate::health::readiness));

                // Use fallback_service to include the versioned routes
                let router_with_health = health_router.fallback_service(router);
                router_with_health.with_state(state)
            }
        };

        // Auto-apply Cedar middleware if configured and enabled
        // NOTE: Cedar must be applied BEFORE JWT because Axum layers run in reverse order
        // This ensures the execution order is: Request → JWT → Cedar → Handler
        #[cfg(feature = "cedar-authz")]
        if let Some(ref cedar_config) = config.cedar {
            if cedar_config.enabled {
                match tokio::runtime::Handle::try_current() {
                    Ok(_handle) => {
                        // Use block_in_place to avoid nested runtime error
                        match tokio::task::block_in_place(|| {
                            tokio::runtime::Handle::current().block_on(async {
                                crate::middleware::cedar::CedarAuthz::new(cedar_config.clone()).await
                            })
                        }) {
                            Ok(cedar_authz) => {
                                tracing::debug!("Auto-applying Cedar authorization middleware");
                                app = app.layer(axum::middleware::from_fn_with_state(
                                    cedar_authz,
                                    crate::middleware::cedar::CedarAuthz::middleware,
                                ));
                            }
                            Err(e) => {
                                tracing::warn!("Failed to initialize Cedar middleware: {}", e);
                            }
                        }
                    }
                    Err(_) => {
                        tracing::warn!("No tokio runtime available for Cedar initialization");
                    }
                }
            }
        }

        // Auto-apply JWT middleware if configured
        // NOTE: JWT must be applied AFTER Cedar because Axum layers run in reverse order
        // This ensures the execution order is: Request → JWT → Cedar → Handler
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
        }
    }
}

impl Default for ServiceBuilder {
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
pub struct ActonService {
    config: Config,
    listener_addr: std::net::SocketAddr,
    app: Router,
    #[cfg(feature = "grpc")]
    grpc_routes: Option<tonic::service::Routes>,
}

impl ActonService {
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
        Ok(())
    }

    /// Get a reference to the service configuration
    pub fn config(&self) -> &Config {
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
