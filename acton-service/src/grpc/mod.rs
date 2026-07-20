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
//! For manual composition, the module also provides:
//! - **Request ID**: Automatic generation and propagation
//! - **Tracing**: OpenTelemetry integration with proper span context
//! - **Authentication**: [`GrpcTokenAuthLayer`] as an HTTP-level tower
//!   layer, or PASETO/JWT interceptors for use with `with_interceptor`
//! - **Rate Limiting**: Governor-based rate limiting (when `governor` feature is enabled)
//!
//! ## Example
//!
//! ```ignore
//! use acton_service::grpc::interceptors::{request_id_interceptor, paseto_auth_interceptor};
//! use acton_service::grpc::middleware::GrpcTracingLayer;
//! use acton_service::middleware::PasetoAuth;
//! use std::sync::Arc;
//! use tonic::transport::Server;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create PASETO auth
//! let paseto_auth = Arc::new(PasetoAuth::new(&config.token.unwrap())?);
//!
//! // Build gRPC service with interceptors
//! let service = MyServiceServer::with_interceptor(
//!     service_impl,
//!     move |req| {
//!         let req = request_id_interceptor(req)?;
//!         paseto_auth_interceptor(paseto_auth.clone())(req)
//!     }
//! );
//!
//! // Serve
//! Server::builder()
//!     .layer(GrpcTracingLayer)
//!     .add_service(service)
//!     .serve(addr)
//!     .await?;
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
