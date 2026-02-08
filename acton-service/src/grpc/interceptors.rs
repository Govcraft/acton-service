//! gRPC interceptors for cross-cutting concerns
//!
//! Interceptors provide similar functionality to HTTP middleware,
//! allowing request/response inspection and modification.

use std::sync::Arc;
use tonic::{Request, Response, Status};

use crate::ids::RequestId;
use crate::middleware::{PasetoAuth, TokenValidator};

#[cfg(feature = "jwt")]
use crate::middleware::JwtAuth;

/// Request ID interceptor
///
/// Extracts or generates a request ID and adds it to request metadata.
/// This enables distributed tracing across gRPC calls.
///
/// The request ID is propagated in both request and response metadata.
pub fn request_id_interceptor<T>(mut req: Request<T>) -> Result<Request<T>, Status> {
    // Try to get existing request ID from metadata, or generate a new one
    let request_id = req
        .metadata()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| RequestId::new().to_string());

    // Insert/update request ID in metadata
    req.metadata_mut().insert(
        "x-request-id",
        request_id
            .parse()
            .map_err(|_| Status::internal("Failed to parse request ID"))?,
    );

    // Store request ID in extensions for response propagation
    req.extensions_mut().insert(RequestIdExtension(request_id));

    Ok(req)
}

/// Extension to store request ID for response propagation
#[derive(Clone, Debug)]
pub struct RequestIdExtension(pub String);

/// Response interceptor to propagate request ID in response metadata
///
/// This should be used in a Tower middleware layer to add request ID to responses.
pub fn add_request_id_to_response<B>(
    response: &mut Response<B>,
    request_id: &str,
) -> Result<(), Status> {
    response.metadata_mut().insert(
        "x-request-id",
        request_id
            .parse()
            .map_err(|_| Status::internal("Failed to add request ID to response"))?,
    );
    Ok(())
}

/// Generic token authentication interceptor factory
///
/// Creates an interceptor that validates tokens from metadata using any TokenValidator.
///
/// # Example
/// ```ignore
/// use acton_service::grpc::interceptors::token_auth_interceptor;
/// use acton_service::middleware::PasetoAuth;
/// use std::sync::Arc;
///
/// let paseto_auth = Arc::new(PasetoAuth::new(&config.token.unwrap())?);
/// let interceptor = token_auth_interceptor(paseto_auth);
///
/// let service = MyServiceServer::with_interceptor(service_impl, interceptor);
/// ```
pub fn token_auth_interceptor<V: TokenValidator + 'static>(
    validator: Arc<V>,
) -> impl Fn(Request<()>) -> Result<Request<()>, Status> + Clone {
    move |mut req: Request<()>| {
        // Extract authorization token from metadata
        let token = req
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .ok_or_else(|| Status::unauthenticated("Missing or invalid authorization token"))?;

        // Validate token and extract claims
        let claims = validator
            .validate_token(token)
            .map_err(|e| Status::unauthenticated(format!("Invalid token: {}", e)))?;

        tracing::debug!(
            sub = %claims.sub,
            roles = ?claims.roles,
            "gRPC request authenticated"
        );

        // Add claims to request extensions for use in handlers
        req.extensions_mut().insert(claims);

        Ok(req)
    }
}

/// PASETO authentication interceptor factory (default)
///
/// Creates an interceptor that validates PASETO tokens from metadata.
///
/// # Example
/// ```ignore
/// use acton_service::grpc::interceptors::paseto_auth_interceptor;
/// use acton_service::middleware::PasetoAuth;
/// use std::sync::Arc;
///
/// let paseto_auth = Arc::new(PasetoAuth::new(&paseto_config)?);
/// let interceptor = paseto_auth_interceptor(paseto_auth);
///
/// let service = MyServiceServer::with_interceptor(service_impl, interceptor);
/// ```
pub fn paseto_auth_interceptor(
    paseto_auth: Arc<PasetoAuth>,
) -> impl Fn(Request<()>) -> Result<Request<()>, Status> + Clone {
    token_auth_interceptor(paseto_auth)
}

/// JWT authentication interceptor factory (requires `jwt` feature)
///
/// Creates an interceptor that validates JWT tokens from metadata.
///
/// # Example
/// ```ignore
/// use acton_service::grpc::interceptors::jwt_auth_interceptor;
/// use acton_service::middleware::JwtAuth;
/// use std::sync::Arc;
///
/// let jwt_auth = Arc::new(JwtAuth::new(&jwt_config)?);
/// let interceptor = jwt_auth_interceptor(jwt_auth);
///
/// let service = MyServiceServer::with_interceptor(service_impl, interceptor);
/// ```
#[cfg(feature = "jwt")]
pub fn jwt_auth_interceptor(
    jwt_auth: Arc<JwtAuth>,
) -> impl Fn(Request<()>) -> Result<Request<()>, Status> + Clone {
    token_auth_interceptor(jwt_auth)
}

/// Simple authentication interceptor (deprecated - use token_auth_interceptor instead)
///
/// This is kept for backward compatibility but doesn't perform actual validation.
#[deprecated(
    since = "0.2.0",
    note = "Use paseto_auth_interceptor or jwt_auth_interceptor instead"
)]
pub fn auth_interceptor<T>(req: Request<T>) -> Result<Request<T>, Status> {
    // Extract authorization token from metadata
    let _token = req
        .metadata()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or_else(|| Status::unauthenticated("Missing authentication token"))?;

    // Note: This does not validate the token
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
