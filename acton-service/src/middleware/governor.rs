//! Governor-based rate limiting middleware
//!
//! Provides local (in-memory) rate limiting as a fallback or complement
//! to Redis-based global rate limiting. Useful for per-endpoint limits
//! and when Redis is unavailable.

use std::time::Duration;

#[cfg(feature = "governor")]
use std::num::NonZeroU32;
#[cfg(feature = "governor")]
use std::sync::Arc;

#[cfg(feature = "governor")]
use axum::{
    body::Body,
    extract::{OriginalUri, Request, State},
    http::{header::HeaderValue, HeaderName},
    middleware::Next,
    response::Response,
};

#[cfg(feature = "governor")]
use dashmap::DashMap;
#[cfg(feature = "governor")]
use governor::{
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
#[cfg(feature = "governor")]
use tracing::{debug, warn};

// Single source of truth for client-IP resolution; the governor supplies its
// own `trust_forwarded_headers` policy rather than the request-context default.
#[cfg(feature = "governor")]
use super::request_context::extract_client_ip;

#[cfg(feature = "governor")]
use super::rate_key::{RateClass, RateClassifier, RateKey, RateRequest, SharedClassifier};
#[cfg(feature = "governor")]
use crate::config::RateLimitConfig;
#[cfg(feature = "governor")]
use crate::error::Error;
#[cfg(all(feature = "governor", feature = "audit"))]
use crate::middleware::Claims;
#[cfg(feature = "governor")]
use crate::middleware::{normalize_path, CompiledRoutePatterns};

/// Configuration for governor-based rate limiting
#[derive(Debug, Clone)]
pub struct GovernorConfig {
    /// Enable governor rate limiting
    pub enabled: bool,
    /// Maximum requests per period
    pub requests_per_period: u32,
    /// Time period for rate limit
    pub period: Duration,
    /// Burst size (allow temporary spikes)
    pub burst_size: u32,
}

impl Default for GovernorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            requests_per_period: 100,
            period: Duration::from_secs(60), // 100 requests per minute
            burst_size: 10,                  // Allow bursts up to 110 requests
        }
    }
}

impl GovernorConfig {
    /// Create a new governor configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set governor enabled
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set requests per period
    pub fn with_requests_per_period(mut self, requests: u32) -> Self {
        self.requests_per_period = requests;
        self
    }

    /// Set time period
    pub fn with_period(mut self, period: Duration) -> Self {
        self.period = period;
        self
    }

    /// Set burst size
    pub fn with_burst_size(mut self, burst: u32) -> Self {
        self.burst_size = burst;
        self
    }

    /// Create configuration for per-second limiting
    pub fn per_second(requests: u32) -> Self {
        Self {
            enabled: true,
            requests_per_period: requests,
            period: Duration::from_secs(1),
            burst_size: requests / 10, // 10% burst allowance
        }
    }

    /// Create configuration for per-minute limiting
    pub fn per_minute(requests: u32) -> Self {
        Self {
            enabled: true,
            requests_per_period: requests,
            period: Duration::from_secs(60),
            burst_size: requests / 10, // 10% burst allowance
        }
    }

    /// Create configuration for per-hour limiting
    pub fn per_hour(requests: u32) -> Self {
        Self {
            enabled: true,
            requests_per_period: requests,
            period: Duration::from_secs(3600),
            burst_size: requests / 10, // 10% burst allowance
        }
    }
}

/// Response when rate limit is exceeded
#[derive(Debug, Clone)]
pub struct RateLimitExceeded {
    /// When the rate limit will reset
    pub retry_after: Duration,
    /// Maximum requests allowed
    pub limit: u32,
    /// Time period for the limit
    pub period: Duration,
}

impl RateLimitExceeded {
    /// Create a new rate limit exceeded response
    pub fn new(retry_after: Duration, limit: u32, period: Duration) -> Self {
        Self {
            retry_after,
            limit,
            period,
        }
    }

    /// The `Retry-After` value in seconds: the wait rounded up, and at least 1.
    pub fn retry_after_secs(&self) -> u64 {
        crate::error::retry_after_secs(self.retry_after)
    }
}

/// Type alias for a governor rate limiter
#[cfg(feature = "governor")]
type GovernorLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

/// Governor-based rate limiting middleware state
///
/// Provides local (in-memory) rate limiting with per-route configuration support.
/// This is a fallback for when Redis is unavailable.
#[cfg(feature = "governor")]
#[derive(Clone)]
pub struct GovernorRateLimit {
    config: RateLimitConfig,
    route_patterns: Arc<CompiledRoutePatterns>,
    /// Per-route rate limiters, keyed by normalized route path
    route_limiters: Arc<DashMap<String, Arc<GovernorLimiter>>>,
    /// Global rate limiters, keyed by user/client/IP identifier
    global_limiters: Arc<DashMap<String, Arc<GovernorLimiter>>>,
    /// Decides which bucket each request is counted in
    classifier: SharedClassifier,
}

#[cfg(feature = "governor")]
impl GovernorRateLimit {
    /// Create a new governor-based rate limiting middleware
    pub fn new(config: RateLimitConfig) -> Self {
        let route_patterns = CompiledRoutePatterns::compile(&config.routes);
        Self {
            config,
            route_patterns: Arc::new(route_patterns),
            route_limiters: Arc::new(DashMap::new()),
            global_limiters: Arc::new(DashMap::new()),
            classifier: SharedClassifier::default(),
        }
    }

    /// Counts requests by `classifier` instead of the default
    /// [`ClaimsClassifier`](super::rate_key::ClaimsClassifier).
    pub fn with_classifier(mut self, classifier: impl RateClassifier) -> Self {
        self.classifier = SharedClassifier::new(classifier);
        self
    }

    /// As [`with_classifier`](Self::with_classifier), for a classifier that is
    /// already shared.
    pub(crate) fn with_shared_classifier(mut self, classifier: Arc<dyn RateClassifier>) -> Self {
        self.classifier = SharedClassifier::from_arc(classifier);
        self
    }

    /// Middleware function to enforce rate limits
    ///
    /// Paths on [`RateLimitConfig::exempt_paths`] are never counted. Every
    /// other request is classified (see [`with_classifier`](Self::with_classifier);
    /// the default classifies by JWT/PASETO claims), and a request the
    /// classifier does not exempt is checked in this order:
    /// 1. Per-route limits (if configured for the request path)
    /// 2. The keyed caller's per-user or per-client limit
    /// 3. Per-IP fallback for an anonymous caller
    ///
    /// The path used for route matching is the request URI as seen by this
    /// layer. When the layer is attached to the outer router (the default
    /// auto-apply position), this is the full pre-nest path. When the
    /// middleware is wired manually inside a nested router, axum populates
    /// `OriginalUri` in the request extensions; the middleware prefers that
    /// value over the post-nest URI so route keys still match the documented
    /// full-path form.
    pub async fn middleware(
        State(rate_limit): State<Self>,
        request: Request<Body>,
        next: Next,
    ) -> Result<Response, Error> {
        let method = request.method().as_str().to_string();

        // Prefer OriginalUri (set by axum on nested routers) so route-key
        // matching always sees the full pre-nest path.
        let path = request
            .extensions()
            .get::<OriginalUri>()
            .map(|ou| ou.0.path().to_string())
            .unwrap_or_else(|| request.uri().path().to_string());

        // Probes and other configured paths are never counted, whatever the
        // classifier would say.
        if rate_limit.config.is_exempt_path(&path) {
            return Ok(next.run(request).await);
        }

        // Resolve the peer address from whichever connect-info the listener
        // installed: `ConnectInfo<SocketAddr>` on plain TCP, or
        // `ConnectInfo<TlsConnectInfo>` on a directly-terminated TLS listener.
        // Without the TLS fallback, per-IP rate limiting silently no-ops on a
        // TLS listener that has no fronting proxy.
        let connect_info = super::request_context::connect_info_remote_addr(request.extensions());
        let client_ip = extract_client_ip(
            request.headers(),
            connect_info.as_ref(),
            rate_limit.config.trust_forwarded_headers,
        );
        let rate_key = rate_limit.classifier.classify(&RateRequest::new(
            request.method(),
            &path,
            request.headers(),
            request.extensions(),
            client_ip,
        ));
        #[cfg(feature = "audit")]
        let subject = request.extensions().get::<Claims>().map(|c| c.sub.clone());

        // Check rate limit and get result for headers
        let result = match rate_limit.check_rate_limit(&method, &path, &rate_key) {
            Ok(Some(result)) => result,
            // The classifier exempted the request: nothing counted, no headers.
            Ok(None) => return Ok(next.run(request).await),
            Err(e) => {
                // Mirror the Redis rate limiter: rejections are audit-visible
                // as HttpRequestDenied (issue #16).
                #[cfg(feature = "audit")]
                if matches!(e, Error::RateLimitExceeded { .. }) {
                    if let Some(logger) = request
                        .extensions()
                        .get::<crate::audit::AuditLogger>()
                        .cloned()
                    {
                        if logger.config().audit_auth_events {
                            let mut source =
                                super::request_context::audit_source_for_request(&request);
                            source.subject = subject;
                            logger
                                .log(
                                    crate::audit::event::AuditEvent::new(
                                        crate::audit::event::AuditEventKind::HttpRequestDenied,
                                        crate::audit::event::AuditSeverity::Warning,
                                        logger.service_name().to_string(),
                                    )
                                    .with_source(source)
                                    .with_http(
                                        request.method().to_string(),
                                        request.uri().path().to_string(),
                                        Some(429),
                                        None,
                                    )
                                    .with_metadata(
                                        serde_json::json!({"reason": "rate_limit_exceeded"}),
                                    ),
                                )
                                .await;
                        }
                    }
                }
                return Err(e);
            }
        };

        // Run the request
        let mut response = next.run(request).await;

        // Add rate limit headers to response
        Self::add_rate_limit_headers(&mut response, &result);

        Ok(response)
    }

    /// Counts a request classified as `rate_key` against its bucket, and
    /// answers `None` when the classifier exempted it.
    fn check_rate_limit(
        &self,
        method: &str,
        path: &str,
        rate_key: &RateKey,
    ) -> Result<Option<GovernorRateLimitResult>, Error> {
        let Some(bucket) = self.bucket(method, path, rate_key) else {
            return Ok(None);
        };
        let limiters = match bucket.scope {
            BucketScope::Route => &self.route_limiters,
            BucketScope::Global => &self.global_limiters,
        };
        self.check_with_limiter(limiters, &bucket.key, bucket.rpm, bucket.burst)
            .map(Some)
    }

    /// The bucket a request classified as `rate_key` is counted in, or `None`
    /// when it is exempt.
    ///
    /// A per-route limit for the request's route comes first: a per-user route
    /// limit keys it by the classified identity or else the client address,
    /// and a shared one keys every caller alike. Otherwise a keyed request is
    /// counted in its own bucket at its class's quota, and an anonymous one in
    /// its address's bucket at [`RateLimitConfig::anonymous_quota`].
    fn bucket(&self, method: &str, path: &str, rate_key: &RateKey) -> Option<Bucket> {
        if matches!(rate_key, RateKey::Exempt) {
            return None;
        }
        let normalized_path = normalize_path(path);

        if let Some(route_config) = self.route_patterns.match_route(method, &normalized_path) {
            debug!(
                "Using per-route governor limit for {} {}: {} rpm",
                method, normalized_path, route_config.requests_per_minute
            );
            let key = if route_config.per_user {
                match rate_key {
                    RateKey::Key { id, .. } => format!("route:{}:{}", normalized_path, id),
                    RateKey::Anonymous(Some(ip)) => format!("route:{}:ip:{}", normalized_path, ip),
                    RateKey::Anonymous(None) | RateKey::Exempt => {
                        format!("route:{}:ip:unknown", normalized_path)
                    }
                }
            } else {
                format!("route:{}:global", normalized_path)
            };
            return Some(Bucket {
                scope: BucketScope::Route,
                key,
                rpm: route_config.requests_per_minute,
                burst: route_config.burst_size,
            });
        }

        let (key, rpm, burst) = match rate_key {
            RateKey::Key { id, class } => {
                let rpm = match class {
                    RateClass::User => self.config.per_user_rpm,
                    RateClass::Client => self.config.per_client_rpm,
                };
                (format!("governor:{}", id), rpm, (rpm / 10).max(1))
            }
            RateKey::Anonymous(ip) => {
                let (rpm, burst) = self.config.anonymous_quota();
                let key = match ip {
                    Some(ip) => format!("governor:ip:{}", ip),
                    None => "governor:ip:unknown".to_string(),
                };
                (key, rpm, burst)
            }
            RateKey::Exempt => return None,
        };
        Some(Bucket {
            scope: BucketScope::Global,
            key,
            rpm,
            burst,
        })
    }

    /// Check rate limit using a specific limiter map
    fn check_with_limiter(
        &self,
        limiters: &DashMap<String, Arc<GovernorLimiter>>,
        key: &str,
        requests_per_minute: u32,
        burst_size: u32,
    ) -> Result<GovernorRateLimitResult, Error> {
        // Get or create limiter for this key
        let limiter = limiters
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(Self::create_limiter(requests_per_minute, burst_size)))
            .clone();

        // Try to acquire a permit
        match limiter.check() {
            Ok(_) => {
                // Calculate approximate remaining based on quota
                // Governor doesn't expose exact counts, so we estimate
                let remaining = requests_per_minute.saturating_sub(1);
                Ok(GovernorRateLimitResult {
                    limit: requests_per_minute,
                    remaining,
                    reset_secs: 60, // 1 minute window
                })
            }
            Err(not_until) => {
                let retry_after = not_until.wait_time_from(governor::clock::Clock::now(
                    &governor::clock::DefaultClock::default(),
                ));

                warn!(
                    "Governor rate limit exceeded for {}: retry after {:?}",
                    key, retry_after
                );

                Err(Error::rate_limited(retry_after))
            }
        }
    }

    /// Create a new rate limiter with the given configuration
    fn create_limiter(requests_per_minute: u32, burst_size: u32) -> GovernorLimiter {
        let burst = NonZeroU32::new(burst_size.max(1)).unwrap_or(NonZeroU32::MIN);
        let quota = Quota::with_period(Self::replenish_interval(requests_per_minute))
            .unwrap_or_else(|| Quota::per_second(NonZeroU32::MAX))
            .allow_burst(burst);

        RateLimiter::direct(quota)
    }

    /// How often one token is added back to a bucket of `requests_per_minute`.
    ///
    /// Counted in nanoseconds: a whole-millisecond interval is zero above
    /// 60 000 requests per minute, and a zero period is not a quota.
    fn replenish_interval(requests_per_minute: u32) -> Duration {
        Duration::from_nanos(60_000_000_000 / u64::from(requests_per_minute.max(1)))
    }

    /// Add rate limit headers to response
    fn add_rate_limit_headers(response: &mut Response, result: &GovernorRateLimitResult) {
        let headers = response.headers_mut();

        // Standard rate limit headers
        if let Ok(value) = HeaderValue::from_str(&result.limit.to_string()) {
            headers.insert(HeaderName::from_static("x-ratelimit-limit"), value);
        }

        if let Ok(value) = HeaderValue::from_str(&result.remaining.to_string()) {
            headers.insert(HeaderName::from_static("x-ratelimit-remaining"), value);
        }

        // Calculate reset timestamp
        let reset_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() + result.reset_secs)
            .unwrap_or(0);

        if let Ok(value) = HeaderValue::from_str(&reset_timestamp.to_string()) {
            headers.insert(HeaderName::from_static("x-ratelimit-reset"), value);
        }
    }

    /// Clean up stale rate limiters (call periodically)
    ///
    /// This removes limiters that haven't been used recently to prevent
    /// unbounded memory growth.
    pub fn cleanup_stale_limiters(&self, max_entries: usize) {
        // Simple cleanup: if we have too many entries, remove some
        // A more sophisticated approach would track last access time
        if self.route_limiters.len() > max_entries {
            let to_remove = self.route_limiters.len() - max_entries;
            let keys: Vec<String> = self
                .route_limiters
                .iter()
                .take(to_remove)
                .map(|e| e.key().clone())
                .collect();
            for key in keys {
                self.route_limiters.remove(&key);
            }
        }

        if self.global_limiters.len() > max_entries {
            let to_remove = self.global_limiters.len() - max_entries;
            let keys: Vec<String> = self
                .global_limiters
                .iter()
                .take(to_remove)
                .map(|e| e.key().clone())
                .collect();
            for key in keys {
                self.global_limiters.remove(&key);
            }
        }
    }
}

/// Which limiter map a bucket lives in.
#[cfg(feature = "governor")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BucketScope {
    Route,
    Global,
}

/// One bucket a request is counted in.
#[cfg(feature = "governor")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct Bucket {
    scope: BucketScope,
    key: String,
    rpm: u32,
    burst: u32,
}

/// Rate limit check result for governor middleware
#[cfg(feature = "governor")]
struct GovernorRateLimitResult {
    /// Maximum requests allowed in window
    limit: u32,
    /// Approximate remaining requests
    remaining: u32,
    /// Seconds until window resets
    reset_secs: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = GovernorConfig::default();
        assert!(config.enabled);
        assert_eq!(config.requests_per_period, 100);
        assert_eq!(config.period, Duration::from_secs(60));
        assert_eq!(config.burst_size, 10);
    }

    #[test]
    fn test_builder_pattern() {
        let config = GovernorConfig::new()
            .with_enabled(true)
            .with_requests_per_period(50)
            .with_period(Duration::from_secs(30))
            .with_burst_size(5);

        assert!(config.enabled);
        assert_eq!(config.requests_per_period, 50);
        assert_eq!(config.period, Duration::from_secs(30));
        assert_eq!(config.burst_size, 5);
    }

    #[test]
    fn test_per_second() {
        let config = GovernorConfig::per_second(10);
        assert_eq!(config.requests_per_period, 10);
        assert_eq!(config.period, Duration::from_secs(1));
        assert_eq!(config.burst_size, 1); // 10% of 10
    }

    #[test]
    fn test_per_minute() {
        let config = GovernorConfig::per_minute(100);
        assert_eq!(config.requests_per_period, 100);
        assert_eq!(config.period, Duration::from_secs(60));
        assert_eq!(config.burst_size, 10); // 10% of 100
    }

    #[test]
    fn test_per_hour() {
        let config = GovernorConfig::per_hour(1000);
        assert_eq!(config.requests_per_period, 1000);
        assert_eq!(config.period, Duration::from_secs(3600));
        assert_eq!(config.burst_size, 100); // 10% of 1000
    }

    #[test]
    fn test_rate_limit_exceeded() {
        let exceeded =
            RateLimitExceeded::new(Duration::from_secs(30), 100, Duration::from_secs(60));

        assert_eq!(exceeded.retry_after_secs(), 30);
        assert_eq!(exceeded.limit, 100);
        assert_eq!(exceeded.period, Duration::from_secs(60));
    }

    #[test]
    fn rate_limit_exceeded_rounds_its_wait_up_to_at_least_one_second() {
        let secs =
            |wait| RateLimitExceeded::new(wait, 100, Duration::from_secs(60)).retry_after_secs();
        assert_eq!(
            secs(Duration::from_millis(100)),
            1,
            "a sub-second wait is 1, never 0"
        );
        assert_eq!(secs(Duration::ZERO), 1, "never 0");
        assert_eq!(
            secs(Duration::from_millis(1_001)),
            2,
            "rounded up, not truncated"
        );
        assert_eq!(
            secs(Duration::from_secs(30)),
            30,
            "a whole wait is unchanged"
        );
    }

    #[cfg(feature = "governor")]
    #[test]
    fn test_governor_rate_limit_creation() {
        let config = RateLimitConfig::default();
        let _rate_limit = GovernorRateLimit::new(config);
    }

    #[cfg(feature = "governor")]
    #[test]
    fn test_governor_rate_limit_with_routes() {
        use crate::config::RouteRateLimitConfig;
        use std::collections::HashMap;

        let mut routes = HashMap::new();
        routes.insert(
            "/api/v1/heavy".to_string(),
            RouteRateLimitConfig {
                requests_per_minute: 10,
                burst_size: 2,
                per_user: true,
            },
        );

        let config = RateLimitConfig {
            routes,
            ..RateLimitConfig::default()
        };
        let rate_limit = GovernorRateLimit::new(config);

        // Verify route patterns were compiled
        assert!(!rate_limit.route_patterns.is_empty());
    }

    #[cfg(feature = "governor")]
    #[test]
    fn test_create_limiter() {
        // 60 requests per minute
        let limiter = GovernorRateLimit::create_limiter(60, 6);

        // Should allow first request
        assert!(limiter.check().is_ok());
    }

    #[cfg(feature = "governor")]
    #[test]
    fn test_limiter_burst() {
        // Allow burst of 5
        let limiter = GovernorRateLimit::create_limiter(60, 5);

        // Should allow burst of requests
        for _ in 0..5 {
            assert!(limiter.check().is_ok());
        }

        // Next request should fail (burst exhausted)
        assert!(limiter.check().is_err());
    }

    // ---------------------------------------------------------------------
    // Bug-fix regression tests for issue #7
    // ---------------------------------------------------------------------

    #[cfg(feature = "governor")]
    #[test]
    fn test_route_match_uses_full_path() {
        // Regression for bug 3: route-key matching must work against the
        // full pre-nest path. With the auto-apply layer attached to the
        // outer router, the middleware sees `/api/v1/uploads`, not just
        // `/uploads`.
        use crate::config::RouteRateLimitConfig;
        use std::collections::HashMap;

        let mut routes = HashMap::new();
        routes.insert(
            "POST /api/v1/uploads".to_string(),
            RouteRateLimitConfig {
                requests_per_minute: 10,
                burst_size: 1,
                per_user: false, // global to avoid claims/IP confusion
            },
        );

        let config = RateLimitConfig {
            routes,
            ..RateLimitConfig::default()
        };
        let rl = GovernorRateLimit::new(config);

        // Full path matches.
        let first = rl.check_rate_limit("POST", "/api/v1/uploads", &RateKey::Anonymous(None));
        assert!(first.is_ok());

        // The 2nd hit on the same global route bucket trips the burst=1 limit.
        let second = rl.check_rate_limit("POST", "/api/v1/uploads", &RateKey::Anonymous(None));
        assert!(matches!(second, Err(Error::RateLimitExceeded { .. })));

        // Post-nest path on a fresh middleware does NOT match the route key
        // (it falls through to the IP-based fallback, which uses the global
        // per_user_rpm and burst=20 — easily allows one request).
        let rl2 = GovernorRateLimit::new(RateLimitConfig {
            routes: {
                let mut m = HashMap::new();
                m.insert(
                    "POST /api/v1/uploads".to_string(),
                    RouteRateLimitConfig {
                        requests_per_minute: 10,
                        burst_size: 1,
                        per_user: false,
                    },
                );
                m
            },
            ..RateLimitConfig::default()
        });
        let post_nest = rl2.check_rate_limit("POST", "/uploads", &RateKey::Anonymous(None));
        assert!(
            post_nest.is_ok(),
            "post-nest path must not match the full-path config key"
        );
    }

    #[cfg(feature = "governor")]
    #[test]
    fn test_anonymous_falls_back_to_ip() {
        // Regression for bug 2: anonymous requests must be IP-rate-limited,
        // not silently allowed.
        use std::net::{IpAddr, Ipv4Addr};

        let config = RateLimitConfig {
            // Tiny limit so the test is fast and deterministic.
            per_user_rpm: 1,
            ..RateLimitConfig::default()
        };
        let rl = GovernorRateLimit::new(config);

        let ip_a = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let ip_b = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));

        // First request from ip_a succeeds.
        let r1 = rl.check_rate_limit("GET", "/whatever", &RateKey::Anonymous(Some(ip_a)));
        assert!(r1.is_ok());

        // Second request from ip_a is rate-limited (burst exhausted).
        let r2 = rl.check_rate_limit("GET", "/whatever", &RateKey::Anonymous(Some(ip_a)));
        assert!(matches!(r2, Err(Error::RateLimitExceeded { .. })));

        // ip_b gets a fresh bucket.
        let r3 = rl.check_rate_limit("GET", "/whatever", &RateKey::Anonymous(Some(ip_b)));
        assert!(r3.is_ok());
    }

    #[cfg(feature = "governor")]
    #[test]
    fn test_anonymous_no_ip_uses_unknown_bucket() {
        // When neither claims nor an IP are available, requests should still
        // be limited (they share a single "unknown" bucket — safer than
        // letting them through).
        let config = RateLimitConfig {
            per_user_rpm: 1,
            ..RateLimitConfig::default()
        };
        let rl = GovernorRateLimit::new(config);

        let first = rl.check_rate_limit("GET", "/x", &RateKey::Anonymous(None));
        assert!(first.is_ok());

        let second = rl.check_rate_limit("GET", "/x", &RateKey::Anonymous(None));
        assert!(matches!(second, Err(Error::RateLimitExceeded { .. })));
    }

    // ---------------------------------------------------------------------
    // Posture: exempt paths, the anonymous knob, Retry-After, mTLS exemption
    // ---------------------------------------------------------------------

    #[cfg(feature = "governor")]
    fn anonymous_router(config: RateLimitConfig) -> axum::Router {
        use axum::routing::get;
        let rate_limit = GovernorRateLimit::new(config);
        axum::Router::new()
            .route("/health", get(|| async { "ok" }))
            .route("/health/", get(|| async { "ok" }))
            .route("/ready", get(|| async { "ok" }))
            .route("/readyz", get(|| async { "ok" }))
            .route("/api/v1/thing", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(
                rate_limit,
                GovernorRateLimit::middleware,
            ))
    }

    #[cfg(feature = "governor")]
    fn from_ip(path: &str, last_octet: u8) -> Request<Body> {
        use std::net::{Ipv4Addr, SocketAddr};
        let mut request = Request::builder().uri(path).body(Body::empty()).unwrap();
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(SocketAddr::from((
                Ipv4Addr::new(198, 51, 100, last_octet),
                40000,
            ))));
        request
    }

    #[cfg(feature = "governor")]
    #[tokio::test]
    async fn probes_are_exempt_by_default() {
        use tower::ServiceExt;
        let router = anonymous_router(RateLimitConfig {
            per_user_rpm: 1,
            ..RateLimitConfig::default()
        });
        for path in ["/health", "/ready"] {
            for n in 0..50 {
                let response = router.clone().oneshot(from_ip(path, 7)).await.unwrap();
                assert_eq!(
                    response.status(),
                    axum::http::StatusCode::OK,
                    "{path} request {n} from one IP was refused under a 1 rpm limit"
                );
            }
        }
    }

    #[cfg(feature = "governor")]
    #[tokio::test]
    async fn exempt_paths_match_exactly_and_replace_the_default() {
        use tower::ServiceExt;
        let router = anonymous_router(RateLimitConfig {
            per_user_rpm: 1,
            exempt_paths: vec!["/health".to_string()],
            ..RateLimitConfig::default()
        });
        // `/readyz` is not `/ready`, and `/ready` is no longer listed.
        let first = router.clone().oneshot(from_ip("/readyz", 8)).await.unwrap();
        assert_eq!(first.status(), axum::http::StatusCode::OK);
        let second = router.clone().oneshot(from_ip("/ready", 8)).await.unwrap();
        assert_eq!(second.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
        let health = router.clone().oneshot(from_ip("/health", 8)).await.unwrap();
        assert_eq!(health.status(), axum::http::StatusCode::OK);
    }

    #[cfg(feature = "governor")]
    #[tokio::test]
    async fn a_refusal_is_429_with_the_limiters_wait_in_retry_after() {
        use tower::ServiceExt;
        // One request a minute, burst 1: the second waits about 60 seconds.
        let router = anonymous_router(RateLimitConfig {
            anonymous_rpm: Some(1),
            anonymous_burst: Some(1),
            ..RateLimitConfig::default()
        });
        let first = router
            .clone()
            .oneshot(from_ip("/api/v1/thing", 9))
            .await
            .unwrap();
        assert_eq!(first.status(), axum::http::StatusCode::OK);
        let refused = router
            .clone()
            .oneshot(from_ip("/api/v1/thing", 9))
            .await
            .unwrap();
        assert_eq!(refused.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
        let retry_after: u64 = refused
            .headers()
            .get(axum::http::header::RETRY_AFTER)
            .expect("every 429 carries Retry-After")
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            (59..=60).contains(&retry_after),
            "Retry-After is the limiter's own wait, rounded up: {retry_after}"
        );
    }

    /// One anonymous request per address, and none back for a minute.
    #[cfg(feature = "governor")]
    fn one_request_each() -> RateLimitConfig {
        RateLimitConfig {
            anonymous_rpm: Some(1),
            anonymous_burst: Some(1),
            ..RateLimitConfig::default()
        }
    }

    #[cfg(feature = "governor")]
    #[tokio::test]
    async fn a_probe_with_a_query_string_is_still_the_probe() {
        use tower::ServiceExt;
        let router = anonymous_router(one_request_each());
        for n in 0..20 {
            let response = router
                .clone()
                .oneshot(from_ip("/ready?x=1", 20))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                axum::http::StatusCode::OK,
                "request {n}: the query string is not part of the path matched"
            );
        }
    }

    #[cfg(feature = "governor")]
    #[tokio::test]
    async fn a_trailing_slash_is_another_path() {
        use tower::ServiceExt;
        let router = anonymous_router(one_request_each());
        let first = router
            .clone()
            .oneshot(from_ip("/health/", 21))
            .await
            .unwrap();
        assert_eq!(first.status(), axum::http::StatusCode::OK);
        let second = router
            .clone()
            .oneshot(from_ip("/health/", 21))
            .await
            .unwrap();
        assert_eq!(
            second.status(),
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            "exact match: /health/ is not /health, so it is counted"
        );
    }

    #[cfg(feature = "governor")]
    #[tokio::test]
    async fn a_head_probe_is_exempt() {
        use tower::ServiceExt;
        let router = anonymous_router(one_request_each());
        for n in 0..20 {
            let mut request = from_ip("/ready", 22);
            *request.method_mut() = axum::http::Method::HEAD;
            let response = router.clone().oneshot(request).await.unwrap();
            assert_eq!(
                response.status(),
                axum::http::StatusCode::OK,
                "HEAD request {n}: the exemption is by path, whatever the method"
            );
        }
    }

    #[cfg(feature = "governor")]
    #[tokio::test]
    async fn an_exempt_request_leaves_the_bucket_untouched() {
        use tower::ServiceExt;
        let router = anonymous_router(one_request_each());
        for path in ["/ready", "/health", "/ready?x=1"] {
            for _ in 0..10 {
                let response = router.clone().oneshot(from_ip(path, 23)).await.unwrap();
                assert_eq!(response.status(), axum::http::StatusCode::OK, "{path}");
            }
        }
        let first = router
            .clone()
            .oneshot(from_ip("/api/v1/thing", 23))
            .await
            .unwrap();
        assert_eq!(
            first.status(),
            axum::http::StatusCode::OK,
            "thirty probes later, the address's one request is still there"
        );
        let second = router
            .clone()
            .oneshot(from_ip("/api/v1/thing", 23))
            .await
            .unwrap();
        assert_eq!(
            second.status(),
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            "and it was the only one"
        );
    }

    #[cfg(feature = "governor")]
    #[tokio::test]
    async fn a_sub_second_wait_is_advertised_as_one_second() {
        use tower::ServiceExt;
        // 600 a minute is one token every 100 ms: the refused request's
        // actual wait is under a second, and Retry-After rounds it up.
        let router = anonymous_router(RateLimitConfig {
            anonymous_rpm: Some(600),
            anonymous_burst: Some(1),
            ..RateLimitConfig::default()
        });
        let first = router
            .clone()
            .oneshot(from_ip("/api/v1/thing", 24))
            .await
            .unwrap();
        assert_eq!(first.status(), axum::http::StatusCode::OK);
        let refused = router
            .clone()
            .oneshot(from_ip("/api/v1/thing", 24))
            .await
            .unwrap();
        assert_eq!(refused.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            refused.headers()[axum::http::header::RETRY_AFTER],
            "1",
            "a sub-second wait is 1, never 0"
        );
    }

    /// The regression for the panic: a limit above 60 000 a minute used to
    /// make a whole-millisecond period of zero, and creating the bucket on the
    /// first request panicked.
    #[cfg(feature = "governor")]
    #[tokio::test]
    async fn a_limit_above_sixty_thousand_a_minute_serves_requests() {
        use tower::ServiceExt;
        let router = anonymous_router(RateLimitConfig {
            per_user_rpm: 120_000,
            ..RateLimitConfig::default()
        });
        for n in 0..5 {
            let response = router
                .clone()
                .oneshot(from_ip("/api/v1/thing", 25))
                .await
                .unwrap();
            assert_eq!(response.status(), axum::http::StatusCode::OK, "request {n}");
        }
    }

    #[cfg(feature = "governor")]
    #[test]
    fn the_anonymous_knob_sizes_the_per_ip_bucket_alone() {
        use std::net::{IpAddr, Ipv4Addr};
        let rl = GovernorRateLimit::new(RateLimitConfig {
            per_user_rpm: 1,
            anonymous_rpm: Some(600),
            anonymous_burst: Some(3),
            ..RateLimitConfig::default()
        });
        let ip = Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3)));
        for n in 0..3 {
            assert!(
                rl.check_rate_limit("GET", "/x", &RateKey::Anonymous(ip))
                    .is_ok(),
                "burst request {n} is inside anonymous_burst = 3"
            );
        }
        assert!(matches!(
            rl.check_rate_limit("GET", "/x", &RateKey::Anonymous(ip)),
            Err(Error::RateLimitExceeded { .. })
        ));
    }

    #[cfg(feature = "governor")]
    #[test]
    fn anonymous_quota_defaults_to_the_user_rate_and_a_tenth() {
        let config = RateLimitConfig {
            per_user_rpm: 200,
            ..RateLimitConfig::default()
        };
        assert_eq!(config.anonymous_quota(), (200, 20));
        let tiny = RateLimitConfig {
            anonymous_rpm: Some(5),
            ..RateLimitConfig::default()
        };
        assert_eq!(tiny.anonymous_quota(), (5, 1), "the burst is at least 1");
        let zero = RateLimitConfig {
            anonymous_rpm: Some(0),
            anonymous_burst: Some(0),
            ..RateLimitConfig::default()
        };
        assert_eq!(zero.anonymous_quota(), (1, 1));
    }

    #[cfg(feature = "governor")]
    #[test]
    fn limits_above_sixty_thousand_a_minute_build_a_quota() {
        // A whole-millisecond period is 0 above 60 000 rpm and was refused.
        assert_eq!(
            GovernorRateLimit::replenish_interval(120_000),
            Duration::from_micros(500)
        );
        assert!(GovernorRateLimit::replenish_interval(u32::MAX) > Duration::ZERO);
        assert_eq!(
            GovernorRateLimit::replenish_interval(0),
            Duration::from_secs(60),
            "a zero rate is one a minute, not a division by zero"
        );
        let limiter = GovernorRateLimit::create_limiter(120_000, 2);
        assert!(limiter.check().is_ok());
    }

    // ---------------------------------------------------------------------
    // Classifiers
    // ---------------------------------------------------------------------

    #[cfg(feature = "governor")]
    #[test]
    fn the_default_classifier_keeps_the_claims_buckets_and_quotas() {
        use crate::middleware::rate_key::claims_rate_key;
        use crate::middleware::Claims;
        use std::net::{IpAddr, Ipv4Addr};
        let rl = GovernorRateLimit::new(RateLimitConfig {
            per_user_rpm: 200,
            per_client_rpm: 1000,
            ..RateLimitConfig::default()
        });
        let claims = |sub: &str| Claims {
            sub: sub.to_string(),
            email: None,
            username: None,
            roles: Vec::new(),
            perms: Vec::new(),
            exp: 0,
            iat: None,
            jti: None,
            iss: None,
            aud: None,
            custom: Default::default(),
        };
        let ip = Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 4)));
        let global = |key: &str, rpm, burst| {
            Some(Bucket {
                scope: BucketScope::Global,
                key: key.to_string(),
                rpm,
                burst,
            })
        };
        // The bucket names and quotas the limiter used before classifiers.
        let cases = [
            ("user:alice", global("governor:user:user:alice", 200, 20)),
            (
                "client:svc",
                global("governor:client:client:svc", 1000, 100),
            ),
            ("robot", global("governor:unknown:robot", 200, 20)),
        ];
        for (sub, expected) in cases {
            let key = claims_rate_key(Some(&claims(sub)), ip);
            assert_eq!(rl.bucket("GET", "/x", &key), expected, "subject {sub}");
        }
        assert_eq!(
            rl.bucket("GET", "/x", &claims_rate_key(None, ip)),
            global("governor:ip:10.0.0.4", 200, 20)
        );
        assert_eq!(rl.bucket("GET", "/x", &RateKey::Exempt), None);
    }

    /// A router whose limiter counts by `classifier`, with anonymous callers
    /// and keyed callers both held to one request a minute.
    #[cfg(feature = "governor")]
    fn classified_router(classifier: impl RateClassifier) -> axum::Router {
        use axum::routing::get;
        let rate_limit = GovernorRateLimit::new(RateLimitConfig {
            per_user_rpm: 1,
            per_client_rpm: 1,
            anonymous_rpm: Some(1),
            anonymous_burst: Some(1),
            ..RateLimitConfig::default()
        })
        .with_classifier(classifier);
        axum::Router::new()
            .route("/ready", get(|| async { "ok" }))
            .route("/api/v1/thing", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(
                rate_limit,
                GovernorRateLimit::middleware,
            ))
    }

    /// A request from `198.51.100.<last_octet>` carrying `x-caller: <caller>`.
    #[cfg(feature = "governor")]
    fn calling(path: &str, last_octet: u8, caller: &str) -> Request<Body> {
        let mut request = from_ip(path, last_octet);
        request.headers_mut().insert(
            "x-caller",
            axum::http::HeaderValue::from_str(caller).unwrap(),
        );
        request
    }

    /// Exempts `operator`, keys `agent-*` per agent, and counts everyone else
    /// by address.
    #[cfg(feature = "governor")]
    fn by_caller(request: &RateRequest<'_>) -> RateKey {
        match request
            .headers()
            .get("x-caller")
            .and_then(|value| value.to_str().ok())
        {
            Some("operator") => RateKey::Exempt,
            Some(agent) if agent.starts_with("agent-") => RateKey::client(agent.to_string()),
            _ => RateKey::Anonymous(request.client_ip()),
        }
    }

    #[cfg(feature = "governor")]
    #[tokio::test]
    async fn a_classifier_can_exempt_a_caller() {
        use tower::ServiceExt;
        let router = classified_router(by_caller);
        for n in 0..5 {
            let response = router
                .clone()
                .oneshot(calling("/api/v1/thing", 20, "operator"))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                axum::http::StatusCode::OK,
                "exempt request {n} was counted"
            );
            assert!(
                response.headers().get("x-ratelimit-limit").is_none(),
                "an exempt request reports no limit"
            );
        }
    }

    #[cfg(feature = "governor")]
    #[tokio::test]
    async fn a_classifier_key_follows_the_caller_across_addresses() {
        use tower::ServiceExt;
        let router = classified_router(by_caller);
        let first = router
            .clone()
            .oneshot(calling("/api/v1/thing", 21, "agent-a"))
            .await
            .unwrap();
        assert_eq!(first.status(), axum::http::StatusCode::OK);
        // Same agent, another address: the same bucket, now empty.
        let moved = router
            .clone()
            .oneshot(calling("/api/v1/thing", 22, "agent-a"))
            .await
            .unwrap();
        assert_eq!(moved.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
        assert!(moved
            .headers()
            .contains_key(axum::http::header::RETRY_AFTER));
        // Another agent behind the first agent's address: its own bucket.
        let neighbor = router
            .clone()
            .oneshot(calling("/api/v1/thing", 21, "agent-b"))
            .await
            .unwrap();
        assert_eq!(neighbor.status(), axum::http::StatusCode::OK);
    }

    #[cfg(feature = "governor")]
    #[tokio::test]
    async fn a_classifier_anonymous_answer_is_counted_by_address() {
        use tower::ServiceExt;
        let router = classified_router(by_caller);
        // Two made-up callers from one address share that address's bucket.
        let first = router
            .clone()
            .oneshot(calling("/api/v1/thing", 23, "nobody-1"))
            .await
            .unwrap();
        assert_eq!(first.status(), axum::http::StatusCode::OK);
        let second = router
            .clone()
            .oneshot(calling("/api/v1/thing", 23, "nobody-2"))
            .await
            .unwrap();
        assert_eq!(second.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
        let elsewhere = router
            .clone()
            .oneshot(calling("/api/v1/thing", 24, "nobody-3"))
            .await
            .unwrap();
        assert_eq!(elsewhere.status(), axum::http::StatusCode::OK);
    }

    #[cfg(feature = "governor")]
    #[tokio::test]
    async fn exempt_paths_apply_before_the_classifier() {
        use tower::ServiceExt;
        // A classifier that puts every request in one bucket cannot count probes.
        let router = classified_router(|_: &RateRequest<'_>| RateKey::user("everyone"));
        for n in 0..50 {
            let response = router
                .clone()
                .oneshot(calling("/ready", 25, "agent-a"))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                axum::http::StatusCode::OK,
                "probe {n} was counted"
            );
        }
    }
}
