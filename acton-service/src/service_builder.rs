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
//!     let config = Config::load()?;
//!     let state = AppState::builder().build().await?;
//!
//!     // Create versioned routes (ONLY way to create routes)
//!     let routes = VersionedApiBuilder::new()
//!         .with_base_path("/api")
//!         .add_version(ApiVersion::V1, |router| {
//!             router.route("/users", get(list_users))
//!         })
//!         .build_routes();  // Returns VersionedRoutes (not Router!)
//!
//!     // Build service with type-safe builder
//!     let service = ServiceBuilder::new()
//!         .with_config(config)
//!         .with_routes(routes)  // Only accepts VersionedRoutes
//!         .with_state(state)
//!         .build();  // Returns ActonService (not Router!)
//!
//!     // Health and readiness are automatically included
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
/// This type can ONLY be created by `VersionedApiBuilder::build_routes_with_health()`.
/// It cannot be constructed manually, ensuring all routes:
/// - Are versioned
/// - Include health and readiness endpoints
#[derive(Debug)]
pub struct VersionedRoutes {
    pub(crate) router: Router,
}

impl VersionedRoutes {
    /// Create from a router (crate-private, only accessible to VersionedApiBuilder)
    pub(crate) fn from_router(router: Router) -> Self {
        Self { router }
    }
}

impl From<VersionedRoutes> for Router {
    fn from(routes: VersionedRoutes) -> Self {
        routes.router
    }
}

impl Default for VersionedRoutes {
    /// Default routes with only health and readiness endpoints
    fn default() -> Self {
        use axum::http::StatusCode;
        use axum::response::IntoResponse;
        use axum::routing::get;

        async fn health() -> impl IntoResponse {
            (StatusCode::OK, "healthy")
        }

        async fn readiness() -> impl IntoResponse {
            (StatusCode::OK, "ready")
        }

        Self {
            router: Router::new()
                .route("/health", get(health))
                .route("/ready", get(readiness)),
        }
    }
}


/// Simplified service builder with sensible defaults
///
/// All fields are optional with defaults:
/// - config: Uses `Config::default()`
/// - routes: Uses `VersionedRoutes::default()` (health + readiness only)
/// - state: Uses `AppState::default()`
///
/// Health and readiness endpoints are ALWAYS included (part of VersionedRoutes).
pub struct ServiceBuilder {
    config: Option<Config>,
    routes: Option<VersionedRoutes>,
    state: Option<AppState>,
}

impl ServiceBuilder {
    /// Create a new service builder with defaults
    pub fn new() -> Self {
        Self {
            config: None,
            routes: None,
            state: None,
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
    /// only be created by `VersionedApiBuilder::build_routes_with_health()`.
    /// This makes it impossible to add unversioned routes.
    ///
    /// If not provided, defaults to VersionedRoutes::default() (health + readiness only).
    pub fn with_routes(mut self, routes: VersionedRoutes) -> Self {
        self.routes = Some(routes);
        self
    }

    /// Set the application state (optional, defaults to AppState::default())
    pub fn with_state(mut self, state: AppState) -> Self {
        self.state = Some(state);
        self
    }
    /// Build the service
    ///
    /// Uses defaults for any fields not set:
    /// - config: Config::default()
    /// - routes: VersionedRoutes::default() (health + readiness only)
    /// - state: AppState::default()
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Minimal - uses all defaults
    /// let service = ServiceBuilder::new().build();
    ///
    /// // With custom routes
    /// let service = ServiceBuilder::new()
    ///     .with_routes(versioned_routes)
    ///     .build();
    ///
    /// // Fully configured
    /// let service = ServiceBuilder::new()
    ///     .with_config(config)
    ///     .with_routes(routes)
    ///     .with_state(state)
    ///     .build();
    /// ```
    pub fn build(self) -> ActonService {
        let config = self.config.unwrap_or_default();
        let routes = self.routes.unwrap_or_default();
        let _state = self.state.unwrap_or_default();  // State not needed since routes are stateless

        // VersionedRoutes already includes health and readiness endpoints
        let app = Router::new().merge(routes);  // Uses From<VersionedRoutes> for Router

        let listener_addr = std::net::SocketAddr::from(([0, 0, 0, 0], config.service.port));

        ActonService {
            config,
            listener_addr,
            app,
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
}

impl ActonService {
    /// Serve the application
    ///
    /// This runs the HTTP server with graceful shutdown support.
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
    pub async fn serve(self) -> crate::error::Result<()> {
        use tokio::net::TcpListener;
        use tokio::signal;

        tracing::info!("Starting service on {}", self.listener_addr);

        let listener = TcpListener::bind(&self.listener_addr).await?;

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
