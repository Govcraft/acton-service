//! Redis-backed rate limiting middleware
//!
//! Provides distributed rate limiting with per-route configuration support.
//! Uses Redis for shared state across multiple service instances.

#[cfg(feature = "cache")]
use deadpool_redis::Pool as RedisPool;
#[cfg(feature = "cache")]
use std::ops::DerefMut;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

#[cfg(feature = "cache")]
use axum::http::{header::HeaderValue, HeaderName};

use crate::{config::RateLimitConfig, error::Error};

#[cfg(feature = "cache")]
use super::rate_key::{RateClass, RateClassifier, RateKey, RateRequest, SharedClassifier};

use super::route_matcher::CompiledRoutePatterns;

#[cfg(feature = "cache")]
use super::route_matcher::normalize_path;

#[cfg(feature = "cache")]
use tracing::{debug, warn};

/// Rate limiting middleware state
///
/// Provides Redis-backed distributed rate limiting with support for:
/// - Global per-user and per-client limits
/// - Per-route rate limit overrides
/// - Automatic path normalization for dynamic segments
#[derive(Clone)]
pub struct RateLimit {
    #[cfg_attr(not(feature = "cache"), allow(dead_code))]
    config: RateLimitConfig,
    #[cfg_attr(not(feature = "cache"), allow(dead_code))]
    route_patterns: Arc<CompiledRoutePatterns>,
    #[cfg(feature = "cache")]
    redis_pool: Option<RedisPool>,
    #[cfg(feature = "cache")]
    classifier: SharedClassifier,
}

/// Rate limit check result containing limit info for response headers
#[cfg(feature = "cache")]
struct RateLimitResult {
    /// Maximum requests allowed in window
    limit: u32,
    /// Current request count (after this request)
    count: u32,
    /// Seconds until window resets
    reset_secs: u64,
}

impl RateLimit {
    /// Create a new rate limiting middleware with Redis backend
    #[cfg(feature = "cache")]
    pub fn new(config: RateLimitConfig, redis_pool: RedisPool) -> Self {
        let route_patterns = CompiledRoutePatterns::compile(&config.routes);
        Self {
            config,
            route_patterns: Arc::new(route_patterns),
            redis_pool: Some(redis_pool),
            classifier: SharedClassifier::default(),
        }
    }

    /// Create a new rate limiting middleware without Redis (for testing)
    #[cfg(not(feature = "cache"))]
    pub fn new(config: RateLimitConfig) -> Self {
        let route_patterns = CompiledRoutePatterns::compile(&config.routes);
        Self {
            config,
            route_patterns: Arc::new(route_patterns),
        }
    }

    /// Counts requests by `classifier` instead of the default
    /// [`ClaimsClassifier`](super::rate_key::ClaimsClassifier).
    #[cfg(feature = "cache")]
    pub fn with_classifier(mut self, classifier: impl RateClassifier) -> Self {
        self.classifier = SharedClassifier::new(classifier);
        self
    }

    /// Middleware function to enforce rate limits
    ///
    /// Paths on [`RateLimitConfig::exempt_paths`] are never counted. Every
    /// other request is classified (see [`with_classifier`](Self::with_classifier)),
    /// and then checked in this order:
    /// 1. Per-route limits (if configured for the request path)
    /// 2. The keyed bucket's per-user or per-client limit
    ///
    /// An anonymous request that matches no per-route limit is not counted.
    pub async fn middleware(
        #[cfg_attr(not(feature = "cache"), allow(unused_variables))] State(rate_limit): State<Self>,
        request: Request<Body>,
        next: Next,
    ) -> Result<Response, Error> {
        #[cfg(feature = "cache")]
        {
            let method = request.method().as_str().to_string();
            let path = request.uri().path().to_string();

            // Probes and other configured paths are never counted, whatever
            // the classifier would say.
            if rate_limit.config.is_exempt_path(&path) {
                return Ok(next.run(request).await);
            }

            let connect_info =
                super::request_context::connect_info_remote_addr(request.extensions());
            let client_ip = super::request_context::extract_client_ip(
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
            if rate_key == RateKey::Exempt {
                return Ok(next.run(request).await);
            }

            #[cfg(feature = "audit")]
            let audit_logger = request
                .extensions()
                .get::<crate::audit::AuditLogger>()
                .cloned();
            #[cfg(feature = "audit")]
            let audit_source = {
                let mut source = super::request_context::audit_source_for_request(&request);
                source.subject = request
                    .extensions()
                    .get::<crate::middleware::Claims>()
                    .map(|c| c.sub.clone());
                source
            };

            // Check rate limit and get result for headers
            let result = match rate_limit
                .check_rate_limit_with_route(&method, &path, &rate_key)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    #[cfg(feature = "audit")]
                    if matches!(e, Error::RateLimitExceeded { .. }) {
                        if let Some(ref logger) = audit_logger {
                            if logger.config().audit_auth_events {
                                logger
                                    .log(
                                        crate::audit::event::AuditEvent::new(
                                            crate::audit::event::AuditEventKind::HttpRequestDenied,
                                            crate::audit::event::AuditSeverity::Warning,
                                            logger.service_name().to_string(),
                                        )
                                        .with_source(audit_source)
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

        #[cfg(not(feature = "cache"))]
        Ok(next.run(request).await)
    }

    /// Check rate limit considering per-route configuration
    #[cfg(feature = "cache")]
    async fn check_rate_limit_with_route(
        &self,
        method: &str,
        path: &str,
        rate_key: &RateKey,
    ) -> Result<RateLimitResult, Error> {
        let normalized_path = normalize_path(path);

        // Check if there's a route-specific rate limit
        if let Some(route_config) = self.route_patterns.match_route(method, &normalized_path) {
            debug!(
                "Using per-route rate limit for {} {}: {} rpm",
                method, normalized_path, route_config.requests_per_minute
            );

            let key = match rate_key {
                // Per-caller route limit
                RateKey::Key { id, .. } if route_config.per_user => {
                    format!("route:{}:{}", normalized_path, id)
                }
                // Global route limit (shared across all callers), which is
                // also where an anonymous caller of a per-caller route counts
                _ => format!("route:{}:global", normalized_path),
            };

            return self
                .check_and_increment(
                    &key,
                    route_config.requests_per_minute,
                    self.config.window_secs,
                )
                .await;
        }

        // Fall back to the keyed caller's global limit
        if let RateKey::Key { id, class } = rate_key {
            let limit = match class {
                RateClass::User => self.config.per_user_rpm,
                RateClass::Client => self.config.per_client_rpm,
            };
            return self
                .check_and_increment(&format!("ratelimit:{}", id), limit, self.config.window_secs)
                .await;
        }

        // Anonymous and no route-specific limit - allow the request
        warn!("Rate limit middleware called without a keyed caller and no route-specific limit");
        Ok(RateLimitResult {
            limit: self.config.per_user_rpm,
            count: 0,
            reset_secs: self.config.window_secs,
        })
    }

    /// Check and increment rate limit counter in Redis
    #[cfg(feature = "cache")]
    async fn check_and_increment(
        &self,
        key: &str,
        limit: u32,
        window_secs: u64,
    ) -> Result<RateLimitResult, Error> {
        let redis_pool = self
            .redis_pool
            .as_ref()
            .ok_or_else(|| Error::Internal("Redis pool not configured".to_string()))?;

        let mut conn = redis_pool.get().await.map_err(|e| {
            let redis_err = redis::RedisError::from((
                redis::ErrorKind::IoError,
                "Failed to get Redis connection",
                e.to_string(),
            ));
            Error::Redis(Box::new(redis_err))
        })?;

        // Use INCR and EXPIRE for simple rate limiting
        // In production, you might want to use a more sophisticated algorithm (sliding window, token bucket)
        let count: u32 = redis::cmd("INCR")
            .arg(key)
            .query_async(conn.deref_mut())
            .await?;

        // Set expiration on first request
        if count == 1 {
            let _: () = redis::cmd("EXPIRE")
                .arg(key)
                .arg(window_secs as i64)
                .query_async(conn.deref_mut())
                .await?;
        }

        // Get TTL for reset time
        let ttl: i64 = redis::cmd("TTL")
            .arg(key)
            .query_async(conn.deref_mut())
            .await
            .unwrap_or(window_secs as i64);

        let reset_secs = if ttl > 0 { ttl as u64 } else { window_secs };

        let verdict = window_verdict(count, limit, reset_secs);
        if verdict.is_err() {
            warn!(
                "Rate limit exceeded for {}: {} requests (limit: {})",
                key, count, limit
            );
        }
        verdict.map(|()| RateLimitResult {
            limit,
            count,
            reset_secs,
        })
    }

    /// Add rate limit headers to response
    #[cfg(feature = "cache")]
    fn add_rate_limit_headers(response: &mut Response, result: &RateLimitResult) {
        let headers = response.headers_mut();

        // Standard rate limit headers
        if let Ok(value) = HeaderValue::from_str(&result.limit.to_string()) {
            headers.insert(HeaderName::from_static("x-ratelimit-limit"), value);
        }

        let remaining = result.limit.saturating_sub(result.count);
        if let Ok(value) = HeaderValue::from_str(&remaining.to_string()) {
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
}

/// Whether the `count`th request in a fixed window that resets in
/// `reset_secs` is inside `limit`. A refusal waits for the window to reset,
/// and never advertises a zero wait.
#[cfg(any(feature = "cache", test))]
fn window_verdict(count: u32, limit: u32, reset_secs: u64) -> Result<(), Error> {
    if count > limit {
        Err(Error::RateLimitExceeded {
            retry_after_secs: reset_secs.max(1),
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{window_verdict, Error};
    #[cfg(not(feature = "cache"))]
    use super::{RateLimit, RateLimitConfig};
    use axum::response::IntoResponse;

    #[test]
    fn a_redis_window_refusal_is_429_with_the_windows_reset_in_retry_after() {
        assert!(
            window_verdict(10, 10, 42).is_ok(),
            "the limit itself is inside"
        );
        let refused = window_verdict(11, 10, 42).expect_err("one past the limit");
        assert!(matches!(
            refused,
            Error::RateLimitExceeded {
                retry_after_secs: 42
            }
        ));
        let response = refused.into_response();
        assert_eq!(response.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response.headers()[axum::http::header::RETRY_AFTER],
            "42",
            "the wait is the window's reset"
        );
        assert!(
            matches!(
                window_verdict(11, 10, 0),
                Err(Error::RateLimitExceeded {
                    retry_after_secs: 1
                })
            ),
            "an expiring window still advertises a wait of at least one second"
        );
    }

    #[test]
    fn test_rate_limit_creation() {
        #[cfg(not(feature = "cache"))]
        {
            let config = RateLimitConfig {
                per_user_rpm: 200,
                per_client_rpm: 1000,
                window_secs: 60,
                routes: std::collections::HashMap::new(),
                auto_apply: true,
                trust_forwarded_headers: false,
                ..RateLimitConfig::default()
            };
            let _rate_limit = RateLimit::new(config);
        }

        // With cache feature, we would need a Redis pool
    }

    #[test]
    fn test_rate_limit_with_routes() {
        #[cfg(not(feature = "cache"))]
        {
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
                auto_apply: true,
                trust_forwarded_headers: false,
                ..RateLimitConfig::default()
            };
            let rate_limit = RateLimit::new(config);

            // Verify route patterns were compiled
            assert!(!rate_limit.route_patterns.is_empty());
        }
    }
}
