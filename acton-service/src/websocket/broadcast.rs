//! Broadcasting utilities for efficient message distribution
//!
//! The `Broadcaster` provides a way to manage WebSocket connections and
//! broadcast messages to groups of connections without using the room manager.
//! This is useful for simple broadcast scenarios where room management isn't needed.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::Message;
use tokio::sync::{mpsc, RwLock};

use super::handler::ConnectionId;

/// Target for a broadcast message
#[derive(Debug, Clone)]
pub enum BroadcastTarget {
    /// Broadcast to all connected clients
    All,
    /// Broadcast to specific connections by ID
    Connections(Vec<ConnectionId>),
    /// Broadcast to all except specified connections
    AllExcept(Vec<ConnectionId>),
}

/// Manages broadcasting to connected WebSocket clients
///
/// The `Broadcaster` maintains a registry of active connections and provides
/// methods to send messages to all or subsets of connections.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::websocket::{Broadcaster, ConnectionId};
/// use axum::extract::ws::Message;
///
/// let broadcaster = Broadcaster::new();
///
/// // Register a connection
/// broadcaster.register(connection_id, sender).await;
///
/// // Broadcast to all
/// broadcaster.broadcast_all(Message::Text("Hello everyone!".into())).await;
///
/// // Unregister when done
/// broadcaster.unregister(&connection_id).await;
/// ```
#[derive(Debug, Clone)]
pub struct Broadcaster {
    connections: Arc<RwLock<HashMap<ConnectionId, mpsc::Sender<Message>>>>,
}

impl Broadcaster {
    /// Create a new broadcaster
    #[must_use]
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new connection with the broadcaster
    ///
    /// # Arguments
    ///
    /// * `id` - The connection's unique identifier
    /// * `sender` - Channel for sending messages to the connection
    pub async fn register(&self, id: ConnectionId, sender: mpsc::Sender<Message>) {
        self.connections.write().await.insert(id, sender);
        tracing::debug!(connection_id = %id, "Connection registered with broadcaster");
    }

    /// Unregister a connection from the broadcaster
    ///
    /// # Arguments
    ///
    /// * `id` - The connection to unregister
    pub async fn unregister(&self, id: &ConnectionId) {
        self.connections.write().await.remove(id);
        tracing::debug!(connection_id = %id, "Connection unregistered from broadcaster");
    }

    /// Broadcast a message to all connections
    ///
    /// Returns the number of connections the message was successfully sent to.
    pub async fn broadcast_all(&self, message: Message) -> usize {
        let connections = self.connections.read().await;
        let mut sent = 0;

        for sender in connections.values() {
            if sender.send(message.clone()).await.is_ok() {
                sent += 1;
            }
        }

        tracing::debug!(sent = sent, total = connections.len(), "Broadcast to all completed");
        sent
    }

    /// Broadcast a message to specific connections
    ///
    /// Returns the number of connections the message was successfully sent to.
    pub async fn broadcast_to(&self, ids: &[ConnectionId], message: Message) -> usize {
        let connections = self.connections.read().await;
        let mut sent = 0;

        for id in ids {
            if let Some(sender) = connections.get(id) {
                if sender.send(message.clone()).await.is_ok() {
                    sent += 1;
                }
            }
        }

        tracing::debug!(sent = sent, requested = ids.len(), "Broadcast to specific connections completed");
        sent
    }

    /// Broadcast a message to all except specified connections
    ///
    /// Returns the number of connections the message was successfully sent to.
    pub async fn broadcast_except(&self, exclude: &[ConnectionId], message: Message) -> usize {
        let connections = self.connections.read().await;
        let mut sent = 0;

        for (id, sender) in connections.iter() {
            if !exclude.contains(id) && sender.send(message.clone()).await.is_ok() {
                sent += 1;
            }
        }

        tracing::debug!(
            sent = sent,
            excluded = exclude.len(),
            "Broadcast except completed"
        );
        sent
    }

    /// Broadcast using a target specification
    ///
    /// Returns the number of connections the message was successfully sent to.
    pub async fn broadcast(&self, target: BroadcastTarget, message: Message) -> usize {
        match target {
            BroadcastTarget::All => self.broadcast_all(message).await,
            BroadcastTarget::Connections(ids) => self.broadcast_to(&ids, message).await,
            BroadcastTarget::AllExcept(exclude) => self.broadcast_except(&exclude, message).await,
        }
    }

    /// Send a message to a single connection
    ///
    /// Returns `true` if the message was sent successfully.
    pub async fn send_to(&self, id: &ConnectionId, message: Message) -> bool {
        let connections = self.connections.read().await;
        if let Some(sender) = connections.get(id) {
            sender.send(message).await.is_ok()
        } else {
            false
        }
    }

    /// Get the current number of registered connections
    pub async fn connection_count(&self) -> usize {
        self.connections.read().await.len()
    }

    /// Check if a connection is registered
    pub async fn has_connection(&self, id: &ConnectionId) -> bool {
        self.connections.read().await.contains_key(id)
    }

    /// Get a list of all registered connection IDs
    pub async fn connection_ids(&self) -> Vec<ConnectionId> {
        self.connections.read().await.keys().copied().collect()
    }
}

impl Default for Broadcaster {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_broadcaster_creation() {
        let broadcaster = Broadcaster::new();
        assert_eq!(broadcaster.connection_count().await, 0);
    }

    #[tokio::test]
    async fn test_register_and_unregister() {
        let broadcaster = Broadcaster::new();
        let id = ConnectionId::new();
        let (tx, _rx) = mpsc::channel(32);

        broadcaster.register(id, tx).await;
        assert!(broadcaster.has_connection(&id).await);
        assert_eq!(broadcaster.connection_count().await, 1);

        broadcaster.unregister(&id).await;
        assert!(!broadcaster.has_connection(&id).await);
        assert_eq!(broadcaster.connection_count().await, 0);
    }

    #[tokio::test]
    async fn test_broadcast_all() {
        let broadcaster = Broadcaster::new();

        let id1 = ConnectionId::new();
        let id2 = ConnectionId::new();
        let (tx1, mut rx1) = mpsc::channel(32);
        let (tx2, mut rx2) = mpsc::channel(32);

        broadcaster.register(id1, tx1).await;
        broadcaster.register(id2, tx2).await;

        let sent = broadcaster.broadcast_all(Message::Text("hello".into())).await;
        assert_eq!(sent, 2);

        // Verify both received
        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_ok());
    }

    #[tokio::test]
    async fn test_broadcast_except() {
        let broadcaster = Broadcaster::new();

        let id1 = ConnectionId::new();
        let id2 = ConnectionId::new();
        let (tx1, mut rx1) = mpsc::channel(32);
        let (tx2, mut rx2) = mpsc::channel(32);

        broadcaster.register(id1, tx1).await;
        broadcaster.register(id2, tx2).await;

        let sent = broadcaster.broadcast_except(&[id1], Message::Text("hello".into())).await;
        assert_eq!(sent, 1);

        // Only id2 should receive
        assert!(rx1.try_recv().is_err());
        assert!(rx2.try_recv().is_ok());
    }

    #[tokio::test]
    async fn test_send_to_single() {
        let broadcaster = Broadcaster::new();
        let id = ConnectionId::new();
        let (tx, mut rx) = mpsc::channel(32);

        broadcaster.register(id, tx).await;

        let success = broadcaster.send_to(&id, Message::Text("direct".into())).await;
        assert!(success);
        assert!(rx.try_recv().is_ok());

        // Sending to unknown connection should fail
        let unknown = ConnectionId::new();
        let success = broadcaster.send_to(&unknown, Message::Text("test".into())).await;
        assert!(!success);
    }
}
