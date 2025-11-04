//! gRPC server support for acton-service
//!
//! This module provides gRPC server functionality that can run alongside HTTP services.
//! It supports both single-port (HTTP + gRPC multiplexed) and dual-port modes.
//!
//! ## Middleware and Interceptors
//!
//! The gRPC implementation provides middleware parity with HTTP:
//! - **Request ID**: Automatic generation and propagation
//! - **Tracing**: OpenTelemetry integration with proper span context
//! - **Authentication**: JWT token validation via interceptors
//! - **Rate Limiting**: Governor-based rate limiting (when `governor` feature is enabled)
//!
//! ## Example
//!
//! ```ignore
//! use acton_service::grpc::interceptors::{request_id_interceptor, jwt_auth_interceptor};
//! use acton_service::grpc::middleware::GrpcTracingLayer;
//! use acton_service::middleware::JwtAuth;
//! use std::sync::Arc;
//! use tonic::transport::Server;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create JWT auth
//! let jwt_auth = Arc::new(JwtAuth::new(&config.jwt)?);
//!
//! // Build gRPC service with interceptors
//! let service = MyServiceServer::with_interceptor(
//!     service_impl,
//!     move |req| {
//!         let req = request_id_interceptor(req)?;
//!         jwt_auth_interceptor(jwt_auth.clone())(req)
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
    request_id_interceptor, jwt_auth_interceptor, RequestIdExtension,
    add_request_id_to_response,
};

#[cfg(feature = "grpc")]
pub use middleware::{
    GrpcTracingLayer, GrpcTracingService,
    LoggingLayer, LoggingService,
};

#[cfg(all(feature = "grpc", feature = "governor"))]
pub use middleware::{
    GrpcRateLimitLayer, GrpcRateLimitService,
};

// Re-export tonic types for convenience
#[cfg(feature = "grpc")]
pub use tonic::{Request, Response, Status, Code};
