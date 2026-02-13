//! Login lockout service
//!
//! Core service for tracking failed login attempts, enforcing progressive
//! delays, and locking accounts after repeated failures. Uses Redis for
//! distributed state, following the same patterns as [`RateLimit`](crate::middleware::RateLimit).

use std::ops::DerefMut;
use std::sync::Arc;

use deadpool_redis::Pool as RedisPool;
use tracing::{debug, info, warn};

use super::config::LockoutConfig;
use super::notification::{LockoutEvent, LockoutNotification, UnlockReason};
use crate::error::{Error, Result};

/// Status of a login lockout check
///
/// Returned by [`LoginLockout::check`] and [`LoginLockout::record_failure`]
/// to inform the caller about the current lockout state.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LockoutStatus {
    /// Whether the account is currently locked
    pub locked: bool,
    /// Number of failed attempts in the current window
    pub attempt_count: u32,
    /// Maximum attempts allowed before lockout
    pub max_attempts: u32,
    /// Seconds remaining until lockout expires (0 if not locked)
    pub lockout_remaining_secs: u64,
    /// Recommended delay in milliseconds before responding (0 if no delay)
    pub delay_ms: u64,
}

/// Login lockout service
///
/// Tracks failed login attempts per identity in Redis and enforces
/// progressive delays and account lockout. Construct once at startup
/// and share via axum `State` or `Extension`.
///
/// # Example
///
/// ```rust,ignore
/// let lockout = LoginLockout::new(lockout_config, redis_pool);
///
/// // In your login handler:
/// let status = lockout.check(&email).await?;
/// if status.locked {
///     return Err(Error::AccountLocked {
///         message: format!("Try again in {} seconds", status.lockout_remaining_secs),
///         retry_after_secs: status.lockout_remaining_secs,
///     });
/// }
/// ```
#[derive(Clone)]
pub struct LoginLockout {
    config: LockoutConfig,
    redis_pool: RedisPool,
    notifications: Vec<Arc<dyn LockoutNotification>>,
}

impl LoginLockout {
    /// Create a new login lockout service
    pub fn new(config: LockoutConfig, redis_pool: RedisPool) -> Self {
        Self {
            config,
            redis_pool,
            notifications: Vec::new(),
        }
    }

    /// Register a notification handler for lockout events
    ///
    /// Multiple handlers can be registered. Events are dispatched
    /// via `tokio::spawn` (fire-and-forget).
    pub fn with_notification(mut self, handler: Arc<dyn LockoutNotification>) -> Self {
        self.notifications.push(handler);
        self
    }

    /// Register an audit logger as a lockout notification handler
    ///
    /// Convenience method that wraps the audit logger in an
    /// `AuditLockoutNotification` and registers it.
    #[cfg(feature = "audit")]
    pub fn with_audit(self, audit_logger: crate::audit::AuditLogger) -> Self {
        let handler = Arc::new(super::AuditLockoutNotification::new(audit_logger));
        self.with_notification(handler)
    }

    /// Check the lockout status for an identity without recording a failure
    ///
    /// Returns the current lockout status. If the account is locked,
    /// `status.locked` will be `true` and `status.lockout_remaining_secs`
    /// will indicate how long until the lock expires.
    pub async fn check(&self, identity: &str) -> Result<LockoutStatus> {
        if !self.config.enabled {
            return Ok(LockoutStatus {
                locked: false,
                attempt_count: 0,
                max_attempts: self.config.max_attempts,
                lockout_remaining_secs: 0,
                delay_ms: 0,
            });
        }

        let mut conn = self.get_connection().await?;

        // Check if locked
        let locked_key = self.locked_key(identity);
        let locked_ttl: i64 = redis::cmd("TTL")
            .arg(&locked_key)
            .query_async(conn.deref_mut())
            .await
            .unwrap_or(-2);

        if locked_ttl > 0 {
            // Account is locked
            let attempt_count = self.get_attempt_count(identity).await.unwrap_or(0);
            return Ok(LockoutStatus {
                locked: true,
                attempt_count,
                max_attempts: self.config.max_attempts,
                lockout_remaining_secs: locked_ttl as u64,
                delay_ms: 0,
            });
        }

        // Not locked — get current attempt count
        let attempt_count = self.get_attempt_count(identity).await.unwrap_or(0);
        let delay_ms = self.compute_delay(attempt_count);

        Ok(LockoutStatus {
            locked: false,
            attempt_count,
            max_attempts: self.config.max_attempts,
            lockout_remaining_secs: 0,
            delay_ms,
        })
    }

    /// Record a failed login attempt for an identity
    ///
    /// Increments the failure counter and, if the threshold is reached,
    /// locks the account. Returns the updated lockout status.
    ///
    /// Fires notification events for:
    /// - Every failed attempt
    /// - When the warning threshold is reached
    /// - When the account is locked
    pub async fn record_failure(&self, identity: &str) -> Result<LockoutStatus> {
        if !self.config.enabled {
            return Ok(LockoutStatus {
                locked: false,
                attempt_count: 0,
                max_attempts: self.config.max_attempts,
                lockout_remaining_secs: 0,
                delay_ms: 0,
            });
        }

        let mut conn = self.get_connection().await?;

        // Increment attempt counter
        let attempts_key = self.attempts_key(identity);
        let count: u32 = redis::cmd("INCR")
            .arg(&attempts_key)
            .query_async(conn.deref_mut())
            .await?;

        // Set expiration on first attempt
        if count == 1 {
            let _: () = redis::cmd("EXPIRE")
                .arg(&attempts_key)
                .arg(self.config.window_secs as i64)
                .query_async(conn.deref_mut())
                .await?;
        }

        debug!(
            identity = identity,
            attempt_count = count,
            max_attempts = self.config.max_attempts,
            "Login failure recorded"
        );

        // Fire failed attempt notification
        self.notify(LockoutEvent::FailedAttempt {
            identity: identity.to_string(),
            attempt_count: count,
            max_attempts: self.config.max_attempts,
        });

        // Check warning threshold
        if self.config.warning_threshold > 0
            && count == self.config.warning_threshold
            && count < self.config.max_attempts
        {
            let remaining = self.config.max_attempts - count;
            self.notify(LockoutEvent::ApproachingThreshold {
                identity: identity.to_string(),
                attempt_count: count,
                remaining_attempts: remaining,
            });
        }

        // Check if we should lock the account
        if count >= self.config.max_attempts {
            let locked_key = self.locked_key(identity);
            let _: () = redis::cmd("SET")
                .arg(&locked_key)
                .arg(chrono::Utc::now().timestamp().to_string())
                .arg("EX")
                .arg(self.config.lockout_duration_secs as i64)
                .query_async(conn.deref_mut())
                .await?;

            warn!(
                identity = identity,
                attempt_count = count,
                lockout_duration_secs = self.config.lockout_duration_secs,
                "Account locked due to repeated login failures"
            );

            self.notify(LockoutEvent::AccountLocked {
                identity: identity.to_string(),
                attempt_count: count,
                lockout_duration_secs: self.config.lockout_duration_secs,
            });

            return Ok(LockoutStatus {
                locked: true,
                attempt_count: count,
                max_attempts: self.config.max_attempts,
                lockout_remaining_secs: self.config.lockout_duration_secs,
                delay_ms: 0,
            });
        }

        let delay_ms = self.compute_delay(count);

        Ok(LockoutStatus {
            locked: false,
            attempt_count: count,
            max_attempts: self.config.max_attempts,
            lockout_remaining_secs: 0,
            delay_ms,
        })
    }

    /// Record a successful login, clearing all lockout state
    ///
    /// Removes both the attempt counter and any active lockout for the identity.
    pub async fn record_success(&self, identity: &str) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        let mut conn = self.get_connection().await?;

        let attempts_key = self.attempts_key(identity);
        let locked_key = self.locked_key(identity);

        // Check if there was an active lock before clearing
        let was_locked: bool = redis::cmd("EXISTS")
            .arg(&locked_key)
            .query_async(conn.deref_mut())
            .await
            .unwrap_or(false);

        // Delete both keys
        let _: () = redis::cmd("DEL")
            .arg(&attempts_key)
            .arg(&locked_key)
            .query_async(conn.deref_mut())
            .await?;

        if was_locked {
            info!(identity = identity, "Account unlocked via successful login");
            self.notify(LockoutEvent::AccountUnlocked {
                identity: identity.to_string(),
                reason: UnlockReason::SuccessfulLogin,
            });
        }

        Ok(())
    }

    /// Manually unlock an account (admin action)
    ///
    /// Removes the lockout flag and resets the attempt counter.
    pub async fn unlock(&self, identity: &str) -> Result<()> {
        let mut conn = self.get_connection().await?;

        let attempts_key = self.attempts_key(identity);
        let locked_key = self.locked_key(identity);

        let _: () = redis::cmd("DEL")
            .arg(&attempts_key)
            .arg(&locked_key)
            .query_async(conn.deref_mut())
            .await?;

        info!(identity = identity, "Account manually unlocked (admin)");

        self.notify(LockoutEvent::AccountUnlocked {
            identity: identity.to_string(),
            reason: UnlockReason::AdminAction,
        });

        Ok(())
    }

    /// Compute the progressive delay for a given attempt count
    ///
    /// Returns 0 if progressive delay is disabled or attempt count is 0.
    /// Formula: `min(base_ms * multiplier^(attempts-1), max_ms)`
    fn compute_delay(&self, attempt_count: u32) -> u64 {
        if !self.config.progressive_delay_enabled || attempt_count == 0 {
            return 0;
        }

        let exponent = (attempt_count - 1) as f64;
        let delay = self.config.base_delay_ms as f64 * self.config.delay_multiplier.powf(exponent);

        // Cap at max_delay_ms, handling potential infinity/NaN from large exponents
        let capped = if delay.is_finite() {
            (delay as u64).min(self.config.max_delay_ms)
        } else {
            self.config.max_delay_ms
        };

        capped
    }

    /// Get a Redis connection from the pool
    async fn get_connection(&self) -> Result<deadpool_redis::Connection> {
        self.redis_pool.get().await.map_err(|e| {
            let redis_err = redis::RedisError::from((
                redis::ErrorKind::IoError,
                "Failed to get Redis connection for lockout",
                e.to_string(),
            ));
            Error::Redis(Box::new(redis_err))
        })
    }

    /// Get the current attempt count for an identity
    async fn get_attempt_count(&self, identity: &str) -> Result<u32> {
        let mut conn = self.get_connection().await?;
        let attempts_key = self.attempts_key(identity);
        let count: Option<u32> = redis::cmd("GET")
            .arg(&attempts_key)
            .query_async(conn.deref_mut())
            .await?;
        Ok(count.unwrap_or(0))
    }

    /// Build the Redis key for attempt counter
    fn attempts_key(&self, identity: &str) -> String {
        format!("{}:attempts:{}", self.config.key_prefix, identity)
    }

    /// Build the Redis key for lockout flag
    fn locked_key(&self, identity: &str) -> String {
        format!("{}:locked:{}", self.config.key_prefix, identity)
    }

    /// Dispatch a notification event to all registered handlers
    fn notify(&self, event: LockoutEvent) {
        for handler in &self.notifications {
            let handler = Arc::clone(handler);
            let event = event.clone();
            tokio::spawn(async move {
                handler.on_event(event).await;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a LoginLockout with default config for testing compute_delay
    fn test_lockout() -> LockoutConfig {
        LockoutConfig::default()
    }

    #[test]
    fn test_compute_delay_zero_attempts() {
        let config = test_lockout();
        let lockout = LoginLockout {
            config,
            redis_pool: create_dummy_pool(),
            notifications: Vec::new(),
        };
        assert_eq!(lockout.compute_delay(0), 0);
    }

    #[test]
    fn test_compute_delay_first_attempt() {
        let config = test_lockout();
        let lockout = LoginLockout {
            config,
            redis_pool: create_dummy_pool(),
            notifications: Vec::new(),
        };
        // attempt 1: base * 2^0 = 1000 * 1 = 1000
        assert_eq!(lockout.compute_delay(1), 1000);
    }

    #[test]
    fn test_compute_delay_progressive() {
        let config = test_lockout();
        let lockout = LoginLockout {
            config,
            redis_pool: create_dummy_pool(),
            notifications: Vec::new(),
        };
        // attempt 2: 1000 * 2^1 = 2000
        assert_eq!(lockout.compute_delay(2), 2000);
        // attempt 3: 1000 * 2^2 = 4000
        assert_eq!(lockout.compute_delay(3), 4000);
        // attempt 4: 1000 * 2^3 = 8000
        assert_eq!(lockout.compute_delay(4), 8000);
        // attempt 5: 1000 * 2^4 = 16000
        assert_eq!(lockout.compute_delay(5), 16000);
    }

    #[test]
    fn test_compute_delay_caps_at_max() {
        let config = test_lockout();
        let lockout = LoginLockout {
            config,
            redis_pool: create_dummy_pool(),
            notifications: Vec::new(),
        };
        // attempt 6: 1000 * 2^5 = 32000, capped at 30000
        assert_eq!(lockout.compute_delay(6), 30000);
        // Very large attempt count should still cap
        assert_eq!(lockout.compute_delay(100), 30000);
    }

    #[test]
    fn test_compute_delay_disabled() {
        let mut config = test_lockout();
        config.progressive_delay_enabled = false;
        let lockout = LoginLockout {
            config,
            redis_pool: create_dummy_pool(),
            notifications: Vec::new(),
        };
        assert_eq!(lockout.compute_delay(1), 0);
        assert_eq!(lockout.compute_delay(5), 0);
    }

    #[test]
    fn test_compute_delay_multiplier_one() {
        let mut config = test_lockout();
        config.delay_multiplier = 1.0;
        let lockout = LoginLockout {
            config,
            redis_pool: create_dummy_pool(),
            notifications: Vec::new(),
        };
        // With multiplier 1.0, delay is always base_delay_ms
        assert_eq!(lockout.compute_delay(1), 1000);
        assert_eq!(lockout.compute_delay(5), 1000);
        assert_eq!(lockout.compute_delay(100), 1000);
    }

    #[test]
    fn test_compute_delay_overflow_protection() {
        let mut config = test_lockout();
        config.delay_multiplier = 10.0;
        config.max_delay_ms = 30000;
        let lockout = LoginLockout {
            config,
            redis_pool: create_dummy_pool(),
            notifications: Vec::new(),
        };
        // Very large exponent: 1000 * 10^99 = infinity, should cap at max
        assert_eq!(lockout.compute_delay(100), 30000);
    }

    #[test]
    fn test_redis_key_format() {
        let config = test_lockout();
        let lockout = LoginLockout {
            config,
            redis_pool: create_dummy_pool(),
            notifications: Vec::new(),
        };
        assert_eq!(
            lockout.attempts_key("user@example.com"),
            "lockout:attempts:user@example.com"
        );
        assert_eq!(
            lockout.locked_key("user@example.com"),
            "lockout:locked:user@example.com"
        );
    }

    #[test]
    fn test_redis_key_custom_prefix() {
        let mut config = test_lockout();
        config.key_prefix = "myapp".to_string();
        let lockout = LoginLockout {
            config,
            redis_pool: create_dummy_pool(),
            notifications: Vec::new(),
        };
        assert_eq!(lockout.attempts_key("alice"), "myapp:attempts:alice");
        assert_eq!(lockout.locked_key("alice"), "myapp:locked:alice");
    }

    /// Create a dummy Redis pool for unit tests that don't need Redis
    fn create_dummy_pool() -> RedisPool {
        let cfg = deadpool_redis::Config::from_url("redis://localhost:6379");
        cfg.create_pool(Some(deadpool_redis::Runtime::Tokio1))
            .expect("Failed to create dummy pool")
    }
}
