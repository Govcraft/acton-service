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
    extract::{Request, State},
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

#[cfg(feature = "governor")]
use crate::config::RateLimitConfig;
#[cfg(feature = "governor")]
use crate::error::Error;
#[cfg(feature = "governor")]
use crate::middleware::{normalize_path, Claims, CompiledRoutePatterns};

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

    /// Get retry-after header value in seconds
    pub fn retry_after_secs(&self) -> u64 {
        self.retry_after.as_secs()
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
    /// Global rate limiters, keyed by user/client ID
    global_limiters: Arc<DashMap<String, Arc<GovernorLimiter>>>,
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
        }
    }

    /// Middleware function to enforce rate limits
    ///
    /// Checks rate limits in the following order:
    /// 1. Per-route limits (if configured for the request path)
    /// 2. Global per-user limits (if JWT claims present)
    /// 3. Global per-client limits (if client token)
    pub async fn middleware(
        State(rate_limit): State<Self>,
        request: Request<Body>,
        next: Next,
    ) -> Result<Response, Error> {
        let method = request.method().as_str();
        let path = request.uri().path();
        let claims = request.extensions().get::<Claims>().cloned();

        // Check rate limit and get result for headers
        let result = rate_limit.check_rate_limit(method, path, claims.as_ref())?;

        // Run the request
        let mut response = next.run(request).await;

        // Add rate limit headers to response
        Self::add_rate_limit_headers(&mut response, &result);

        Ok(response)
    }

    /// Check rate limit considering per-route configuration
    fn check_rate_limit(
        &self,
        method: &str,
        path: &str,
        claims: Option<&Claims>,
    ) -> Result<GovernorRateLimitResult, Error> {
        let normalized_path = normalize_path(path);

        // Check if there's a route-specific rate limit
        if let Some(route_config) = self.route_patterns.match_route(method, &normalized_path) {
            debug!(
                "Using per-route governor limit for {} {}: {} rpm",
                method, normalized_path, route_config.requests_per_minute
            );

            let key = if route_config.per_user {
                // Per-user route limit
                if let Some(claims) = claims {
                    format!("route:{}:user:{}", normalized_path, claims.sub)
                } else {
                    // No claims, use global route limit
                    format!("route:{}:global", normalized_path)
                }
            } else {
                // Global route limit (shared across all users)
                format!("route:{}:global", normalized_path)
            };

            return self.check_with_limiter(
                &self.route_limiters,
                &key,
                route_config.requests_per_minute,
                route_config.burst_size,
            );
        }

        // Fall back to global user/client limits
        if let Some(claims) = claims {
            let (key, limit) = if claims.is_user() {
                (
                    format!("governor:user:{}", claims.sub),
                    self.config.per_user_rpm,
                )
            } else if claims.is_client() {
                (
                    format!("governor:client:{}", claims.sub),
                    self.config.per_client_rpm,
                )
            } else {
                // Default to user limit
                (
                    format!("governor:unknown:{}", claims.sub),
                    self.config.per_user_rpm,
                )
            };

            // Calculate burst size (10% of limit, minimum 1)
            let burst_size = (limit / 10).max(1);

            return self.check_with_limiter(&self.global_limiters, &key, limit, burst_size);
        }

        // No claims and no route-specific limit - allow the request
        warn!("Governor rate limit called without JWT claims and no route-specific limit");
        Ok(GovernorRateLimitResult {
            limit: self.config.per_user_rpm,
            remaining: self.config.per_user_rpm,
            reset_secs: self.config.window_secs,
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
            .or_insert_with(|| {
                Arc::new(Self::create_limiter(requests_per_minute, burst_size))
            })
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

                Err(Error::RateLimitExceeded)
            }
        }
    }

    /// Create a new rate limiter with the given configuration
    fn create_limiter(requests_per_minute: u32, burst_size: u32) -> GovernorLimiter {
        // Calculate replenishment interval: how often to add one token
        // For 60 RPM: 60000ms / 60 = 1000ms per token
        let replenish_interval_ms = 60_000u64 / (requests_per_minute as u64).max(1);

        // Create quota with burst capacity
        let burst = NonZeroU32::new(burst_size.max(1)).unwrap();
        let quota = Quota::with_period(Duration::from_millis(replenish_interval_ms))
            .expect("Replenish interval should be valid")
            .allow_burst(burst);

        RateLimiter::direct(quota)
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
        let exceeded = RateLimitExceeded::new(Duration::from_secs(30), 100, Duration::from_secs(60));

        assert_eq!(exceeded.retry_after_secs(), 30);
        assert_eq!(exceeded.limit, 100);
        assert_eq!(exceeded.period, Duration::from_secs(60));
    }

    #[cfg(feature = "governor")]
    #[test]
    fn test_governor_rate_limit_creation() {
        use std::collections::HashMap;
        let config = RateLimitConfig {
            per_user_rpm: 200,
            per_client_rpm: 1000,
            window_secs: 60,
            routes: HashMap::new(),
        };
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
            per_user_rpm: 200,
            per_client_rpm: 1000,
            window_secs: 60,
            routes,
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
}
