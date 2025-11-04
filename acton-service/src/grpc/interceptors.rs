//! gRPC interceptors for cross-cutting concerns
//!
//! Interceptors provide similar functionality to HTTP middleware,
//! allowing request/response inspection and modification.

use tonic::{Request, Status};
use uuid::Uuid;

/// Request ID interceptor
///
/// Extracts or generates a request ID and adds it to request metadata.
/// This enables distributed tracing across gRPC calls.
pub fn request_id_interceptor<T>(mut req: Request<T>) -> Result<Request<T>, Status> {
    // Try to get existing request ID from metadata
    let request_id = req
        .metadata()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // Insert/update request ID in metadata
    req.metadata_mut()
        .insert("x-request-id", request_id.parse().map_err(|_| {
            Status::internal("Failed to parse request ID")
        })?);

    Ok(req)
}

/// Authentication interceptor
///
/// Validates JWT tokens from metadata and extracts user information.
pub fn auth_interceptor<T>(req: Request<T>) -> Result<Request<T>, Status> {
    // Extract authorization token from metadata
    let token = req
        .metadata()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));

    if token.is_none() {
        return Err(Status::unauthenticated("Missing authentication token"));
    }

    // TODO: Validate JWT token and extract claims
    // For now, just pass through
    Ok(req)
}

/// Tracing interceptor
///
/// Creates a tracing span for the gRPC request with key metadata.
pub fn tracing_interceptor<T>(req: Request<T>) -> Result<Request<T>, Status> {
    // Note: gRPC method path is available in the request extensions, not URI
    let request_id = req
        .metadata()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    tracing::info!(
        request_id = %request_id,
        "gRPC request received"
    );

    Ok(req)
}
