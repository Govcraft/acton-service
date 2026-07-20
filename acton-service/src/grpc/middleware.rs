//! gRPC middleware utilities
//!
//! Provides Tower middleware that can be used with gRPC services.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;
use tonic::server::NamedService;
use tonic::{Request, Response, Status};
use tower::{Layer, Service};

use crate::grpc::interceptors::RequestIdExtension;
use crate::middleware::token::{extract_token, TokenValidator};

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

/// Whether a gRPC method path belongs to infrastructure services that are
/// exempt from authentication and authorization.
///
/// Covers the standard health checking protocol (`grpc.health.v1.Health`),
/// which infrastructure probes call without credentials, and server
/// reflection (`grpc.reflection.*`), mirroring the `/health` and `/ready`
/// exemptions on the HTTP side.
pub(crate) fn is_grpc_infra_path(path: &str) -> bool {
    path.starts_with("/grpc.health.v1.Health/") || path.starts_with("/grpc.reflection.")
}

/// HTTP-level token authentication layer for gRPC services
///
/// Validates the `authorization` bearer token (gRPC metadata is carried in
/// HTTP headers) using any [`TokenValidator`] and inserts the resulting
/// [`Claims`](crate::middleware::token::Claims) into the request extensions,
/// where downstream layers such as
/// [`CedarAuthzLayer`](crate::middleware::cedar::CedarAuthzLayer) and service
/// handlers can read them. Requests that fail validation are answered with a
/// gRPC `UNAUTHENTICATED` status without reaching the inner service.
///
/// Unlike the tonic interceptor helpers in
/// [`interceptors`](crate::grpc::interceptors), which run *inside* a generated
/// server via `with_interceptor`, this layer wraps the service from the
/// outside, so claims are available to other wrapping layers. The
/// `NamedService` impl forwards the inner service's name, so a wrapped service
/// can be registered with
/// [`GrpcServicesBuilder::add_service`](crate::grpc::server::GrpcServicesBuilder::add_service).
///
/// Health and reflection service methods are exempt, as are any configured
/// public path prefixes.
///
/// # Example
/// ```ignore
/// let auth_layer = GrpcTokenAuthLayer::new(paseto_auth);
///
/// let services = GrpcServicesBuilder::new()
///     .add_service(auth_layer.layer(MyServiceServer::new(svc)))
///     .build(None);
/// ```
///
/// When token auth is configured through [`Config`](crate::config::Config),
/// the framework applies this layer to all gRPC routes automatically; this
/// type is for manual composition.
///
/// Note: like the tonic interceptor helpers (and unlike the HTTP
/// middleware), this layer validates the token itself but does not consult
/// a token revocation list.
#[derive(Clone)]
pub struct GrpcTokenAuthLayer<V> {
    validator: V,
    public_paths: Arc<[String]>,
}

impl<V> GrpcTokenAuthLayer<V> {
    /// Create a new token authentication layer from a validator
    pub fn new(validator: V) -> Self {
        Self {
            validator,
            public_paths: Arc::from(Vec::new()),
        }
    }

    /// Exempt gRPC method paths starting with any of these prefixes
    ///
    /// Prefixes match against the full method path, e.g.
    /// `"/hello.v1.HelloService/"` exempts every method of that service.
    pub fn with_public_paths(mut self, paths: Vec<String>) -> Self {
        self.public_paths = paths.into();
        self
    }
}

impl<S, V: Clone> Layer<S> for GrpcTokenAuthLayer<V> {
    type Service = GrpcTokenAuthService<S, V>;

    fn layer(&self, inner: S) -> Self::Service {
        GrpcTokenAuthService {
            inner,
            validator: self.validator.clone(),
            public_paths: self.public_paths.clone(),
        }
    }
}

/// HTTP-level token authentication service for gRPC
///
/// See [`GrpcTokenAuthLayer`] for usage.
#[derive(Clone)]
pub struct GrpcTokenAuthService<S, V> {
    inner: S,
    validator: V,
    public_paths: Arc<[String]>,
}

impl<S: NamedService, V> NamedService for GrpcTokenAuthService<S, V> {
    const NAME: &'static str = S::NAME;
}

impl<S, V, ReqBody, ResBody> Service<http::Request<ReqBody>> for GrpcTokenAuthService<S, V>
where
    S: Service<http::Request<ReqBody>, Response = http::Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    V: TokenValidator,
    ReqBody: Send + 'static,
    ResBody: Default + Send + 'static,
{
    type Response = http::Response<ResBody>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: http::Request<ReqBody>) -> Self::Future {
        // Take the ready inner service and leave a fresh clone in its place,
        // so the readiness obtained via poll_ready is the one consumed here.
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);

        let path = req.uri().path();
        let public = is_grpc_infra_path(path)
            || self
                .public_paths
                .iter()
                .any(|p| path.starts_with(p.as_str()));

        if !public {
            let validated = extract_token(req.headers())
                .and_then(|token| self.validator.validate_token(&token));
            match validated {
                Ok(claims) => {
                    tracing::debug!(
                        sub = %claims.sub,
                        roles = ?claims.roles,
                        "gRPC request authenticated"
                    );
                    req.extensions_mut().insert(claims);
                }
                Err(e) => {
                    let status = Status::unauthenticated(e.to_string());
                    return Box::pin(async move { Ok(status.into_http()) });
                }
            }
        }

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

    #[test]
    fn test_is_grpc_infra_path() {
        assert!(is_grpc_infra_path("/grpc.health.v1.Health/Check"));
        assert!(is_grpc_infra_path("/grpc.health.v1.Health/Watch"));
        assert!(is_grpc_infra_path(
            "/grpc.reflection.v1.ServerReflection/ServerReflectionInfo"
        ));
        assert!(is_grpc_infra_path(
            "/grpc.reflection.v1alpha.ServerReflection/ServerReflectionInfo"
        ));
        assert!(!is_grpc_infra_path("/hello.v1.HelloService/SayHello"));
    }

    mod token_auth {
        use super::super::*;
        use crate::error::Error;
        use crate::middleware::token::Claims;
        use std::convert::Infallible;

        #[derive(Clone)]
        struct TestValidator;

        impl TokenValidator for TestValidator {
            fn validate_token(&self, token: &str) -> Result<Claims, Error> {
                if token == "good" {
                    Ok(Claims {
                        sub: "user:123".to_string(),
                        email: None,
                        username: None,
                        roles: vec![],
                        perms: vec![],
                        exp: 0,
                        iat: None,
                        jti: None,
                        iss: None,
                        aud: None,
                        custom: Default::default(),
                    })
                } else {
                    Err(Error::Unauthorized("Invalid token".to_string()))
                }
            }
        }

        /// Reports whether Claims were present when the request arrived
        #[derive(Clone)]
        struct ClaimsProbe;

        impl NamedService for ClaimsProbe {
            const NAME: &'static str = "test.v1.ClaimsProbe";
        }

        impl Service<http::Request<String>> for ClaimsProbe {
            type Response = http::Response<String>;
            type Error = Infallible;
            type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

            fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                Poll::Ready(Ok(()))
            }

            fn call(&mut self, req: http::Request<String>) -> Self::Future {
                let body = if req.extensions().get::<Claims>().is_some() {
                    "claims"
                } else {
                    "no-claims"
                };
                std::future::ready(Ok(http::Response::new(body.to_string())))
            }
        }

        fn request(path: &str, token: Option<&str>) -> http::Request<String> {
            let mut builder = http::Request::builder().uri(path);
            if let Some(token) = token {
                builder = builder.header("authorization", format!("Bearer {token}"));
            }
            builder.body(String::new()).unwrap()
        }

        fn grpc_status(resp: &http::Response<String>) -> Option<&str> {
            resp.headers()
                .get("grpc-status")
                .and_then(|v| v.to_str().ok())
        }

        fn service() -> GrpcTokenAuthService<ClaimsProbe, TestValidator> {
            GrpcTokenAuthLayer::new(TestValidator).layer(ClaimsProbe)
        }

        #[test]
        fn named_service_impl_forwards_the_inner_name() {
            assert_eq!(
                <GrpcTokenAuthService<ClaimsProbe, TestValidator> as NamedService>::NAME,
                "test.v1.ClaimsProbe"
            );
        }

        #[tokio::test]
        async fn valid_token_injects_claims() {
            let resp = service()
                .call(request("/hello.v1.HelloService/SayHello", Some("good")))
                .await
                .unwrap();
            assert_eq!(resp.body(), "claims");
        }

        #[tokio::test]
        async fn missing_token_is_unauthenticated() {
            let resp = service()
                .call(request("/hello.v1.HelloService/SayHello", None))
                .await
                .unwrap();
            // tonic Code::Unauthenticated == 16
            assert_eq!(grpc_status(&resp), Some("16"));
        }

        #[tokio::test]
        async fn invalid_token_is_unauthenticated() {
            let resp = service()
                .call(request("/hello.v1.HelloService/SayHello", Some("bad")))
                .await
                .unwrap();
            assert_eq!(grpc_status(&resp), Some("16"));
        }

        #[tokio::test]
        async fn health_service_is_exempt() {
            let resp = service()
                .call(request("/grpc.health.v1.Health/Check", None))
                .await
                .unwrap();
            assert_eq!(resp.body(), "no-claims");
        }

        #[tokio::test]
        async fn public_path_prefixes_are_exempt() {
            let mut svc = GrpcTokenAuthLayer::new(TestValidator)
                .with_public_paths(vec!["/hello.v1.HelloService/".to_string()])
                .layer(ClaimsProbe);
            let resp = svc
                .call(request("/hello.v1.HelloService/SayHello", None))
                .await
                .unwrap();
            assert_eq!(resp.body(), "no-claims");

            let resp = svc
                .call(request("/other.v1.OtherService/Do", None))
                .await
                .unwrap();
            assert_eq!(grpc_status(&resp), Some("16"));
        }
    }
}
