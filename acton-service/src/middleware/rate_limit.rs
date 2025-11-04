//! Redis-backed rate limiting middleware

#[cfg(feature = "cache")]
use deadpool_redis::Pool as RedisPool;
#[cfg(feature = "cache")]
use std::ops::DerefMut;

use axum::{
    body::Body,
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

use crate::{config::RateLimitConfig, error::Error};

#[cfg(feature = "cache")]
use crate::middleware::Claims;

#[cfg(feature = "cache")]
use tracing::warn;

/// Rate limiting middleware state
#[derive(Clone)]
pub struct RateLimit {
    #[allow(dead_code)]
    config: RateLimitConfig,
    #[cfg(feature = "cache")]
    redis_pool: Option<RedisPool>,
}

impl RateLimit {
    /// Create a new rate limiting middleware
    #[cfg(feature = "cache")]
    pub fn new(config: RateLimitConfig, redis_pool: RedisPool) -> Self {
        Self {
            config,
            redis_pool: Some(redis_pool),
        }
    }

    /// Create a new rate limiting middleware without Redis (for testing)
    #[cfg(not(feature = "cache"))]
    pub fn new(config: RateLimitConfig) -> Self {
        Self { config }
    }

    /// Middleware function to enforce rate limits
    pub async fn middleware(
        #[cfg_attr(not(feature = "cache"), allow(unused_variables))]
        State(rate_limit): State<Self>,
        request: Request<Body>,
        next: Next,
    ) -> Result<Response, Error> {
        #[cfg(feature = "cache")]
        {
            // Extract claims if present
            let claims = request.extensions().get::<Claims>().cloned();

            if let Some(claims) = claims {
                // Check rate limit
                rate_limit.check_rate_limit(&claims).await?;
            } else {
                warn!("Rate limit middleware called without JWT claims");
            }
        }

        Ok(next.run(request).await)
    }

    /// Check rate limit for a user or client
    #[cfg(feature = "cache")]
    async fn check_rate_limit(&self, claims: &Claims) -> Result<(), Error> {
        let redis_pool = self
            .redis_pool
            .as_ref()
            .ok_or_else(|| Error::Internal("Redis pool not configured".to_string()))?;

        let mut conn = redis_pool
            .get()
            .await
            .map_err(|e| {
                let redis_err = redis::RedisError::from((
                    redis::ErrorKind::IoError,
                    "Failed to get Redis connection",
                    e.to_string()
                ));
                Error::Redis(Box::new(redis_err))
            })?;

        // Determine if this is a user or client token
        let (key, limit) = if claims.is_user() {
            (format!("ratelimit:user:{}", claims.sub), self.config.per_user_rpm)
        } else if claims.is_client() {
            (format!("ratelimit:client:{}", claims.sub), self.config.per_client_rpm)
        } else {
            // Default to user limit
            (format!("ratelimit:unknown:{}", claims.sub), self.config.per_user_rpm)
        };

        // Use INCR and EXPIRE for simple rate limiting
        // In production, you might want to use a more sophisticated algorithm (sliding window, token bucket)
        let count: u32 = redis::cmd("INCR")
            .arg(&key)
            .query_async(conn.deref_mut())
            .await?;

        // Set expiration on first request
        if count == 1 {
            let _: () = redis::cmd("EXPIRE")
                .arg(&key)
                .arg(self.config.window_secs as i64)
                .query_async(conn.deref_mut())
                .await?;
        }

        // Check if limit exceeded
        if count > limit {
            warn!(
                "Rate limit exceeded for {}: {} requests in {} seconds (limit: {})",
                claims.sub, count, self.config.window_secs, limit
            );
            return Err(Error::RateLimitExceeded);
        }

        Ok(())
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
            };
            let _rate_limit = RateLimit::new(config);
        }

        // With cache feature, we would need a Redis pool
    }
}
