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
use crate::middleware::Claims;

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

    /// Middleware function to enforce rate limits
    ///
    /// Checks rate limits in the following order:
    /// 1. Per-route limits (if configured for the request path)
    /// 2. Global per-user limits (if JWT claims present)
    /// 3. Global per-client limits (if client token)
    pub async fn middleware(
        #[cfg_attr(not(feature = "cache"), allow(unused_variables))]
        State(rate_limit): State<Self>,
        request: Request<Body>,
        next: Next,
    ) -> Result<Response, Error> {
        #[cfg(feature = "cache")]
        {
            let method = request.method().as_str();
            let path = request.uri().path();
            let claims = request.extensions().get::<Claims>().cloned();

            // Check rate limit and get result for headers
            let result = rate_limit
                .check_rate_limit_with_route(method, path, claims.as_ref())
                .await?;

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
        claims: Option<&Claims>,
    ) -> Result<RateLimitResult, Error> {
        let normalized_path = normalize_path(path);

        // Check if there's a route-specific rate limit
        if let Some(route_config) = self.route_patterns.match_route(method, &normalized_path) {
            debug!(
                "Using per-route rate limit for {} {}: {} rpm",
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

            return self
                .check_and_increment(&key, route_config.requests_per_minute, self.config.window_secs)
                .await;
        }

        // Fall back to global user/client limits
        if let Some(claims) = claims {
            let (key, limit) = if claims.is_user() {
                (
                    format!("ratelimit:user:{}", claims.sub),
                    self.config.per_user_rpm,
                )
            } else if claims.is_client() {
                (
                    format!("ratelimit:client:{}", claims.sub),
                    self.config.per_client_rpm,
                )
            } else {
                // Default to user limit
                (
                    format!("ratelimit:unknown:{}", claims.sub),
                    self.config.per_user_rpm,
                )
            };

            return self
                .check_and_increment(&key, limit, self.config.window_secs)
                .await;
        }

        // No claims and no route-specific limit - allow the request
        // In production, you might want to add IP-based limiting here
        warn!("Rate limit middleware called without JWT claims and no route-specific limit");
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

        // Check if limit exceeded
        if count > limit {
            warn!(
                "Rate limit exceeded for {}: {} requests (limit: {})",
                key, count, limit
            );
            return Err(Error::RateLimitExceeded);
        }

        Ok(RateLimitResult {
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
            headers.insert(
                HeaderName::from_static("x-ratelimit-limit"),
                value,
            );
        }

        let remaining = result.limit.saturating_sub(result.count);
        if let Ok(value) = HeaderValue::from_str(&remaining.to_string()) {
            headers.insert(
                HeaderName::from_static("x-ratelimit-remaining"),
                value,
            );
        }

        // Calculate reset timestamp
        let reset_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() + result.reset_secs)
            .unwrap_or(0);

        if let Ok(value) = HeaderValue::from_str(&reset_timestamp.to_string()) {
            headers.insert(
                HeaderName::from_static("x-ratelimit-reset"),
                value,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(not(feature = "cache"))]
    use super::{RateLimit, RateLimitConfig};

    #[test]
    fn test_rate_limit_creation() {
        #[cfg(not(feature = "cache"))]
        {
            let config = RateLimitConfig {
                per_user_rpm: 200,
                per_client_rpm: 1000,
                window_secs: 60,
                routes: std::collections::HashMap::new(),
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
            };
            let rate_limit = RateLimit::new(config);

            // Verify route patterns were compiled
            assert!(!rate_limit.route_patterns.is_empty());
        }
    }
}
