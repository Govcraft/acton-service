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
pub mod ids;
pub mod middleware;
pub mod health;
pub mod pool_health;
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

#[cfg(feature = "grpc")]
pub mod grpc;

/// Internal agent-based components
///
/// Connection pool management is handled internally by agents. Users don't
/// need to interact with this module directly - just use `ServiceBuilder::build()`
/// and access pools via `state.db()`, `state.redis()`, etc.
///
/// The only user-facing types from this module are:
/// - [`BackgroundWorker`](agents::BackgroundWorker) - for managed background tasks
/// - [`TaskStatus`](agents::TaskStatus) - for checking background task status
/// - [`HealthStatus`](agents::HealthStatus) - for health check results
pub mod agents;

/// Build-time utilities for compiling protocol buffers
///
/// These are used in `build.rs` scripts, not at runtime.
pub mod build_utils;

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::config::Config;

    #[cfg(feature = "cedar-authz")]
    pub use crate::config::CedarConfig;

    pub use crate::error::{Error, Result};
    pub use crate::health::{health, readiness, pool_metrics};
    pub use crate::ids::{MakeTypedRequestId, RequestId, RequestIdError};
    pub use crate::pool_health::PoolHealthSummary;

    #[cfg(feature = "database")]
    pub use crate::pool_health::DatabasePoolHealth;

    #[cfg(feature = "cache")]
    pub use crate::pool_health::RedisPoolHealth;

    #[cfg(feature = "events")]
    pub use crate::pool_health::NatsClientHealth;
    pub use crate::middleware::{
        Claims, JwtAuth, RateLimit, RequestTrackingConfig, PROPAGATE_HEADERS, SENSITIVE_HEADERS,
        request_id_layer, request_id_propagation_layer, sensitive_headers_layer,
    };

    #[cfg(feature = "cache")]
    pub use crate::middleware::{JwtRevocation, RedisJwtRevocation};
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

    #[cfg(feature = "cedar-authz")]
    pub use crate::middleware::CedarAuthz;

    #[cfg(all(feature = "cedar-authz", feature = "cache"))]
    pub use crate::middleware::{PolicyCache, RedisPolicyCache};

    #[cfg(all(feature = "cedar-authz", feature = "grpc"))]
    pub use crate::middleware::{CedarAuthzLayer, CedarAuthzService};

    #[cfg(feature = "observability")]
    pub use crate::observability::init_tracing;

    #[cfg(feature = "openapi")]
    pub use crate::openapi::{OpenApiBuilder, RapiDoc, ReDoc, SwaggerUI};

    #[cfg(feature = "grpc")]
    pub use crate::grpc::{
        GrpcServer, HealthService, Request,
        Response as GrpcResponse,
        Status, Code,
        request_id_interceptor, jwt_auth_interceptor, RequestIdExtension,
        add_request_id_to_response, GrpcTracingLayer, LoggingLayer,
    };

    #[cfg(all(feature = "grpc", feature = "governor"))]
    pub use crate::grpc::GrpcRateLimitLayer;

    // Background task management (user-facing)
    pub use crate::agents::{BackgroundWorker, TaskStatus};

    // Health status types (for checking aggregated health)
    pub use crate::agents::{AggregatedHealthResponse, HealthStatus};

    pub use axum::{
        extract::{Path, Query, State},
        http::{HeaderMap, HeaderValue, StatusCode},
        response::{IntoResponse, Json, Response},
        routing::{delete, get, patch, post, put},
        Extension, Router,
    };

    pub use serde::{Deserialize, Serialize};

    // Re-export tracing macros and types
    pub use tracing::{debug, error, info, instrument, trace, warn, Level, Span};

    // Re-export tokio for async runtime
    pub use tokio;

    // Re-export async-trait for async trait definitions
    pub use async_trait::async_trait;

    // Re-export error handling utilities
    pub use thiserror::Error;
    pub use anyhow::{self, Context as AnyhowContext};

    // Re-export time utilities
    pub use chrono::{DateTime, Duration as ChronoDuration, NaiveDateTime, Utc};

    // Re-export UUID
    pub use uuid::Uuid;

    // Re-export futures utilities
    pub use futures::{future, stream, Future, Stream, StreamExt, TryFutureExt, TryStreamExt};

    // Re-export HTTP types
    pub use http::{Method, Uri};

    // Re-export acton-reactive prelude for actor system
    pub use acton_reactive::prelude::*;
}
