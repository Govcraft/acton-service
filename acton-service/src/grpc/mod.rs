//! gRPC server support for acton-service
//!
//! This module provides gRPC server functionality that can run alongside HTTP services.
//! It supports both single-port (HTTP + gRPC multiplexed) and dual-port modes.
//!
//! ## Authentication and Authorization
//!
//! When `[token]` is configured, `ServiceBuilder` automatically applies
//! token authentication ([`GrpcTokenAuthLayer`]) to all registered gRPC
//! services, validating the `authorization` metadata and injecting
//! [`Claims`](crate::middleware::token::Claims) into request extensions.
//! With the `cedar-authz` feature and `[cedar]` enabled, each method is
//! additionally authorized against Cedar policies as
//! `Action::"/package.Service/Method"`. Health and reflection services are
//! exempt, as are configured `public_paths` prefixes. See the `cedar-grpc`
//! example for an end-to-end demonstration.
//!
//! ## Middleware and Interceptors
//!
//! For manual composition, the module also provides HTTP-level tower layers
//! (each with a forwarding `NamedService` impl, so wrapped services register
//! with [`GrpcServicesBuilder::add_service`](server::GrpcServicesBuilder::add_service)):
//! - **Logging**: [`LoggingLayer`] logs method, duration, and status
//! - **Tracing**: [`GrpcTracingLayer`] creates OpenTelemetry-compatible spans
//! - **Authentication**: [`GrpcTokenAuthLayer`], or PASETO/JWT interceptors
//!   for use with `with_interceptor`
//! - **Rate Limiting**: [`GrpcRateLimitLayer`] token bucket limiting (when
//!   the `governor` feature is enabled)
//!
//! ## Example
//!
//! ```ignore
//! use acton_service::grpc::middleware::{GrpcTokenAuthLayer, GrpcTracingLayer, LoggingLayer};
//! use acton_service::grpc::server::GrpcServicesBuilder;
//! use acton_service::middleware::PasetoAuth;
//! use tower::Layer;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let paseto_auth = PasetoAuth::new(&config.token.unwrap())?;
//!
//! // Outermost layer first: tracing -> auth -> service
//! let service = GrpcTracingLayer.layer(
//!     GrpcTokenAuthLayer::new(paseto_auth).layer(MyServiceServer::new(service_impl)),
//! );
//!
//! let grpc_routes = GrpcServicesBuilder::new()
//!     .add_service(service)
//!     .build(None);
//! # Ok(())
//! # }
//! ```

#[cfg(feature = "grpc")]
pub mod server;

#[cfg(feature = "grpc")]
pub mod interceptors;

#[cfg(feature = "grpc")]
pub mod middleware;

#[cfg(feature = "grpc")]
pub mod health;

// Re-exports
#[cfg(feature = "grpc")]
pub use server::GrpcServer;

#[cfg(feature = "grpc")]
pub use health::HealthService;

#[cfg(feature = "grpc")]
pub use interceptors::{
    add_request_id_to_response, paseto_auth_interceptor, request_id_interceptor,
    token_auth_interceptor, RequestIdExtension,
};

#[cfg(all(feature = "grpc", feature = "jwt"))]
pub use interceptors::jwt_auth_interceptor;

#[cfg(feature = "grpc")]
pub use middleware::{
    GrpcTokenAuthLayer, GrpcTokenAuthService, GrpcTracingLayer, GrpcTracingService, LoggingLayer,
    LoggingService,
};

#[cfg(all(feature = "grpc", feature = "governor"))]
pub use middleware::{GrpcRateLimitLayer, GrpcRateLimitService};

// Re-export tonic types for convenience
#[cfg(feature = "grpc")]
pub use tonic::{Code, Request, Response, Status};
