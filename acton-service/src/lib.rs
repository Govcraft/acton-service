//! # acton-service
//!
//! Production-ready Rust backend framework with multi-protocol support (HTTP + gRPC + WebSocket).
//! Works equally well for monolithic applications and microservices architectures.
//!
//! ## Features
//!
//! - **Multi-protocol**: HTTP (axum) + gRPC (tonic) + WebSocket on single port
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

// Ensure database backends are mutually exclusive
#[cfg(all(feature = "database", feature = "turso"))]
compile_error!(
    "Features `database` (PostgreSQL) and `turso` (libsql) are mutually exclusive. \
     Enable only one database backend."
);

#[cfg(all(feature = "database", feature = "surrealdb"))]
compile_error!(
    "Features `database` (PostgreSQL) and `surrealdb` are mutually exclusive. \
     Enable only one database backend."
);

#[cfg(all(feature = "turso", feature = "surrealdb"))]
compile_error!(
    "Features `turso` (libsql) and `surrealdb` are mutually exclusive. \
     Enable only one database backend."
);

pub mod config;
pub mod error;
pub mod health;
pub mod ids;
pub mod middleware;
pub mod pool_health;
pub mod responses;
pub mod server;
pub mod service_builder;
pub mod state;
pub mod versioning;

#[cfg(feature = "database")]
pub mod database;

#[cfg(feature = "turso")]
pub mod turso;

#[cfg(feature = "surrealdb")]
pub mod surrealdb_backend;

#[cfg(feature = "cache")]
pub mod cache;

#[cfg(feature = "events")]
pub mod events;

#[cfg(feature = "clickhouse")]
pub mod clickhouse_backend;

pub mod observability;

#[cfg(feature = "openapi")]
pub mod openapi;

#[cfg(feature = "grpc")]
pub mod grpc;

#[cfg(feature = "websocket")]
pub mod websocket;

#[cfg(feature = "auth")]
pub mod auth;

#[cfg(feature = "session")]
pub mod session;

#[cfg(feature = "htmx")]
pub mod htmx;

#[cfg(feature = "askama")]
pub mod templates;

#[cfg(feature = "sse")]
pub mod sse;

#[cfg(feature = "pagination")]
pub mod pagination;

#[cfg(feature = "repository")]
pub mod repository;

#[cfg(feature = "handlers")]
pub mod handlers;

#[cfg(feature = "audit")]
pub mod audit;

#[cfg(feature = "tls")]
pub mod tls;

#[cfg(feature = "login-lockout")]
pub mod lockout;

#[cfg(feature = "accounts")]
pub mod accounts;

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

/// Actor-backed extensions for custom application state
///
/// Provides the [`ActorExtension`](extensions::ActorExtension) trait for registering
/// custom supervised actors via [`ServiceBuilder::with_actor`](service_builder::ServiceBuilder::with_actor).
pub mod extensions;

/// Build-time utilities for compiling protocol buffers
///
/// These are used in `build.rs` scripts, not at runtime.
pub mod build_utils;

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::config::{Config, RateLimitConfig, RouteRateLimitConfig};

    #[cfg(feature = "cedar-authz")]
    pub use crate::config::CedarConfig;

    pub use crate::error::{Error, Result};

    #[cfg(any(feature = "database", feature = "turso", feature = "surrealdb"))]
    pub use crate::error::{DatabaseError, DatabaseErrorKind, DatabaseOperation};

    pub use crate::health::{health, pool_metrics, readiness};
    pub use crate::ids::{MakeTypedRequestId, RequestId, RequestIdError};
    pub use crate::pool_health::PoolHealthSummary;

    #[cfg(feature = "database")]
    pub use crate::pool_health::DatabasePoolHealth;

    #[cfg(feature = "turso")]
    pub use crate::pool_health::TursoDbHealth;

    #[cfg(feature = "surrealdb")]
    pub use crate::pool_health::SurrealDbHealth;

    #[cfg(feature = "cache")]
    pub use crate::pool_health::RedisPoolHealth;

    pub use crate::middleware::{
        normalize_path, request_id_layer, request_id_propagation_layer, sensitive_headers_layer,
        Claims, CompiledRoutePatterns, PasetoAuth, RateLimit, RequestTrackingConfig,
        TokenValidator, PROPAGATE_HEADERS, SENSITIVE_HEADERS,
    };
    #[cfg(feature = "events")]
    pub use crate::pool_health::NatsClientHealth;

    #[cfg(feature = "clickhouse")]
    pub use crate::pool_health::ClickHouseHealth;

    #[cfg(feature = "clickhouse")]
    pub use crate::clickhouse_backend::AnalyticsWriter;

    #[cfg(feature = "cache")]
    pub use crate::middleware::{RedisTokenRevocation, TokenRevocation};

    #[cfg(feature = "jwt")]
    pub use crate::middleware::JwtAuth;
    pub use crate::responses::{
        Accepted, Conflict, Created, FieldError, NoContent, Success, ValidationError,
    };
    pub use crate::server::Server;
    pub use crate::service_builder::{ActonService, ServiceBuilder, VersionedRoutes};
    pub use crate::extensions::{ActorExtension, ActorExtensions};
    pub use crate::state::{AppState, AppStateBuilder};
    pub use crate::versioning::{
        extract_version_from_path, versioned_router, ApiVersion, DeprecationInfo,
        VersionedApiBuilder, VersionedResponse,
    };

    #[cfg(feature = "resilience")]
    pub use crate::middleware::ResilienceConfig;

    #[cfg(feature = "otel-metrics")]
    pub use crate::middleware::{metric_labels, metric_names, MetricsConfig};

    #[cfg(feature = "governor")]
    pub use crate::middleware::{GovernorConfig, GovernorRateLimit, RateLimitExceeded};

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
        add_request_id_to_response, paseto_auth_interceptor, request_id_interceptor,
        token_auth_interceptor, Code, GrpcServer, GrpcTracingLayer, HealthService, LoggingLayer,
        Request, RequestIdExtension, Response as GrpcResponse, Status,
    };

    #[cfg(all(feature = "grpc", feature = "jwt"))]
    pub use crate::grpc::jwt_auth_interceptor;

    #[cfg(all(feature = "grpc", feature = "governor"))]
    pub use crate::grpc::GrpcRateLimitLayer;

    // Auth module exports
    #[cfg(feature = "auth")]
    pub use crate::auth::{
        ApiKey, ApiKeyGenerator, AuthConfig, CachedKey, KeyFormat, KeyManager, KeyRotationConfig,
        KeyStatus, PasetoGenerationConfig, PasetoGenerator, PasswordConfig, PasswordHasher,
        RefreshTokenConfig, SigningKeyMetadata, TokenGenerationConfig, TokenGenerator, TokenPair,
    };

    // Key rotation storage trait (requires auth + a database backend)
    #[cfg(all(
        feature = "auth",
        any(feature = "database", feature = "turso", feature = "surrealdb")
    ))]
    pub use crate::auth::KeyRotationStorage;

    #[cfg(all(feature = "auth", feature = "jwt"))]
    pub use crate::auth::JwtGenerator;

    #[cfg(feature = "oauth")]
    pub use crate::auth::{
        ApiKeyConfig, OAuthConfig, OAuthProvider, OAuthProviderConfig, OAuthTokens, OAuthUserInfo,
    };

    #[cfg(feature = "websocket")]
    pub use crate::websocket::{
        BroadcastTarget,
        BroadcastToRoom,
        // Broadcasting
        Broadcaster,
        ConnectionDisconnected,
        // Connection handling
        ConnectionId,
        // Messages
        JoinRoomRequest,
        LeaveRoomRequest,
        Message as WsMessage,
        Room,
        RoomConfig,
        RoomId,
        // Room management
        RoomManager,
        RoomMember,
        // Re-exported axum types
        WebSocket,
        // Configuration
        WebSocketConfig,
        WebSocketConnection,
        WebSocketUpgrade,
    };

    #[cfg(feature = "session")]
    pub use crate::session::{
        csrf_middleware,
        AuthSession,
        CsrfConfig,
        CsrfLayer,
        CsrfMiddleware,
        // CSRF protection
        CsrfToken,
        FlashKind,
        // Flash messages
        FlashMessage,
        FlashMessages,
        SessionAuth,
        // Configuration
        SessionConfig,
        SessionData,
        SessionStorage,
        // Typed session and extractors
        TypedSession,
    };

    // Re-export tower-sessions Session type for direct use
    #[cfg(feature = "session")]
    pub use tower_sessions::Session;

    // HTMX support
    #[cfg(feature = "htmx")]
    pub use crate::htmx::{
        // Helpers
        fragment_or_full,
        is_boosted_request,
        is_htmx_request,
        // Middleware
        AutoVaryLayer,
        AutoVaryMiddleware,
        // Custom types
        HtmlFragment,
        // Extractors
        HxBoosted,
        HxCurrentUrl,
        // Event types
        HxEvent,
        HxHistoryRestoreRequest,
        // Response headers
        HxLocation,
        HxPrompt,
        HxPushUrl,
        HxRedirect,
        HxRefresh,
        HxReplaceUrl,
        HxRequest,
        HxReselect,
        HxResponseTrigger,
        HxReswap,
        HxRetarget,
        HxTarget,
        HxTrigger,
        HxTriggerEvents,
        HxTriggerName,
        OutOfBandSwap,
        SwapOption,
        TriggerTiming,
        // Vary responders
        VaryHxRequest,
        VaryHxTarget,
        VaryHxTrigger,
        VaryHxTriggerName,
    };

    // Template engine support
    #[cfg(feature = "askama")]
    pub use crate::templates::{
        // Helpers
        classes,
        pluralize,
        truncate,
        HtmlTemplate,
        RenderMode,
        // Re-export askama Template derive
        Template,
        // Core types
        TemplateContext,
    };

    // Note: Html is re-exported in the main axum block below

    // Server-Sent Events support
    #[cfg(feature = "sse")]
    pub use crate::sse::{
        htmx_close_event,
        htmx_event,
        htmx_json_event,
        htmx_oob_event,
        htmx_trigger,
        BroadcastMessage,
        BroadcastTarget as SseBroadcastTarget,
        // Connection tracking (aliased to avoid conflict with websocket types)
        ConnectionId as SseConnectionId,
        // HTMX helpers
        HtmxSwap,
        // Broadcasting (BroadcastTarget aliased to avoid conflict with websocket)
        SseBroadcaster,
        // Configuration
        SseConfig,
        SseConnection,
        // Event building
        SseEventExt,
        TypedEvent,
    };

    // Re-export axum SSE types for direct use
    #[cfg(feature = "sse")]
    pub use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};

    // Background task management (user-facing)
    pub use crate::agents::{BackgroundWorker, BackgroundWorkerConfig, TaskStatus};

    // Health status types (for checking aggregated health)
    pub use crate::agents::{AggregatedHealthResponse, HealthStatus};

    // Audit logging
    #[cfg(feature = "audit")]
    pub use crate::audit::{
        AlertConfig, AuditAlertEvent, AuditAlertHook, AuditConfig, AuditEvent, AuditEventKind,
        AuditLogger, AuditRoute, AuditSeverity, AuditSource, AuditStorage,
    };

    // Login lockout
    #[cfg(feature = "login-lockout")]
    pub use crate::lockout::{
        LockoutConfig, LockoutEvent, LockoutMiddleware, LockoutNotification, LockoutStatus,
        LoginLockout, UnlockReason,
    };

    #[cfg(all(feature = "login-lockout", feature = "audit"))]
    pub use crate::lockout::AuditLockoutNotification;

    // Account management
    #[cfg(feature = "accounts")]
    pub use crate::accounts::{
        Account, AccountError, AccountEvent, AccountId, AccountNotification, AccountService,
        AccountStatus, AccountStorage, AccountsConfig, CreateAccount, UpdateAccount,
    };

    #[cfg(all(feature = "accounts", feature = "audit"))]
    pub use crate::accounts::AuditAccountNotification;

    #[cfg(feature = "account-handlers")]
    pub use crate::accounts::handlers::account_routes;

    // =========================================================================
    // Axum Re-exports
    // =========================================================================
    // Comprehensive axum re-exports so developers don't need axum as a direct
    // dependency. If you need types not in the prelude, you can still add axum
    // to your Cargo.toml.

    // Core types
    pub use axum::{serve, Extension, Router};

    // Extractors - common types for handling request data
    pub use axum::extract::{
        ConnectInfo, // Socket address of client
        Form,        // Form data extraction
        MatchedPath, // The matched route path
        OriginalUri, // Original request URI before routing
        Path,        // URL path parameter extraction
        Query,       // Query string parameter extraction
        RawQuery,    // Raw query string
        State,       // Application state extraction
    };

    // Request type - aliased to avoid conflict with tonic::Request when gRPC is enabled
    pub use axum::extract::Request as AxumRequest;

    // HTTP types - headers, status codes, and related types
    pub use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};

    // Response types - for building HTTP responses
    pub use axum::response::{
        AppendHeaders, // Append headers to response
        ErrorResponse, // Error response type
        Html,          // HTML response wrapper
        IntoResponse,  // Trait for converting types to responses
        Json,          // JSON response wrapper
        Redirect,      // Redirect responses
        Response,      // HTTP response type
    };

    // Response building types - for complex multi-part responses (useful for HTMX)
    pub use axum::response::{IntoResponseParts, ResponseParts};

    // Routing - route handlers and method routing
    pub use axum::routing::{
        any,          // Match any HTTP method
        delete,       // DELETE method handler
        get,          // GET method handler
        on,           // Match specific method(s)
        patch,        // PATCH method handler
        post,         // POST method handler
        put,          // PUT method handler
        MethodRouter, // Type for building method-specific routes
    };

    // Middleware utilities - for writing custom middleware
    pub use axum::middleware::{from_fn, from_fn_with_state, Next};

    // Body types - for advanced request/response body handling
    pub use axum::body::{Body, Bytes};

    // Request parts - for custom extractors (FromRequestParts implementations)
    pub use axum::extract::FromRequestParts;
    pub use axum::http::request::Parts as RequestParts;

    pub use serde::{Deserialize, Serialize};

    // Re-export tracing macros and types
    pub use tracing::{debug, error, info, instrument, trace, warn, Level, Span};

    // Re-export tokio for async runtime
    pub use tokio;

    // Re-export async-trait for async trait definitions
    pub use async_trait::async_trait;

    // Re-export error handling utilities
    pub use anyhow::{self, Context as AnyhowContext};
    pub use thiserror::Error;

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

    // Pagination support (core types from paginator-rs)
    #[cfg(feature = "pagination")]
    pub use crate::pagination::{
        // Cursor pagination
        Cursor,
        CursorBuilder,
        CursorDirection,
        CursorValue,
        // Filtering
        Filter,
        FilterBuilder,
        FilterOperator,
        FilterValue,
        // Parameters
        IntoPaginationParams,
        PaginationParams,
        // Main types
        Paginator,
        PaginatorBuilder,
        PaginatorError,
        PaginatorResponse,
        PaginatorResponseMeta,
        PaginatorResult,
        PaginatorTrait,
        // Search
        SearchBuilder,
        SearchParams,
        // Sorting
        SortBuilder,
        SortDirection,
    };

    // Pagination Axum integration (extractors and responses)
    #[cfg(feature = "pagination-axum")]
    pub use crate::pagination::{
        create_link_header, PaginatedJson, PaginationQuery, PaginationQueryParams,
    };

    // Pagination SQLx integration (database pagination)
    #[cfg(feature = "pagination-sqlx")]
    pub use crate::pagination::{
        validate_field_name, PaginateQuery, PaginatedQuery, QueryBuilderExt,
    };

    // Repository traits for database CRUD abstractions
    // Note: FilterCondition, FilterOperator, FilterValue, and Pagination are re-exported
    // only when pagination feature is disabled to avoid naming conflicts.
    // When using both features, import these types directly from crate::repository.
    #[cfg(all(feature = "repository", not(feature = "pagination")))]
    pub use crate::repository::{FilterCondition, FilterOperator, FilterValue, Pagination};

    #[cfg(feature = "repository")]
    pub use crate::repository::{
        OrderDirection, RelationLoader, Repository, RepositoryError, RepositoryErrorKind,
        RepositoryOperation, RepositoryResult, SoftDeleteRepository,
    };

    // Handler traits for REST CRUD patterns
    #[cfg(feature = "handlers")]
    pub use crate::handlers::{
        ApiError, ApiErrorKind, ApiOperation, CollectionHandler, ItemResponse, ListQuery,
        ListResponse, PaginationMeta, ResponseMeta, SoftDeleteHandler, SortOrder, DEFAULT_PER_PAGE,
        MAX_PER_PAGE,
    };
}
