//! # acton-service
//!
//! Production-ready Rust microservice framework with dual-protocol support (HTTP + gRPC).
//!
//! ## Features
//!
//! - **Dual-protocol**: HTTP (axum) + gRPC (tonic) on single port
//! - **Middleware stack**: JWT auth, rate limiting, request tracking, panic recovery, body size limits
//! - **Resilience**: Circuit breaker, retry with backoff, bulkhead (concurrency limiting)
//! - **Observability**: OpenTelemetry tracing, HTTP metrics, request ID propagation
//! - **Connection pooling**: Database (YSQL), Redis, NATS JetStream
//! - **Health checks**: Liveness and readiness probes
//! - **Graceful shutdown**: Proper signal handling (SIGTERM, SIGINT)
//!
//! ## Example
//!
//! ```rust,no_run
//! use acton_service::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Load configuration
//!     let config = Config::load()?;
//!
//!     // Initialize tracing
//!     init_tracing(&config)?;
//!
//!     // Build application state
//!     let state = AppState::builder()
//!         .config(config.clone())
//!         .build()
//!         .await?;
//!
//!     // Create router
//!     let app = Router::new()
//!         .route("/health", get(health))
//!         .route("/ready", get(readiness))
//!         .with_state(state);
//!
//!     // Run server
//!     Server::new(config)
//!         .serve(app)
//!         .await?;
//!
//!     Ok(())
//! }
//! ```

pub mod config;
pub mod error;
pub mod middleware;
pub mod health;
pub mod responses;
pub mod server;
pub mod service_builder;
pub mod state;
pub mod versioning;

#[cfg(feature = "database")]
pub mod database;

#[cfg(feature = "cache")]
pub mod cache;

#[cfg(feature = "events")]
pub mod events;

#[cfg(feature = "observability")]
pub mod observability;

#[cfg(feature = "openapi")]
pub mod openapi;

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::config::Config;
    pub use crate::error::{Error, Result};
    pub use crate::health::{health, readiness};
    pub use crate::middleware::{
        Claims, JwtAuth, RateLimit, RequestTrackingConfig, PROPAGATE_HEADERS, SENSITIVE_HEADERS,
        request_id_layer, request_id_propagation_layer, sensitive_headers_layer,
    };
    pub use crate::server::Server;
    pub use crate::service_builder::{ActonService, ServiceBuilder, VersionedRoutes};
    pub use crate::state::{AppState, AppStateBuilder};
    pub use crate::versioning::{
        ApiVersion, DeprecationInfo, VersionedApiBuilder, VersionedResponse,
        extract_version_from_path, versioned_router,
    };
    pub use crate::responses::{
        Accepted, Conflict, Created, FieldError, NoContent, Success, ValidationError,
    };

    #[cfg(feature = "resilience")]
    pub use crate::middleware::ResilienceConfig;

    #[cfg(feature = "otel-metrics")]
    pub use crate::middleware::{MetricsConfig, metric_labels, metric_names};

    #[cfg(feature = "governor")]
    pub use crate::middleware::{GovernorConfig, RateLimitExceeded};

    #[cfg(feature = "observability")]
    pub use crate::observability::init_tracing;

    #[cfg(feature = "openapi")]
    pub use crate::openapi::{OpenApiBuilder, RapiDoc, ReDoc, SwaggerUI};

    pub use axum::{
        extract::{Path, Query, State},
        http::StatusCode,
        response::{IntoResponse, Json},
        routing::{delete, get, patch, post, put},
        Extension, Router,
    };

    pub use serde::{Deserialize, Serialize};
    pub use tracing::{debug, error, info, warn};
}
