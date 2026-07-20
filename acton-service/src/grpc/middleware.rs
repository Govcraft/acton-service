//! gRPC middleware utilities
//!
//! Provides Tower middleware that can be used with gRPC services.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;
use tonic::server::NamedService;
use tonic::Status;
use tower::{Layer, Service};
use tracing::Instrument;

use crate::grpc::interceptors::RequestIdExtension;
use crate::middleware::token::{extract_token, TokenValidator};

/// Logging middleware for gRPC requests
///
/// An HTTP-level tower layer (gRPC requests are HTTP/2 requests), so it
/// composes both with
/// [`GrpcServicesBuilder::add_service`](crate::grpc::server::GrpcServicesBuilder::add_service)
/// (the `NamedService` impl forwards the inner service's name) and with
/// `axum::Router::layer`.
///
/// Logs the method path and duration of every request. The gRPC status is
/// logged from the `grpc-status` response header, which is only present on
/// trailers-only responses (errors produced before the handler, e.g. by
/// authentication layers); responses whose status arrives in HTTP/2 trailers
/// are logged as status `0`.
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

impl<S: NamedService> NamedService for LoggingService<S> {
    const NAME: &'static str = S::NAME;
}

impl<S, ReqBody, ResBody> Service<http::Request<ReqBody>> for LoggingService<S>
where
    S: Service<http::Request<ReqBody>, Response = http::Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: std::fmt::Display,
    ReqBody: Send + 'static,
    ResBody: Send + 'static,
{
    type Response = http::Response<ResBody>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<ReqBody>) -> Self::Future {
        // Take the ready inner service and leave a fresh clone in its place,
        // so the readiness obtained via poll_ready is the one consumed here.
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);

        let method = req.uri().path().to_string();

        Box::pin(async move {
            let start = Instant::now();

            tracing::debug!(method = %method, "gRPC request started");

            let result = inner.call(req).await;

            let duration = start.elapsed();

            match &result {
                Ok(response) => {
                    let grpc_status = response
                        .headers()
                        .get("grpc-status")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("0");
                    tracing::info!(
                        method = %method,
                        duration_ms = duration.as_millis(),
                        grpc.status_code = grpc_status,
                        "gRPC request completed"
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        method = %method,
                        duration_ms = duration.as_millis(),
                        error = %error,
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
/// Creates an OpenTelemetry-compatible span for each gRPC request, with
/// `rpc.service`/`rpc.method` taken from the request URI path and the request
/// ID from a [`RequestIdExtension`] inserted by an earlier layer or from the
/// `x-request-id` header.
///
/// Like [`LoggingLayer`], this is an HTTP-level tower layer with a forwarding
/// `NamedService` impl, so a wrapped service can be registered with
/// [`GrpcServicesBuilder::add_service`](crate::grpc::server::GrpcServicesBuilder::add_service)
/// or layered onto an axum router.
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

impl<S: NamedService> NamedService for GrpcTracingService<S> {
    const NAME: &'static str = S::NAME;
}

impl<S, ReqBody, ResBody> Service<http::Request<ReqBody>> for GrpcTracingService<S>
where
    S: Service<http::Request<ReqBody>, Response = http::Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: std::fmt::Display,
    ReqBody: Send + 'static,
    ResBody: Send + 'static,
{
    type Response = http::Response<ResBody>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<ReqBody>) -> Self::Future {
        // Take the ready inner service and leave a fresh clone in its place,
        // so the readiness obtained via poll_ready is the one consumed here.
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);

        let request_id = req
            .extensions()
            .get::<RequestIdExtension>()
            .map(|ext| ext.0.clone())
            .or_else(|| {
                req.headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| "unknown".to_string());

        let method = req.uri().path().to_string();

        let span = tracing::info_span!(
            "grpc_request",
            otel.kind = "server",
            rpc.system = "grpc",
            rpc.service = %extract_service_name(&method),
            rpc.method = %extract_method_name(&method),
            request_id = %request_id,
        );

        Box::pin(
            async move {
                let start = Instant::now();

                tracing::debug!(method = %method, "gRPC request started");

                let result = inner.call(req).await;

                let duration = start.elapsed();

                match &result {
                    Ok(response) => {
                        // Only trailers-only (error) responses carry grpc-status
                        // in the headers; anything else is OK at this point.
                        let status = response
                            .headers()
                            .get("grpc-status")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("0");

                        tracing::info!(
                            duration_ms = duration.as_millis(),
                            grpc.status_code = status,
                            "gRPC request completed"
                        );
                    }
                    Err(error) => {
                        tracing::warn!(
                            duration_ms = duration.as_millis(),
                            error.message = %error,
                            "gRPC request failed"
                        );
                    }
                }

                result
            }
            .instrument(span),
        )
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

/// Governor rate limiter shared by every service a [`GrpcRateLimitLayer`]
/// wraps, and by every clone of those services.
#[cfg(feature = "governor")]
type DirectRateLimiter = governor::RateLimiter<
    governor::state::NotKeyed,
    governor::state::InMemoryState,
    governor::clock::DefaultClock,
>;

/// Token bucket rate limiting layer for gRPC
///
/// Sustains `requests_per_period` requests per `period_secs`, allowing spikes
/// up to `burst_size`. Requests over the limit are answered with a gRPC
/// `RESOURCE_EXHAUSTED` status without reaching the inner service. Health and
/// reflection methods (see the module docs) are exempt so infrastructure
/// probes are never throttled.
///
/// The limiter state lives in the layer, so all services wrapped by one layer
/// (and all clones of them) share a single bucket. The bucket is in-memory
/// and per-instance; for distributed rate limiting, use Redis-based rate
/// limiting in your gRPC handlers.
///
/// Like the other layers in this module, this is an HTTP-level tower layer
/// with a forwarding `NamedService` impl, so a wrapped service can be
/// registered with
/// [`GrpcServicesBuilder::add_service`](crate::grpc::server::GrpcServicesBuilder::add_service)
/// or layered onto an axum router.
#[cfg(feature = "governor")]
#[derive(Clone)]
pub struct GrpcRateLimitLayer {
    enabled: bool,
    limiter: Arc<DirectRateLimiter>,
}

#[cfg(feature = "governor")]
impl GrpcRateLimitLayer {
    /// Create a new rate limiting layer
    pub fn new(config: crate::config::LocalRateLimitConfig) -> Self {
        use std::num::NonZeroU32;

        // Replenish one token every period/requests, with bucket capacity
        // burst_size — the same quota shape as the HTTP-side governor.
        let requests = u64::from(config.requests_per_period.max(1));
        let replenish_interval =
            std::time::Duration::from_millis((config.period().as_millis() as u64 / requests).max(1));
        let burst = NonZeroU32::new(config.burst_size.max(1)).expect("max(1) is non-zero");
        let quota = governor::Quota::with_period(replenish_interval)
            .expect("replenish interval is non-zero")
            .allow_burst(burst);

        Self {
            enabled: config.enabled,
            limiter: Arc::new(governor::RateLimiter::direct(quota)),
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
            limiter: self.limiter.clone(),
        }
    }
}

/// Rate limiting service implementation
///
/// See [`GrpcRateLimitLayer`] for usage.
#[cfg(feature = "governor")]
#[derive(Clone)]
pub struct GrpcRateLimitService<S> {
    inner: S,
    enabled: bool,
    limiter: Arc<DirectRateLimiter>,
}

#[cfg(feature = "governor")]
impl<S: NamedService> NamedService for GrpcRateLimitService<S> {
    const NAME: &'static str = S::NAME;
}

#[cfg(feature = "governor")]
impl<S, ReqBody, ResBody> Service<http::Request<ReqBody>> for GrpcRateLimitService<S>
where
    S: Service<http::Request<ReqBody>, Response = http::Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
    ResBody: Default + Send + 'static,
{
    type Response = http::Response<ResBody>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<ReqBody>) -> Self::Future {
        // Take the ready inner service and leave a fresh clone in its place,
        // so the readiness obtained via poll_ready is the one consumed here.
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);

        if self.enabled && !is_grpc_infra_path(req.uri().path()) {
            if let Err(not_until) = self.limiter.check() {
                let retry_after = not_until.wait_time_from(governor::clock::Clock::now(
                    &governor::clock::DefaultClock::default(),
                ));
                tracing::warn!(
                    method = %req.uri().path(),
                    retry_after_ms = retry_after.as_millis(),
                    "gRPC rate limit exceeded"
                );
                let status = Status::resource_exhausted(format!(
                    "Rate limit exceeded; retry in {}ms",
                    retry_after.as_millis()
                ));
                return Box::pin(async move { Ok(status.into_http()) });
            }
        }

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

    mod observability_and_rate_limit {
        use super::super::*;
        use std::convert::Infallible;

        /// Echoes the request path back as the response body
        #[derive(Clone)]
        struct Echo;

        impl NamedService for Echo {
            const NAME: &'static str = "test.v1.Echo";
        }

        impl Service<http::Request<String>> for Echo {
            type Response = http::Response<String>;
            type Error = Infallible;
            type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

            fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                Poll::Ready(Ok(()))
            }

            fn call(&mut self, req: http::Request<String>) -> Self::Future {
                std::future::ready(Ok(http::Response::new(req.uri().path().to_string())))
            }
        }

        fn request(path: &str) -> http::Request<String> {
            http::Request::builder()
                .uri(path)
                .body(String::new())
                .unwrap()
        }

        #[test]
        fn named_service_impls_forward_the_inner_name() {
            assert_eq!(<LoggingService<Echo> as NamedService>::NAME, "test.v1.Echo");
            assert_eq!(
                <GrpcTracingService<Echo> as NamedService>::NAME,
                "test.v1.Echo"
            );
            #[cfg(feature = "governor")]
            assert_eq!(
                <GrpcRateLimitService<Echo> as NamedService>::NAME,
                "test.v1.Echo"
            );
        }

        #[tokio::test]
        async fn logging_layer_passes_the_request_through() {
            let mut svc = LoggingLayer.layer(Echo);
            let resp = svc
                .call(request("/hello.v1.HelloService/SayHello"))
                .await
                .unwrap();
            assert_eq!(resp.body(), "/hello.v1.HelloService/SayHello");
        }

        #[tokio::test]
        async fn tracing_layer_passes_the_request_through() {
            let mut svc = GrpcTracingLayer.layer(Echo);
            let resp = svc
                .call(request("/hello.v1.HelloService/SayHello"))
                .await
                .unwrap();
            assert_eq!(resp.body(), "/hello.v1.HelloService/SayHello");
        }

        #[cfg(feature = "governor")]
        mod rate_limit {
            use super::*;
            use crate::config::LocalRateLimitConfig;

            fn config(enabled: bool) -> LocalRateLimitConfig {
                // One request per hour with burst 1, so the second request in
                // a test deterministically exceeds the limit.
                LocalRateLimitConfig {
                    enabled,
                    requests_per_period: 1,
                    period_secs: 3600,
                    burst_size: 1,
                }
            }

            fn grpc_status(resp: &http::Response<String>) -> Option<&str> {
                resp.headers()
                    .get("grpc-status")
                    .and_then(|v| v.to_str().ok())
            }

            #[tokio::test]
            async fn requests_over_the_burst_get_resource_exhausted() {
                let mut svc = GrpcRateLimitLayer::new(config(true)).layer(Echo);

                let first = svc
                    .call(request("/hello.v1.HelloService/SayHello"))
                    .await
                    .unwrap();
                assert_eq!(grpc_status(&first), None);

                let second = svc
                    .call(request("/hello.v1.HelloService/SayHello"))
                    .await
                    .unwrap();
                // tonic Code::ResourceExhausted == 8
                assert_eq!(grpc_status(&second), Some("8"));
            }

            #[tokio::test]
            async fn disabled_limiter_passes_everything_through() {
                let mut svc = GrpcRateLimitLayer::new(config(false)).layer(Echo);

                for _ in 0..3 {
                    let resp = svc
                        .call(request("/hello.v1.HelloService/SayHello"))
                        .await
                        .unwrap();
                    assert_eq!(grpc_status(&resp), None);
                }
            }

            #[tokio::test]
            async fn health_service_is_exempt() {
                let mut svc = GrpcRateLimitLayer::new(config(true)).layer(Echo);

                for _ in 0..3 {
                    let resp = svc
                        .call(request("/grpc.health.v1.Health/Check"))
                        .await
                        .unwrap();
                    assert_eq!(grpc_status(&resp), None);
                }
            }

            #[tokio::test]
            async fn limiter_state_is_shared_across_wrapped_services() {
                let layer = GrpcRateLimitLayer::new(config(true));
                let mut first_svc = layer.layer(Echo);
                let mut second_svc = layer.layer(Echo);

                let first = first_svc
                    .call(request("/hello.v1.HelloService/SayHello"))
                    .await
                    .unwrap();
                assert_eq!(grpc_status(&first), None);

                // The second service shares the layer's bucket, so the burst
                // consumed above applies to it too.
                let second = second_svc
                    .call(request("/hello.v1.HelloService/SayHello"))
                    .await
                    .unwrap();
                assert_eq!(grpc_status(&second), Some("8"));
            }
        }
    }
}
