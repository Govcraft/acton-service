//! WebSocket Chat Server Example
//!
//! Demonstrates:
//! - WebSocket upgrades from HTTP
//! - Room-based chat functionality
//! - Broadcasting messages to room members
//! - Connection management with the Broadcaster
//!
//! ## Running
//!
//! ```bash
//! cargo run --example chat-server --features websocket
//! ```
//!
//! ## Testing
//!
//! You can use `websocat` or a browser-based WebSocket client:
//!
//! ```bash
//! # Install websocat: cargo install websocat
//! websocat ws://localhost:8080/api/v1/ws
//! ```
//!
//! Then send JSON messages:
//! ```json
//! {"type": "join", "room": "general"}
//! {"type": "message", "room": "general", "content": "Hello everyone!"}
//! {"type": "leave", "room": "general"}
//! ```

use acton_service::prelude::*;
use acton_service::websocket::{Broadcaster, ConnectionId, Message, WebSocket, WebSocketUpgrade};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Chat message types that clients can send
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum IncomingMessage {
    /// Join a chat room
    Join { room: String },
    /// Leave a chat room
    Leave { room: String },
    /// Send a message to a room
    Message { room: String, content: String },
}

/// Messages sent from server to clients
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OutgoingMessage {
    /// Confirmation of joining a room
    Joined { room: String },
    /// Confirmation of leaving a room
    Left { room: String },
    /// A chat message from another user
    Message {
        room: String,
        content: String,
        from: String,
    },
    /// An error occurred
    Error { message: String },
    /// System notification
    System { message: String },
}

/// HTTP handler for WebSocket upgrade
async fn ws_handler(
    ws: WebSocketUpgrade,
    Extension(broadcaster): Extension<Arc<Broadcaster>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, broadcaster))
}

/// Handle an individual WebSocket connection
async fn handle_socket(socket: WebSocket, broadcaster: Arc<Broadcaster>) {
    let (mut sender, mut receiver) = socket.split();
    let connection_id = ConnectionId::new();

    // Create a channel for sending messages to this connection
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Message>(32);

    // Register this connection with the broadcaster
    broadcaster.register(connection_id, tx.clone()).await;

    tracing::info!(connection_id = %connection_id, "New WebSocket connection");

    // Send welcome message
    let welcome = OutgoingMessage::System {
        message: format!("Welcome! Your connection ID is {}", connection_id),
    };
    let _ = sender
        .send(Message::Text(
            serde_json::to_string(&welcome).unwrap().into(),
        ))
        .await;

    // Spawn a task to forward messages from the channel to the WebSocket
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Process incoming messages
    let broadcaster_clone = broadcaster.clone();
    let conn_id_str = connection_id.to_string();

    while let Some(result) = receiver.next().await {
        match result {
            Ok(Message::Text(text)) => match serde_json::from_str::<IncomingMessage>(&text) {
                Ok(msg) => {
                    handle_incoming_message(
                        msg,
                        connection_id,
                        &conn_id_str,
                        &tx,
                        &broadcaster_clone,
                    )
                    .await;
                }
                Err(e) => {
                    let error = OutgoingMessage::Error {
                        message: format!("Invalid message format: {}", e),
                    };
                    let _ = tx
                        .send(Message::Text(serde_json::to_string(&error).unwrap().into()))
                        .await;
                }
            },
            Ok(Message::Ping(data)) => {
                let _ = tx.send(Message::Pong(data)).await;
            }
            Ok(Message::Close(_)) => {
                break;
            }
            Err(e) => {
                tracing::warn!(connection_id = %connection_id, error = %e, "WebSocket error");
                break;
            }
            _ => {}
        }
    }

    // Cleanup
    broadcaster.unregister(&connection_id).await;
    send_task.abort();

    tracing::info!(connection_id = %connection_id, "WebSocket connection closed");
}

/// Handle an incoming chat message
async fn handle_incoming_message(
    msg: IncomingMessage,
    connection_id: ConnectionId,
    conn_id_str: &str,
    tx: &tokio::sync::mpsc::Sender<Message>,
    broadcaster: &Broadcaster,
) {
    match msg {
        IncomingMessage::Join { room } => {
            tracing::info!(connection_id = %connection_id, room = %room, "User joining room");

            let response = OutgoingMessage::Joined { room: room.clone() };
            let _ = tx
                .send(Message::Text(
                    serde_json::to_string(&response).unwrap().into(),
                ))
                .await;

            // Notify others in the room (in a full implementation, you'd track room membership)
            let notification = OutgoingMessage::System {
                message: format!("User {} joined the room", conn_id_str),
            };
            let _ = broadcaster
                .broadcast_except(
                    &[connection_id],
                    Message::Text(serde_json::to_string(&notification).unwrap().into()),
                )
                .await;
        }
        IncomingMessage::Leave { room } => {
            tracing::info!(connection_id = %connection_id, room = %room, "User leaving room");

            let response = OutgoingMessage::Left { room };
            let _ = tx
                .send(Message::Text(
                    serde_json::to_string(&response).unwrap().into(),
                ))
                .await;
        }
        IncomingMessage::Message { room, content } => {
            tracing::debug!(
                connection_id = %connection_id,
                room = %room,
                "Broadcasting message"
            );

            // Broadcast to all connections (in a full implementation, filter by room)
            let broadcast_msg = OutgoingMessage::Message {
                room,
                content,
                from: conn_id_str.to_string(),
            };
            let _ = broadcaster
                .broadcast_except(
                    &[connection_id],
                    Message::Text(serde_json::to_string(&broadcast_msg).unwrap().into()),
                )
                .await;

            // Echo back to sender as confirmation
            let _ = tx
                .send(Message::Text(
                    serde_json::to_string(&broadcast_msg).unwrap().into(),
                ))
                .await;
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    // Create the broadcaster as shared state
    let broadcaster = Arc::new(Broadcaster::new());

    // Build routes with WebSocket endpoint
    // Note: We add the broadcaster as an Extension layer
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router
                .route("/ws", get(ws_handler))
                .layer(Extension(broadcaster.clone()))
        })
        .build_routes();

    // Build and configure the service
    let mut config = Config::<()>::default();
    config.service.name = "chat-server".to_string();
    config.service.port = 8080;

    tracing::info!("Starting chat server on http://localhost:8080");
    tracing::info!("Connect via WebSocket at ws://localhost:8080/api/v1/ws");

    // Build and serve
    ServiceBuilder::new()
        .with_config(config)
        .with_routes(routes)
        .build()
        .serve()
        .await?;

    Ok(())
}
