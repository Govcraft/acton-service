//! Actor messages for WebSocket coordination
//!
//! These messages are used for communication between WebSocket handlers
//! and the room manager actor.

use super::handler::ConnectionId;
use super::rooms::{RoomId, RoomMember};
use axum::extract::ws::Message;

/// Request to join a room
#[derive(Debug, Clone)]
pub struct JoinRoomRequest {
    /// The room to join
    pub room_id: RoomId,
    /// The member joining
    pub member: RoomMember,
}

impl JoinRoomRequest {
    /// Create a new join room request
    #[must_use]
    pub fn new(room_id: impl Into<RoomId>, member: RoomMember) -> Self {
        Self {
            room_id: room_id.into(),
            member,
        }
    }
}

/// Request to leave a room
#[derive(Debug, Clone)]
pub struct LeaveRoomRequest {
    /// The room to leave
    pub room_id: RoomId,
    /// The connection leaving
    pub connection_id: ConnectionId,
}

impl LeaveRoomRequest {
    /// Create a new leave room request
    #[must_use]
    pub fn new(room_id: impl Into<RoomId>, connection_id: ConnectionId) -> Self {
        Self {
            room_id: room_id.into(),
            connection_id,
        }
    }
}

/// Broadcast a message to all members of a room
#[derive(Debug, Clone)]
pub struct BroadcastToRoom {
    /// The room to broadcast to
    pub room_id: RoomId,
    /// The message to broadcast
    pub message: Message,
    /// Optionally exclude the sender from receiving the broadcast
    pub exclude_sender: Option<ConnectionId>,
}

impl BroadcastToRoom {
    /// Create a new broadcast request
    #[must_use]
    pub fn new(room_id: impl Into<RoomId>, message: Message) -> Self {
        Self {
            room_id: room_id.into(),
            message,
            exclude_sender: None,
        }
    }

    /// Create a broadcast request excluding the sender
    #[must_use]
    pub fn excluding_sender(
        room_id: impl Into<RoomId>,
        message: Message,
        sender: ConnectionId,
    ) -> Self {
        Self {
            room_id: room_id.into(),
            message,
            exclude_sender: Some(sender),
        }
    }
}

/// Notification that a connection has disconnected
///
/// This is sent to the room manager when a WebSocket connection closes
/// so it can clean up room memberships.
#[derive(Debug, Clone)]
pub struct ConnectionDisconnected {
    /// The connection that disconnected
    pub connection_id: ConnectionId,
}

impl ConnectionDisconnected {
    /// Create a new disconnection notification
    #[must_use]
    pub fn new(connection_id: ConnectionId) -> Self {
        Self { connection_id }
    }
}

/// Request to get information about a room
#[derive(Debug, Clone)]
pub struct GetRoomInfo {
    /// The room to get info for
    pub room_id: RoomId,
}

impl GetRoomInfo {
    /// Create a new room info request
    #[must_use]
    pub fn new(room_id: impl Into<RoomId>) -> Self {
        Self {
            room_id: room_id.into(),
        }
    }
}

/// Response with room information
#[derive(Debug, Clone)]
pub struct RoomInfoResponse {
    /// The room ID
    pub room_id: RoomId,
    /// Number of members in the room
    pub member_count: usize,
    /// Whether the room exists
    pub exists: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[test]
    fn test_join_room_request() {
        let (tx, _rx) = mpsc::channel(32);
        let member = RoomMember::new(ConnectionId::new(), tx);
        let request = JoinRoomRequest::new("test-room", member);
        assert_eq!(request.room_id.as_str(), "test-room");
    }

    #[test]
    fn test_leave_room_request() {
        let conn_id = ConnectionId::new();
        let request = LeaveRoomRequest::new("test-room", conn_id);
        assert_eq!(request.room_id.as_str(), "test-room");
        assert_eq!(request.connection_id, conn_id);
    }

    #[test]
    fn test_broadcast_excluding_sender() {
        let sender_id = ConnectionId::new();
        let msg = Message::Text("hello".into());
        let broadcast = BroadcastToRoom::excluding_sender("room1", msg, sender_id);
        assert_eq!(broadcast.exclude_sender, Some(sender_id));
    }
}
