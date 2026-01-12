//! SSE connection tracking and management.

use std::fmt;
use uuid::Uuid;

/// Unique identifier for an SSE connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionId(Uuid);

impl ConnectionId {
    /// Create a new unique connection ID.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Get the underlying UUID.
    #[must_use]
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for ConnectionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for ConnectionId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

/// Metadata about an SSE connection.
#[derive(Debug, Clone)]
pub struct SseConnection {
    /// Unique identifier.
    pub id: ConnectionId,
    /// Optional user ID if authenticated.
    pub user_id: Option<String>,
    /// Client IP address.
    pub client_ip: Option<String>,
    /// Last event ID sent (for reconnection).
    pub last_event_id: Option<String>,
    /// Channels this connection is subscribed to.
    pub subscribed_channels: Vec<String>,
}

impl SseConnection {
    /// Create a new SSE connection.
    #[must_use]
    pub fn new() -> Self {
        Self {
            id: ConnectionId::new(),
            user_id: None,
            client_ip: None,
            last_event_id: None,
            subscribed_channels: Vec::new(),
        }
    }

    /// Create an authenticated connection.
    #[must_use]
    pub fn authenticated(user_id: impl Into<String>) -> Self {
        Self {
            id: ConnectionId::new(),
            user_id: Some(user_id.into()),
            client_ip: None,
            last_event_id: None,
            subscribed_channels: Vec::new(),
        }
    }

    /// Set client IP.
    #[must_use]
    pub fn with_client_ip(mut self, ip: impl Into<String>) -> Self {
        self.client_ip = Some(ip.into());
        self
    }

    /// Set last event ID (from Last-Event-ID header).
    #[must_use]
    pub fn with_last_event_id(mut self, id: impl Into<String>) -> Self {
        self.last_event_id = Some(id.into());
        self
    }

    /// Subscribe to a channel.
    #[must_use]
    pub fn with_channel(mut self, channel: impl Into<String>) -> Self {
        self.subscribed_channels.push(channel.into());
        self
    }

    /// Check if subscribed to a channel.
    #[must_use]
    pub fn is_subscribed(&self, channel: &str) -> bool {
        self.subscribed_channels.iter().any(|c| c == channel)
    }
}

impl Default for SseConnection {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_id() {
        let id1 = ConnectionId::new();
        let id2 = ConnectionId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_sse_connection_builder() {
        let conn = SseConnection::authenticated("user123")
            .with_client_ip("192.168.1.1")
            .with_last_event_id("event-42")
            .with_channel("notifications");

        assert_eq!(conn.user_id, Some("user123".to_string()));
        assert_eq!(conn.client_ip, Some("192.168.1.1".to_string()));
        assert_eq!(conn.last_event_id, Some("event-42".to_string()));
        assert!(conn.is_subscribed("notifications"));
        assert!(!conn.is_subscribed("other"));
    }
}
