//! WebSocket handler utilities and connection management

use axum::extract::ws::Message;
use std::fmt;
use std::hash::{Hash, Hasher};
use tokio::sync::mpsc;
use uuid::Uuid;

/// Unique identifier for a WebSocket connection
#[derive(Clone, Copy, Eq)]
pub struct ConnectionId(Uuid);

impl ConnectionId {
    /// Create a new unique connection ID
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Get the underlying UUID
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

impl fmt::Debug for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ConnectionId({})", self.0)
    }
}

impl fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl PartialEq for ConnectionId {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Hash for ConnectionId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl From<Uuid> for ConnectionId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

/// Represents an active WebSocket connection
///
/// This struct holds information about a connected WebSocket client,
/// including a channel for sending messages to the client.
#[derive(Debug)]
pub struct WebSocketConnection {
    /// Unique identifier for this connection
    pub id: ConnectionId,

    /// Channel sender for sending messages to this connection
    pub sender: mpsc::Sender<Message>,

    /// Optional user ID if the connection is authenticated
    pub user_id: Option<String>,

    /// Rooms this connection has joined
    pub rooms: Vec<String>,

    /// Client IP address (if available)
    pub client_ip: Option<String>,
}

impl WebSocketConnection {
    /// Create a new WebSocket connection
    #[must_use]
    pub fn new(sender: mpsc::Sender<Message>) -> Self {
        Self {
            id: ConnectionId::new(),
            sender,
            user_id: None,
            rooms: Vec::new(),
            client_ip: None,
        }
    }

    /// Create a new authenticated WebSocket connection
    #[must_use]
    pub fn authenticated(sender: mpsc::Sender<Message>, user_id: String) -> Self {
        Self {
            id: ConnectionId::new(),
            sender,
            user_id: Some(user_id),
            rooms: Vec::new(),
            client_ip: None,
        }
    }

    /// Set the client IP address
    #[must_use]
    pub fn with_client_ip(mut self, ip: String) -> Self {
        self.client_ip = Some(ip);
        self
    }

    /// Check if the connection is authenticated
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.user_id.is_some()
    }

    /// Send a message to this connection
    ///
    /// Returns an error if the connection is closed.
    pub async fn send(&self, message: Message) -> Result<(), mpsc::error::SendError<Message>> {
        self.sender.send(message).await
    }

    /// Send a text message to this connection
    pub async fn send_text(&self, text: impl Into<String>) -> Result<(), mpsc::error::SendError<Message>> {
        self.send(Message::Text(text.into().into())).await
    }

    /// Send a binary message to this connection
    pub async fn send_binary(&self, data: Vec<u8>) -> Result<(), mpsc::error::SendError<Message>> {
        self.send(Message::Binary(data.into())).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_id_uniqueness() {
        let id1 = ConnectionId::new();
        let id2 = ConnectionId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_connection_id_display() {
        let id = ConnectionId::new();
        let display = format!("{}", id);
        assert!(!display.is_empty());
    }

    #[tokio::test]
    async fn test_websocket_connection_creation() {
        let (tx, _rx) = mpsc::channel(32);
        let conn = WebSocketConnection::new(tx);
        assert!(!conn.is_authenticated());
        assert!(conn.rooms.is_empty());
    }

    #[tokio::test]
    async fn test_authenticated_connection() {
        let (tx, _rx) = mpsc::channel(32);
        let conn = WebSocketConnection::authenticated(tx, "user123".to_string());
        assert!(conn.is_authenticated());
        assert_eq!(conn.user_id, Some("user123".to_string()));
    }
}
