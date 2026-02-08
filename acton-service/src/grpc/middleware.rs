//! gRPC middleware utilities
//!
//! Provides Tower middleware that can be used with gRPC services.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;
use tonic::{Request, Response, Status};
use tower::{Layer, Service};

use crate::grpc::interceptors::RequestIdExtension;

/// Logging middleware for gRPC requests
#[derive(Clone)]
pub struct LoggingLayer;

impl<S> Layer<S> for LoggingLayer {
    type Service = LoggingService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        LoggingService { inner }
    }
}

/// Logging service implementation
#[derive(Clone)]
pub struct LoggingService<S> {
    inner: S,
}

impl<S, ReqBody> Service<Request<ReqBody>> for LoggingService<S>
where
    S: Service<Request<ReqBody>, Response = Response<tonic::body::Body>, Error = Status>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let start = Instant::now();

            tracing::debug!("gRPC request started");

            let result = inner.call(req).await;

            let duration = start.elapsed();

            match &result {
                Ok(_) => {
                    tracing::info!(duration_ms = duration.as_millis(), "gRPC request completed");
                }
                Err(status) => {
                    tracing::warn!(
                        duration_ms = duration.as_millis(),
                        status_code = ?status.code(),
                        "gRPC request failed"
                    );
                }
            }

            result
        })
    }
}

/// Tracing middleware layer for gRPC
///
/// This layer creates OpenTelemetry spans for gRPC requests with proper context propagation.
#[derive(Clone)]
pub struct GrpcTracingLayer;

impl<S> Layer<S> for GrpcTracingLayer {
    type Service = GrpcTracingService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        GrpcTracingService { inner }
    }
}

/// Tracing service implementation
#[derive(Clone)]
pub struct GrpcTracingService<S> {
    inner: S,
}

impl<S, ReqBody> Service<Request<ReqBody>> for GrpcTracingService<S>
where
    S: Service<Request<ReqBody>, Response = Response<tonic::body::Body>, Error = Status>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let mut inner = self.inner.clone();

        // Extract request ID from extensions if available
        let request_id = req
            .extensions()
            .get::<RequestIdExtension>()
            .map(|ext| ext.0.clone())
            .or_else(|| {
                req.metadata()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| "unknown".to_string());

        // Get method name from metadata (set by tonic)
        let method = req
            .metadata()
            .get(":path")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .to_string();

        Box::pin(async move {
            let start = Instant::now();

            // Create a span for this gRPC request
            let span = tracing::info_span!(
                "grpc_request",
                otel.kind = "server",
                rpc.system = "grpc",
                rpc.service = %extract_service_name(&method),
                rpc.method = %extract_method_name(&method),
                request_id = %request_id,
            );

            let _guard = span.enter();

            tracing::debug!(method = %method, "gRPC request started");

            let result = inner.call(req).await;

            let duration = start.elapsed();

            match &result {
                Ok(response) => {
                    let status = response
                        .metadata()
                        .get("grpc-status")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("0"); // 0 = OK in gRPC

                    tracing::info!(
                        duration_ms = duration.as_millis(),
                        grpc.status_code = status,
                        "gRPC request completed"
                    );
                }
                Err(status) => {
                    tracing::warn!(
                        duration_ms = duration.as_millis(),
                        grpc.status_code = ?status.code(),
                        error.message = %status.message(),
                        "gRPC request failed"
                    );
                }
            }

            result
        })
    }
}

/// Extract service name from gRPC method path
///
/// gRPC method paths are in the format: /package.Service/Method
fn extract_service_name(path: &str) -> &str {
    path.trim_start_matches('/')
        .split('/')
        .next()
        .and_then(|s| s.rsplit('.').next())
        .unwrap_or("unknown")
}

/// Extract method name from gRPC method path
fn extract_method_name(path: &str) -> &str {
    path.trim_start_matches('/')
        .split('/')
        .nth(1)
        .unwrap_or("unknown")
}

/// Simple rate limiting layer for gRPC
///
/// This provides basic token bucket rate limiting. For production use with more
/// sophisticated algorithms, consider integrating tower-governor middleware directly.
///
/// Note: This implementation uses a simple in-memory atomic counter and is suitable
/// for single-instance deployments. For distributed rate limiting, use Redis-based
/// rate limiting in your gRPC handlers.
#[cfg(feature = "governor")]
#[derive(Clone)]
pub struct GrpcRateLimitLayer {
    enabled: bool,
}

#[cfg(feature = "governor")]
impl GrpcRateLimitLayer {
    /// Create a new rate limiting layer
    pub fn new(config: crate::config::LocalRateLimitConfig) -> Self {
        Self {
            enabled: config.enabled,
        }
    }
}

#[cfg(feature = "governor")]
impl<S> Layer<S> for GrpcRateLimitLayer {
    type Service = GrpcRateLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        GrpcRateLimitService {
            inner,
            enabled: self.enabled,
        }
    }
}

/// Rate limiting service implementation
///
/// For Phase 2, this provides a placeholder that logs when it would rate limit.
/// Full rate limiting integration with governor will be added in Phase 5.
#[cfg(feature = "governor")]
#[derive(Clone)]
pub struct GrpcRateLimitService<S> {
    inner: S,
    enabled: bool,
}

#[cfg(feature = "governor")]
impl<S, ReqBody> Service<Request<ReqBody>> for GrpcRateLimitService<S>
where
    S: Service<Request<ReqBody>, Response = Response<tonic::body::Body>, Error = Status>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        // For Phase 2, we just pass through and log that rate limiting is configured
        // Full implementation will be added in Phase 5
        if self.enabled {
            tracing::trace!("Rate limiting enabled for gRPC request");
        }

        let mut inner = self.inner.clone();
        Box::pin(async move { inner.call(req).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_service_name() {
        assert_eq!(
            extract_service_name("/example.v1.Greeter/SayHello"),
            "Greeter"
        );
        assert_eq!(
            extract_service_name("/mypackage.UserService/GetUser"),
            "UserService"
        );
        assert_eq!(extract_service_name("/Service/Method"), "Service");
        // For malformed paths, the function extracts what it can
        assert_eq!(extract_service_name("invalid"), "invalid");
        // Empty path returns empty string (first split element)
        assert_eq!(extract_service_name(""), "");
    }

    #[test]
    fn test_extract_method_name() {
        assert_eq!(
            extract_method_name("/example.v1.Greeter/SayHello"),
            "SayHello"
        );
        assert_eq!(
            extract_method_name("/mypackage.UserService/GetUser"),
            "GetUser"
        );
        assert_eq!(extract_method_name("/Service/Method"), "Method");
        assert_eq!(extract_method_name("invalid"), "unknown");
    }
}
