//! SSE broadcasting utilities for multi-connection delivery.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use super::connection::ConnectionId;

/// Message that can be broadcast to SSE connections.
#[derive(Debug, Clone)]
pub struct BroadcastMessage {
    /// Optional event type name.
    pub event_type: Option<String>,
    /// Event data (serialized).
    pub data: String,
    /// Optional event ID.
    pub id: Option<String>,
}

impl BroadcastMessage {
    /// Create a new broadcast message with data only.
    #[must_use]
    pub fn new(data: impl Into<String>) -> Self {
        Self {
            event_type: None,
            data: data.into(),
            id: None,
        }
    }

    /// Create a named broadcast message.
    #[must_use]
    pub fn named(event_type: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            event_type: Some(event_type.into()),
            data: data.into(),
            id: None,
        }
    }

    /// Create a message with JSON-serialized data.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    pub fn json<T: serde::Serialize>(data: &T) -> Result<Self, serde_json::Error> {
        Ok(Self {
            event_type: None,
            data: serde_json::to_string(data)?,
            id: None,
        })
    }

    /// Create a named message with JSON-serialized data.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    pub fn json_named<T: serde::Serialize>(
        event_type: impl Into<String>,
        data: &T,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            event_type: Some(event_type.into()),
            data: serde_json::to_string(data)?,
            id: None,
        })
    }

    /// Set event ID.
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }
}

/// Target for broadcast messages.
#[derive(Debug, Clone)]
pub enum BroadcastTarget {
    /// Broadcast to all connected clients.
    All,
    /// Broadcast to specific connections.
    Connections(Vec<ConnectionId>),
    /// Broadcast to all except specified connections.
    AllExcept(Vec<ConnectionId>),
    /// Broadcast to a named channel/topic.
    Channel(String),
}

/// Connection info stored in the broadcaster.
#[derive(Debug)]
struct ConnectionInfo {
    /// Channels this connection is subscribed to.
    subscribed_channels: Vec<String>,
}

/// Manages SSE connections and broadcasting.
///
/// Unlike WebSocket which uses mpsc channels per connection,
/// SSE uses broadcast channels for efficient fan-out.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::sse::{SseBroadcaster, BroadcastMessage};
/// use std::sync::Arc;
///
/// let broadcaster = Arc::new(SseBroadcaster::new());
///
/// // In your SSE handler
/// async fn sse_handler(
///     Extension(broadcaster): Extension<Arc<SseBroadcaster>>,
/// ) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
///     let mut receiver = broadcaster.subscribe();
///
///     let stream = async_stream::stream! {
///         while let Ok(msg) = receiver.recv().await {
///             let mut event = Event::default().data(msg.data);
///             if let Some(event_type) = msg.event_type {
///                 event = event.event(event_type);
///             }
///             yield Ok(event);
///         }
///     };
///
///     Sse::new(stream).keep_alive(KeepAlive::default())
/// }
///
/// // In your trigger endpoint
/// async fn trigger(
///     Extension(broadcaster): Extension<Arc<SseBroadcaster>>,
/// ) -> impl IntoResponse {
///     broadcaster.broadcast(BroadcastMessage::new("Hello!"));
///     "OK"
/// }
/// ```
#[derive(Debug, Clone)]
pub struct SseBroadcaster {
    /// Global broadcast channel.
    sender: broadcast::Sender<BroadcastMessage>,
    /// Per-channel senders for topic-based broadcasting.
    channels: Arc<RwLock<HashMap<String, broadcast::Sender<BroadcastMessage>>>>,
    /// Connection tracking for targeted delivery.
    connections: Arc<RwLock<HashMap<ConnectionId, ConnectionInfo>>>,
    /// Channel capacity.
    capacity: usize,
}

impl SseBroadcaster {
    /// Create a new broadcaster with default capacity (256).
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(256)
    }

    /// Create a broadcaster with specific capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            channels: Arc::new(RwLock::new(HashMap::new())),
            connections: Arc::new(RwLock::new(HashMap::new())),
            capacity,
        }
    }

    /// Subscribe to the global broadcast channel.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<BroadcastMessage> {
        self.sender.subscribe()
    }

    /// Subscribe to a named channel.
    pub async fn subscribe_channel(&self, channel: &str) -> broadcast::Receiver<BroadcastMessage> {
        let mut channels = self.channels.write().await;
        if let Some(sender) = channels.get(channel) {
            sender.subscribe()
        } else {
            let (sender, receiver) = broadcast::channel(self.capacity);
            channels.insert(channel.to_string(), sender);
            receiver
        }
    }

    /// Register a connection.
    pub async fn register(&self, id: ConnectionId) {
        self.connections.write().await.insert(
            id,
            ConnectionInfo {
                subscribed_channels: Vec::new(),
            },
        );
        tracing::debug!(connection_id = %id, "SSE connection registered");
    }

    /// Register a connection with channel subscriptions.
    pub async fn register_with_channels(&self, id: ConnectionId, channels: Vec<String>) {
        self.connections.write().await.insert(
            id,
            ConnectionInfo {
                subscribed_channels: channels,
            },
        );
        tracing::debug!(connection_id = %id, "SSE connection registered with channels");
    }

    /// Unregister a connection.
    pub async fn unregister(&self, id: &ConnectionId) {
        self.connections.write().await.remove(id);
        tracing::debug!(connection_id = %id, "SSE connection unregistered");
    }

    /// Broadcast a message to all connections.
    ///
    /// Returns the number of receivers that will receive the message.
    pub fn broadcast(&self, message: BroadcastMessage) -> Result<usize, broadcast::error::SendError<BroadcastMessage>> {
        self.sender.send(message)
    }

    /// Broadcast to a specific channel.
    ///
    /// Returns the number of receivers, or 0 if the channel doesn't exist.
    pub async fn broadcast_to_channel(
        &self,
        channel: &str,
        message: BroadcastMessage,
    ) -> Result<usize, broadcast::error::SendError<BroadcastMessage>> {
        let channels = self.channels.read().await;
        if let Some(sender) = channels.get(channel) {
            sender.send(message)
        } else {
            Ok(0) // No subscribers
        }
    }

    /// Get the number of active connections.
    pub async fn connection_count(&self) -> usize {
        self.connections.read().await.len()
    }

    /// Get the number of active channels.
    pub async fn channel_count(&self) -> usize {
        self.channels.read().await.len()
    }

    /// Check if a channel exists.
    pub async fn has_channel(&self, channel: &str) -> bool {
        self.channels.read().await.contains_key(channel)
    }

    /// Get channels a connection is subscribed to.
    pub async fn connection_channels(&self, id: &ConnectionId) -> Option<Vec<String>> {
        self.connections
            .read()
            .await
            .get(id)
            .map(|info| info.subscribed_channels.clone())
    }
}

impl Default for SseBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_broadcast_message() {
        let msg = BroadcastMessage::new("Hello");
        assert_eq!(msg.data, "Hello");
        assert!(msg.event_type.is_none());
        assert!(msg.id.is_none());
    }

    #[test]
    fn test_broadcast_message_named() {
        let msg = BroadcastMessage::named("notification", "Hello");
        assert_eq!(msg.data, "Hello");
        assert_eq!(msg.event_type, Some("notification".to_string()));
    }

    #[test]
    fn test_broadcast_message_with_id() {
        let msg = BroadcastMessage::new("Hello").with_id("event-123");
        assert_eq!(msg.id, Some("event-123".to_string()));
    }

    #[tokio::test]
    async fn test_broadcaster() {
        let broadcaster = SseBroadcaster::new();
        let mut receiver = broadcaster.subscribe();

        broadcaster.broadcast(BroadcastMessage::new("Test")).unwrap();

        let msg = receiver.recv().await.unwrap();
        assert_eq!(msg.data, "Test");
    }

    #[tokio::test]
    async fn test_connection_tracking() {
        let broadcaster = SseBroadcaster::new();
        let id = ConnectionId::new();

        assert_eq!(broadcaster.connection_count().await, 0);

        broadcaster.register(id).await;
        assert_eq!(broadcaster.connection_count().await, 1);

        broadcaster.unregister(&id).await;
        assert_eq!(broadcaster.connection_count().await, 0);
    }
}
