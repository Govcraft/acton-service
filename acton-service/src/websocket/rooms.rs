//! Room/channel management using acton-reactive actors
//!
//! This module provides actor-based room management for WebSocket connections.
//! Rooms allow connections to be grouped for targeted message broadcasting.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use acton_reactive::prelude::*;
use axum::extract::ws::Message;
use chrono::{DateTime, Utc};
use tokio::sync::mpsc;

use super::config::RoomConfig;
use super::handler::ConnectionId;
use super::messages::{
    BroadcastToRoom, ConnectionDisconnected, GetRoomInfo, JoinRoomRequest, LeaveRoomRequest,
    RoomInfoResponse,
};

/// Unique identifier for a room
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RoomId(String);

impl RoomId {
    /// Create a new room ID
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the room ID as a string slice
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RoomId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for RoomId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for RoomId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// A member of a room
#[derive(Debug, Clone)]
pub struct RoomMember {
    /// The connection ID
    pub connection_id: ConnectionId,
    /// Channel for sending messages to this member
    pub sender: mpsc::Sender<Message>,
    /// Optional user ID if authenticated
    pub user_id: Option<String>,
    /// When the member joined
    pub joined_at: DateTime<Utc>,
}

impl RoomMember {
    /// Create a new room member
    #[must_use]
    pub fn new(connection_id: ConnectionId, sender: mpsc::Sender<Message>) -> Self {
        Self {
            connection_id,
            sender,
            user_id: None,
            joined_at: Utc::now(),
        }
    }

    /// Create an authenticated room member
    #[must_use]
    pub fn authenticated(
        connection_id: ConnectionId,
        sender: mpsc::Sender<Message>,
        user_id: String,
    ) -> Self {
        Self {
            connection_id,
            sender,
            user_id: Some(user_id),
            joined_at: Utc::now(),
        }
    }
}

/// A chat room / channel
#[derive(Debug)]
pub struct Room {
    /// Room identifier
    pub id: RoomId,
    /// Members currently in the room
    pub members: HashMap<ConnectionId, RoomMember>,
    /// When the room was created
    pub created_at: DateTime<Utc>,
    /// Last activity time (for idle cleanup)
    pub last_activity: DateTime<Utc>,
    /// Optional metadata
    pub metadata: HashMap<String, String>,
}

impl Room {
    /// Create a new empty room
    #[must_use]
    pub fn new(id: RoomId) -> Self {
        let now = Utc::now();
        Self {
            id,
            members: HashMap::new(),
            created_at: now,
            last_activity: now,
            metadata: HashMap::new(),
        }
    }

    /// Get the number of members in the room
    #[must_use]
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    /// Check if the room is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// Update the last activity timestamp
    pub fn touch(&mut self) {
        self.last_activity = Utc::now();
    }
}

/// State for the room manager actor
#[derive(Debug, Default)]
pub struct RoomManagerState {
    /// All rooms indexed by ID
    rooms: HashMap<RoomId, Room>,
    /// Rooms each connection is a member of (for cleanup on disconnect)
    connection_rooms: HashMap<ConnectionId, HashSet<RoomId>>,
    /// Maximum members per room
    max_members_per_room: usize,
    /// Maximum rooms per connection
    max_rooms_per_connection: usize,
}

/// Shared room manager storage for AppState access
pub type SharedRoomManager = Arc<ActorHandle>;

/// Actor-based room manager
///
/// Manages WebSocket rooms/channels using the acton-reactive actor system.
/// This provides thread-safe room operations without explicit locking.
pub struct RoomManager;

impl RoomManager {
    /// Spawn the room manager actor
    ///
    /// # Arguments
    ///
    /// * `runtime` - The actor runtime to spawn into
    /// * `config` - Room configuration
    ///
    /// # Returns
    ///
    /// The actor handle for sending messages to the room manager
    pub async fn spawn(
        runtime: &mut ActorRuntime,
        config: RoomConfig,
    ) -> anyhow::Result<ActorHandle> {
        let mut agent = runtime.new_actor::<RoomManagerState>();

        // Initialize configuration
        agent.model.max_members_per_room = config.max_members;
        agent.model.max_rooms_per_connection = config.max_rooms_per_connection;

        // Handle join room requests
        agent.mutate_on::<JoinRoomRequest>(|agent, envelope| {
            let request = envelope.message();
            let room_id = request.room_id.clone();
            let member = request.member.clone();
            let connection_id = member.connection_id;

            // Check connection room limit
            let connection_rooms = agent
                .model
                .connection_rooms
                .entry(connection_id)
                .or_default();

            if connection_rooms.len() >= agent.model.max_rooms_per_connection {
                tracing::warn!(
                    connection_id = %connection_id,
                    limit = agent.model.max_rooms_per_connection,
                    "Connection at max room limit"
                );
                return Reply::ready();
            }

            // Get or create room
            let room = agent
                .model
                .rooms
                .entry(room_id.clone())
                .or_insert_with(|| Room::new(room_id.clone()));

            // Check room member limit
            if room.members.len() >= agent.model.max_members_per_room {
                tracing::warn!(
                    room_id = %room_id,
                    limit = agent.model.max_members_per_room,
                    "Room at max capacity"
                );
                return Reply::ready();
            }

            // Add member to room
            room.members.insert(connection_id, member);
            room.touch();
            connection_rooms.insert(room_id.clone());

            tracing::info!(
                room_id = %room_id,
                connection_id = %connection_id,
                member_count = room.members.len(),
                "Member joined room"
            );

            Reply::ready()
        });

        // Handle leave room requests
        agent.mutate_on::<LeaveRoomRequest>(|agent, envelope| {
            let request = envelope.message();
            let room_id = &request.room_id;
            let connection_id = request.connection_id;

            // Remove from room
            if let Some(room) = agent.model.rooms.get_mut(room_id) {
                room.members.remove(&connection_id);
                room.touch();

                tracing::info!(
                    room_id = %room_id,
                    connection_id = %connection_id,
                    member_count = room.members.len(),
                    "Member left room"
                );

                // Clean up empty rooms
                if room.is_empty() {
                    agent.model.rooms.remove(room_id);
                    tracing::debug!(room_id = %room_id, "Empty room removed");
                }
            }

            // Update connection tracking
            if let Some(rooms) = agent.model.connection_rooms.get_mut(&connection_id) {
                rooms.remove(room_id);
            }

            Reply::ready()
        });

        // Handle broadcast to room
        agent.act_on::<BroadcastToRoom>(|agent, envelope| {
            let request = envelope.message();
            let room_id = &request.room_id;
            let message = request.message.clone();
            let exclude_sender = request.exclude_sender;

            if let Some(room) = agent.model.rooms.get(room_id) {
                // Collect senders (filtering out excluded connection)
                let senders: Vec<_> = room
                    .members
                    .values()
                    .filter(|m| {
                        exclude_sender
                            .map(|id| m.connection_id != id)
                            .unwrap_or(true)
                    })
                    .map(|m| m.sender.clone())
                    .collect();

                let member_count = senders.len();
                let room_id_log = room_id.clone();

                Reply::pending(async move {
                    let mut sent = 0;
                    for sender in senders {
                        if sender.send(message.clone()).await.is_ok() {
                            sent += 1;
                        }
                    }
                    tracing::debug!(
                        room_id = %room_id_log,
                        sent = sent,
                        total = member_count,
                        "Broadcast completed"
                    );
                })
            } else {
                Reply::ready()
            }
        });

        // Handle connection disconnect (leave all rooms)
        agent.mutate_on::<ConnectionDisconnected>(|agent, envelope| {
            let connection_id = envelope.message().connection_id;

            // Get all rooms this connection was in
            if let Some(room_ids) = agent.model.connection_rooms.remove(&connection_id) {
                for room_id in room_ids {
                    if let Some(room) = agent.model.rooms.get_mut(&room_id) {
                        room.members.remove(&connection_id);

                        // Clean up empty rooms
                        if room.is_empty() {
                            agent.model.rooms.remove(&room_id);
                            tracing::debug!(room_id = %room_id, "Empty room removed after disconnect");
                        }
                    }
                }
            }

            tracing::debug!(
                connection_id = %connection_id,
                "Connection removed from all rooms"
            );

            Reply::ready()
        });

        // Handle room info requests
        agent.act_on::<GetRoomInfo>(|agent, envelope| {
            let room_id = envelope.message().room_id.clone();
            let reply = envelope.reply_envelope();

            let response = if let Some(room) = agent.model.rooms.get(&room_id) {
                RoomInfoResponse {
                    room_id,
                    member_count: room.member_count(),
                    exists: true,
                }
            } else {
                RoomInfoResponse {
                    room_id,
                    member_count: 0,
                    exists: false,
                }
            };

            Reply::pending(async move {
                reply.send(response).await;
            })
        });

        // Log startup
        agent.after_start(|_agent| {
            tracing::info!("WebSocket room manager started");
            Reply::ready()
        });

        // Log shutdown
        agent.before_stop(|agent| {
            let room_count = agent.model.rooms.len();
            let connection_count = agent.model.connection_rooms.len();

            tracing::info!(
                rooms = room_count,
                connections = connection_count,
                "WebSocket room manager shutting down"
            );

            Reply::ready()
        });

        let handle = agent.start().await;
        Ok(handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_room_id_from_string() {
        let id: RoomId = "test-room".into();
        assert_eq!(id.as_str(), "test-room");
    }

    #[test]
    fn test_room_creation() {
        let room = Room::new("test".into());
        assert!(room.is_empty());
        assert_eq!(room.member_count(), 0);
    }

    #[tokio::test]
    async fn test_room_member_creation() {
        let (tx, _rx) = mpsc::channel(32);
        let member = RoomMember::new(ConnectionId::new(), tx);
        assert!(member.user_id.is_none());
    }

    #[tokio::test]
    async fn test_authenticated_member() {
        let (tx, _rx) = mpsc::channel(32);
        let member = RoomMember::authenticated(ConnectionId::new(), tx, "user123".to_string());
        assert_eq!(member.user_id, Some("user123".to_string()));
    }
}
