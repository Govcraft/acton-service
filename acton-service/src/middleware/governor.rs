//! Governor-based rate limiting middleware
//!
//! Provides local (in-memory) rate limiting as a fallback or complement
//! to Redis-based global rate limiting. Useful for per-endpoint limits
//! and when Redis is unavailable.

use std::time::Duration;

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
            burst_size: 10, // Allow bursts up to 110 requests
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
        let exceeded = RateLimitExceeded::new(
            Duration::from_secs(30),
            100,
            Duration::from_secs(60),
        );

        assert_eq!(exceeded.retry_after_secs(), 30);
        assert_eq!(exceeded.limit, 100);
        assert_eq!(exceeded.period, Duration::from_secs(60));
    }
}
