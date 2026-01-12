//! SSE configuration types.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// SSE server configuration.
///
/// Configure keep-alive intervals, retry settings, and connection limits.
///
/// # Example
///
/// ```toml
/// [sse]
/// keep_alive_interval_secs = 15
/// default_retry_ms = 3000
/// max_connections_per_client = 10
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SseConfig {
    /// Keep-alive interval in seconds (default: 15).
    ///
    /// The server will send comment-only events at this interval
    /// to keep the connection alive.
    #[serde(default = "default_keep_alive_interval")]
    pub keep_alive_interval_secs: u64,

    /// Keep-alive comment text (default: empty, sends ": \n").
    #[serde(default)]
    pub keep_alive_text: Option<String>,

    /// Default retry interval for clients in milliseconds (default: 3000).
    ///
    /// Sent in the `retry:` field to tell clients how long to wait
    /// before reconnecting.
    #[serde(default = "default_retry_ms")]
    pub default_retry_ms: u64,

    /// Maximum concurrent SSE connections per client IP (default: 10).
    #[serde(default = "default_max_connections_per_client")]
    pub max_connections_per_client: usize,

    /// Connection timeout in seconds (0 = no timeout, default: 0).
    #[serde(default)]
    pub connection_timeout_secs: u64,
}

impl SseConfig {
    /// Get the keep-alive interval as a Duration.
    #[must_use]
    pub fn keep_alive_interval(&self) -> Duration {
        Duration::from_secs(self.keep_alive_interval_secs)
    }

    /// Get the default retry as a Duration.
    #[must_use]
    pub fn default_retry(&self) -> Duration {
        Duration::from_millis(self.default_retry_ms)
    }

    /// Get the connection timeout as a Duration, or None if disabled.
    #[must_use]
    pub fn connection_timeout(&self) -> Option<Duration> {
        if self.connection_timeout_secs == 0 {
            None
        } else {
            Some(Duration::from_secs(self.connection_timeout_secs))
        }
    }
}

impl Default for SseConfig {
    fn default() -> Self {
        Self {
            keep_alive_interval_secs: default_keep_alive_interval(),
            keep_alive_text: None,
            default_retry_ms: default_retry_ms(),
            max_connections_per_client: default_max_connections_per_client(),
            connection_timeout_secs: 0,
        }
    }
}

fn default_keep_alive_interval() -> u64 {
    15
}

fn default_retry_ms() -> u64 {
    3000
}

fn default_max_connections_per_client() -> usize {
    10
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SseConfig::default();
        assert_eq!(config.keep_alive_interval_secs, 15);
        assert_eq!(config.default_retry_ms, 3000);
        assert_eq!(config.max_connections_per_client, 10);
        assert_eq!(config.connection_timeout_secs, 0);
    }

    #[test]
    fn test_keep_alive_interval() {
        let config = SseConfig::default();
        assert_eq!(config.keep_alive_interval(), Duration::from_secs(15));
    }

    #[test]
    fn test_connection_timeout() {
        let mut config = SseConfig::default();
        assert!(config.connection_timeout().is_none());

        config.connection_timeout_secs = 60;
        assert_eq!(config.connection_timeout(), Some(Duration::from_secs(60)));
    }
}
