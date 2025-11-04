//! Resilience middleware for fault tolerance and reliability
//!
//! This module provides circuit breaker, retry, and bulkhead patterns
//! to ensure service stability and graceful degradation.

use std::time::Duration;

/// Configuration for resilience patterns
#[derive(Debug, Clone)]
pub struct ResilienceConfig {
    /// Enable circuit breaker
    pub circuit_breaker_enabled: bool,
    /// Failure threshold before circuit opens (0.0-1.0)
    pub circuit_breaker_threshold: f64,
    /// Minimum requests before calculating failure rate
    pub circuit_breaker_min_requests: u64,
    /// Duration to wait before attempting to close circuit
    pub circuit_breaker_wait_duration: Duration,

    /// Enable retry logic
    pub retry_enabled: bool,
    /// Maximum number of retry attempts
    pub retry_max_attempts: usize,
    /// Base delay for exponential backoff
    pub retry_base_delay: Duration,
    /// Maximum delay for exponential backoff
    pub retry_max_delay: Duration,

    /// Enable bulkhead (concurrency limiting)
    pub bulkhead_enabled: bool,
    /// Maximum concurrent requests
    pub bulkhead_max_concurrent: usize,
    /// Maximum queued requests
    pub bulkhead_max_queued: usize,
}

impl Default for ResilienceConfig {
    fn default() -> Self {
        Self {
            circuit_breaker_enabled: true,
            circuit_breaker_threshold: 0.5, // 50% failure rate
            circuit_breaker_min_requests: 10,
            circuit_breaker_wait_duration: Duration::from_secs(30),

            retry_enabled: true,
            retry_max_attempts: 3,
            retry_base_delay: Duration::from_millis(100),
            retry_max_delay: Duration::from_secs(10),

            bulkhead_enabled: true,
            bulkhead_max_concurrent: 100,
            bulkhead_max_queued: 200,
        }
    }
}

impl ResilienceConfig {
    /// Create a new resilience configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set circuit breaker enabled
    pub fn with_circuit_breaker(mut self, enabled: bool) -> Self {
        self.circuit_breaker_enabled = enabled;
        self
    }

    /// Set circuit breaker threshold
    pub fn with_circuit_breaker_threshold(mut self, threshold: f64) -> Self {
        self.circuit_breaker_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Set retry enabled
    pub fn with_retry(mut self, enabled: bool) -> Self {
        self.retry_enabled = enabled;
        self
    }

    /// Set maximum retry attempts
    pub fn with_retry_max_attempts(mut self, attempts: usize) -> Self {
        self.retry_max_attempts = attempts;
        self
    }

    /// Set bulkhead enabled
    pub fn with_bulkhead(mut self, enabled: bool) -> Self {
        self.bulkhead_enabled = enabled;
        self
    }

    /// Set bulkhead maximum concurrent requests
    pub fn with_bulkhead_max_concurrent(mut self, max: usize) -> Self {
        self.bulkhead_max_concurrent = max;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ResilienceConfig::default();
        assert!(config.circuit_breaker_enabled);
        assert!(config.retry_enabled);
        assert!(config.bulkhead_enabled);
    }

    #[test]
    fn test_builder_pattern() {
        let config = ResilienceConfig::new()
            .with_circuit_breaker(false)
            .with_retry_max_attempts(5)
            .with_bulkhead_max_concurrent(50);

        assert!(!config.circuit_breaker_enabled);
        assert_eq!(config.retry_max_attempts, 5);
        assert_eq!(config.bulkhead_max_concurrent, 50);
    }

    #[test]
    fn test_threshold_clamping() {
        let config = ResilienceConfig::new()
            .with_circuit_breaker_threshold(1.5);

        assert_eq!(config.circuit_breaker_threshold, 1.0);

        let config = ResilienceConfig::new()
            .with_circuit_breaker_threshold(-0.5);

        assert_eq!(config.circuit_breaker_threshold, 0.0);
    }
}
