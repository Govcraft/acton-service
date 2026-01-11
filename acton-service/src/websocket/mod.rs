//! WebSocket support for acton-service
//!
//! This module provides WebSocket server functionality that integrates with
//! the existing HTTP server. WebSocket connections upgrade from HTTP on the
//! same port, allowing seamless coexistence with REST and gRPC.
//!
//! ## Features
//!
//! - **Bidirectional messaging**: Full duplex communication
//! - **Room/channel support**: Actor-based room management
//! - **Broadcasting**: Efficient message distribution
//! - **Authentication**: JWT integration for WebSocket handshake
//! - **Rate limiting**: Per-connection message rate limits
//!
//! ## Example
//!
//! ```rust,ignore
//! use acton_service::prelude::*;
//! use acton_service::websocket::{WebSocketUpgrade, WebSocket};
//!
//! async fn ws_handler(
//!     ws: WebSocketUpgrade,
//!     State(state): State<AppState>,
//! ) -> impl IntoResponse {
//!     ws.on_upgrade(|socket| handle_socket(socket, state))
//! }
//!
//! async fn handle_socket(mut socket: WebSocket, state: AppState) {
//!     while let Some(Ok(msg)) = socket.recv().await {
//!         // Handle WebSocket messages
//!     }
//! }
//! ```

mod broadcast;
mod config;
mod handler;
mod messages;
mod rooms;

// Re-exports
pub use broadcast::{BroadcastTarget, Broadcaster};
pub use config::{RoomConfig, WebSocketConfig};
pub use handler::{ConnectionId, WebSocketConnection};
pub use messages::{
    BroadcastToRoom, ConnectionDisconnected, GetRoomInfo, JoinRoomRequest, LeaveRoomRequest,
    RoomInfoResponse,
};
pub use rooms::{Room, RoomId, RoomManager, RoomMember, SharedRoomManager};

// Re-export axum WebSocket types for convenience
pub use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
