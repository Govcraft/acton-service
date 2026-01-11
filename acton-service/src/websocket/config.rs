//! WebSocket configuration

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// WebSocket server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketConfig {
    /// Enable WebSocket support
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Maximum message size in bytes (default: 64KB)
    #[serde(default = "default_max_message_size")]
    pub max_message_size_bytes: usize,

    /// Maximum number of concurrent WebSocket connections per client IP
    #[serde(default = "default_max_connections_per_client")]
    pub max_connections_per_client: usize,

    /// Ping interval in seconds (for keepalive)
    #[serde(default = "default_ping_interval")]
    pub ping_interval_secs: u64,

    /// Pong timeout in seconds (disconnect if no pong received)
    #[serde(default = "default_pong_timeout")]
    pub pong_timeout_secs: u64,

    /// Maximum frame size in bytes
    #[serde(default = "default_max_frame_size")]
    pub max_frame_size_bytes: usize,

    /// Room/channel configuration
    #[serde(default)]
    pub rooms: RoomConfig,
}

impl WebSocketConfig {
    /// Get the ping interval as a Duration
    #[must_use]
    pub fn ping_interval(&self) -> Duration {
        Duration::from_secs(self.ping_interval_secs)
    }

    /// Get the pong timeout as a Duration
    #[must_use]
    pub fn pong_timeout(&self) -> Duration {
        Duration::from_secs(self.pong_timeout_secs)
    }
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_message_size_bytes: default_max_message_size(),
            max_connections_per_client: default_max_connections_per_client(),
            ping_interval_secs: default_ping_interval(),
            pong_timeout_secs: default_pong_timeout(),
            max_frame_size_bytes: default_max_frame_size(),
            rooms: RoomConfig::default(),
        }
    }
}

/// Room/channel configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomConfig {
    /// Enable room/channel support
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Maximum members per room
    #[serde(default = "default_max_room_members")]
    pub max_members: usize,

    /// Maximum rooms a single connection can join
    #[serde(default = "default_max_rooms_per_connection")]
    pub max_rooms_per_connection: usize,

    /// Room idle timeout in seconds (auto-cleanup empty rooms)
    #[serde(default = "default_room_idle_timeout")]
    pub idle_timeout_secs: u64,
}

impl RoomConfig {
    /// Get the idle timeout as a Duration
    #[must_use]
    pub fn idle_timeout(&self) -> Duration {
        Duration::from_secs(self.idle_timeout_secs)
    }
}

impl Default for RoomConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_members: default_max_room_members(),
            max_rooms_per_connection: default_max_rooms_per_connection(),
            idle_timeout_secs: default_room_idle_timeout(),
        }
    }
}

// Default value functions

const fn default_enabled() -> bool {
    true
}

const fn default_max_message_size() -> usize {
    65536 // 64KB
}

const fn default_max_connections_per_client() -> usize {
    5
}

const fn default_ping_interval() -> u64 {
    30
}

const fn default_pong_timeout() -> u64 {
    10
}

const fn default_max_frame_size() -> usize {
    16384 // 16KB
}

const fn default_max_room_members() -> usize {
    1000
}

const fn default_max_rooms_per_connection() -> usize {
    10
}

const fn default_room_idle_timeout() -> u64 {
    3600 // 1 hour
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_websocket_config() {
        let config = WebSocketConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_message_size_bytes, 65536);
        assert_eq!(config.ping_interval_secs, 30);
        assert!(config.rooms.enabled);
    }

    #[test]
    fn test_duration_helpers() {
        let config = WebSocketConfig::default();
        assert_eq!(config.ping_interval(), Duration::from_secs(30));
        assert_eq!(config.pong_timeout(), Duration::from_secs(10));
    }
}
