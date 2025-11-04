//! gRPC server support for acton-service
//!
//! This module provides gRPC server functionality that can run alongside HTTP services.
//! It supports both single-port (HTTP + gRPC multiplexed) and dual-port modes.

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

// Re-export tonic types for convenience
#[cfg(feature = "grpc")]
pub use tonic::{Request, Response, Status, Code};
