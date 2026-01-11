---
title: WebSocket
nextjs:
  metadata:
    title: WebSocket
    description: WebSocket support for real-time bidirectional communication with room management, broadcasting, and connection handling
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See [Dual HTTP+gRPC](/docs/dual-protocol) for protocol multiplexing basics. Check the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---

Build real-time applications with WebSocket support for bidirectional communication, room-based messaging, and efficient broadcasting.

---

## Overview

acton-service provides production-ready WebSocket support with:

- **Bidirectional messaging** - Full-duplex communication over a single connection
- **Room/channel support** - Actor-based room management for chat-like scenarios
- **Broadcasting** - Efficient message distribution to multiple connections
- **Same-port coexistence** - WebSocket upgrades seamlessly from HTTP
- **Connection management** - Unique connection IDs and lifecycle handling

{% callout type="note" title="Actor-Based Room Management" %}
Room management is powered by **acton-reactive** actors that handle join/leave operations, message broadcasting, and connection lifecycle. The `RoomManager` actor ensures thread-safe room operations. See [Reactive Architecture](/docs/reactive-architecture) for implementation details.
{% /callout %}

---

## Installation

Enable the WebSocket feature:

```toml
[dependencies]
acton-service = { version = "0.8", features = ["websocket"] }
```

Or add to existing features:

```toml
[dependencies]
acton-service = { version = "0.8", features = ["http", "websocket", "observability"] }
```

---

## Quick Start

### Basic WebSocket Handler

```rust
use acton_service::prelude::*;
use acton_service::websocket::{WebSocket, WebSocketUpgrade, Message};
use futures::{SinkExt, StreamExt};

async fn ws_handler(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: WebSocket) {
    // Send a welcome message
    let _ = socket.send(Message::Text("Welcome!".into())).await;

    // Echo incoming messages
    while let Some(Ok(msg)) = socket.next().await {
        match msg {
            Message::Text(text) => {
                let _ = socket.send(Message::Text(text)).await;
            }
            Message::Ping(data) => {
                let _ = socket.send(Message::Pong(data)).await;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/ws", get(ws_handler))
        })
        .build_routes();

    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await
}
```

### Testing

```bash
# Install websocat
cargo install websocat

# Connect to WebSocket endpoint
websocat ws://localhost:8080/api/v1/ws
```

---

## Configuration

WebSocket configuration is optional with sensible defaults:

```toml
# config.toml
[websocket]
# Maximum message size (default: 64KB)
max_message_size_bytes = 65536

# Ping interval to keep connection alive (default: 30s)
ping_interval_secs = 30

# Pong timeout before considering connection dead (default: 10s)
pong_timeout_secs = 10

[websocket.rooms]
# Enable room management (default: true)
enabled = true

# Maximum members per room (default: 1000)
max_members = 1000

# Maximum rooms a connection can join (default: 10)
max_rooms_per_connection = 10

# Room idle timeout before cleanup (default: 3600s / 1 hour)
idle_timeout_secs = 3600
```

### Environment Variable Override

```bash
ACTON_WEBSOCKET_MAX_MESSAGE_SIZE_BYTES=131072 cargo run
```

---

## Connection Management

### ConnectionId

Each WebSocket connection gets a unique identifier:

```rust
use acton_service::websocket::ConnectionId;

async fn handle_socket(socket: WebSocket) {
    let connection_id = ConnectionId::new();
    tracing::info!(connection_id = %connection_id, "New connection");

    // Use connection_id for tracking, logging, room membership, etc.
}
```

### WebSocketConnection

Track connection state with sender channel:

```rust
use acton_service::websocket::{ConnectionId, WebSocketConnection, Message};
use tokio::sync::mpsc;

let connection_id = ConnectionId::new();
let (tx, rx) = mpsc::channel::<Message>(32);

let connection = WebSocketConnection::new(connection_id, tx);

// Send message to this connection
connection.send(Message::Text("Hello".into())).await;
```

---

## Broadcasting

The `Broadcaster` manages multiple connections and enables efficient message distribution.

### Setup

```rust
use acton_service::websocket::{Broadcaster, ConnectionId, Message};
use std::sync::Arc;

// Create broadcaster as shared state
let broadcaster = Arc::new(Broadcaster::new());

// Register connections
broadcaster.register(connection_id, sender_channel).await;

// Unregister on disconnect
broadcaster.unregister(&connection_id).await;
```

### Broadcast Patterns

```rust
use acton_service::websocket::{Broadcaster, BroadcastTarget, Message};

// Broadcast to all connected clients
broadcaster.broadcast_all(Message::Text("Hello everyone!".into())).await;

// Broadcast to specific connections
broadcaster.broadcast_to(
    &[connection_id_1, connection_id_2],
    Message::Text("Private message".into()),
).await;

// Broadcast to all except specified (useful for echo prevention)
broadcaster.broadcast_except(
    &[sender_connection_id],
    Message::Text("Message from another user".into()),
).await;
```

### Complete Broadcasting Example

```rust
use acton_service::prelude::*;
use acton_service::websocket::{
    Broadcaster, ConnectionId, Message, WebSocket, WebSocketUpgrade,
};
use futures::{SinkExt, StreamExt};
use std::sync::Arc;

async fn ws_handler(
    ws: WebSocketUpgrade,
    Extension(broadcaster): Extension<Arc<Broadcaster>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, broadcaster))
}

async fn handle_socket(socket: WebSocket, broadcaster: Arc<Broadcaster>) {
    let (mut sender, mut receiver) = socket.split();
    let connection_id = ConnectionId::new();

    // Create channel for this connection
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Message>(32);

    // Register with broadcaster
    broadcaster.register(connection_id, tx.clone()).await;

    // Forward messages from channel to WebSocket
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Process incoming messages
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => {
                // Broadcast to all other connections
                broadcaster
                    .broadcast_except(
                        &[connection_id],
                        Message::Text(text),
                    )
                    .await;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    // Cleanup
    broadcaster.unregister(&connection_id).await;
    send_task.abort();
}

#[tokio::main]
async fn main() -> Result<()> {
    let broadcaster = Arc::new(Broadcaster::new());

    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router
                .route("/ws", get(ws_handler))
                .layer(Extension(broadcaster.clone()))
        })
        .build_routes();

    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await
}
```

---

## Room Management

For chat-like applications, use the actor-based `RoomManager`:

### Room Types

```rust
use acton_service::websocket::{RoomId, RoomMember, Room};

// Room identifier
let room_id = RoomId::new("general");

// Room member with sender channel
let member = RoomMember::new(connection_id, sender_channel);

// Room with members and metadata
let room = Room::new(room_id);
```

### RoomManager Actor

The `RoomManager` handles room operations through actor messages:

```rust
use acton_service::websocket::{
    RoomManager, SharedRoomManager,
    JoinRoomRequest, LeaveRoomRequest, BroadcastToRoom,
    ConnectionDisconnected, GetRoomInfo,
};
use acton_reactive::prelude::*;

// Spawn the room manager actor
let room_manager: SharedRoomManager = RoomManager::spawn().await?;

// Join a room
room_manager.send(JoinRoomRequest {
    room_id: RoomId::new("general"),
    connection_id,
    sender: tx.clone(),
}).await;

// Leave a room
room_manager.send(LeaveRoomRequest {
    room_id: RoomId::new("general"),
    connection_id,
}).await;

// Broadcast to room members
room_manager.send(BroadcastToRoom {
    room_id: RoomId::new("general"),
    message: Message::Text("Hello room!".into()),
    exclude: Some(connection_id), // Exclude sender
}).await;

// Handle connection disconnect (leaves all rooms)
room_manager.send(ConnectionDisconnected {
    connection_id,
}).await;

// Get room information
let info = room_manager.ask(GetRoomInfo {
    room_id: RoomId::new("general"),
}).await?;

if let Some(room_info) = info {
    println!("Room has {} members", room_info.member_count);
}
```

---

## Chat Server Example

Complete chat server with room support:

```rust
use acton_service::prelude::*;
use acton_service::websocket::{
    Broadcaster, ConnectionId, Message, WebSocket, WebSocketUpgrade,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum IncomingMessage {
    Join { room: String },
    Leave { room: String },
    Message { room: String, content: String },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OutgoingMessage {
    Joined { room: String },
    Left { room: String },
    Message { room: String, content: String, from: String },
    Error { message: String },
    System { message: String },
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    Extension(broadcaster): Extension<Arc<Broadcaster>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, broadcaster))
}

async fn handle_socket(socket: WebSocket, broadcaster: Arc<Broadcaster>) {
    let (mut sender, mut receiver) = socket.split();
    let connection_id = ConnectionId::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Message>(32);

    broadcaster.register(connection_id, tx.clone()).await;

    // Send welcome
    let welcome = OutgoingMessage::System {
        message: format!("Connected as {}", connection_id),
    };
    let _ = sender
        .send(Message::Text(serde_json::to_string(&welcome).unwrap().into()))
        .await;

    // Forward task
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Process messages
    while let Some(Ok(Message::Text(text))) = receiver.next().await {
        match serde_json::from_str::<IncomingMessage>(&text) {
            Ok(IncomingMessage::Join { room }) => {
                let response = OutgoingMessage::Joined { room };
                let _ = tx
                    .send(Message::Text(serde_json::to_string(&response).unwrap().into()))
                    .await;
            }
            Ok(IncomingMessage::Message { room, content }) => {
                let broadcast = OutgoingMessage::Message {
                    room,
                    content,
                    from: connection_id.to_string(),
                };
                let _ = broadcaster
                    .broadcast_except(
                        &[connection_id],
                        Message::Text(serde_json::to_string(&broadcast).unwrap().into()),
                    )
                    .await;
            }
            Ok(IncomingMessage::Leave { room }) => {
                let response = OutgoingMessage::Left { room };
                let _ = tx
                    .send(Message::Text(serde_json::to_string(&response).unwrap().into()))
                    .await;
            }
            Err(e) => {
                let error = OutgoingMessage::Error {
                    message: format!("Invalid message: {}", e),
                };
                let _ = tx
                    .send(Message::Text(serde_json::to_string(&error).unwrap().into()))
                    .await;
            }
        }
    }

    broadcaster.unregister(&connection_id).await;
    send_task.abort();
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let broadcaster = Arc::new(Broadcaster::new());

    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router
                .route("/ws", get(ws_handler))
                .layer(Extension(broadcaster.clone()))
        })
        .build_routes();

    let mut config = Config::<()>::default();
    config.service.name = "chat-server".to_string();
    config.service.port = 8080;

    tracing::info!("Chat server starting on ws://localhost:8080/api/v1/ws");

    ServiceBuilder::new()
        .with_config(config)
        .with_routes(routes)
        .build()
        .serve()
        .await
}
```

### Testing the Chat Server

```bash
# Terminal 1: Start server
cargo run --example chat-server --features websocket

# Terminal 2: Connect client 1
websocat ws://localhost:8080/api/v1/ws
{"type": "join", "room": "general"}
{"type": "message", "room": "general", "content": "Hello!"}

# Terminal 3: Connect client 2
websocat ws://localhost:8080/api/v1/ws
{"type": "join", "room": "general"}
# Receives: {"type":"message","room":"general","content":"Hello!","from":"..."}
```

---

## Message Types

WebSocket messages use Axum's `Message` enum:

```rust
use acton_service::websocket::Message;

match message {
    // UTF-8 text message
    Message::Text(text) => {
        println!("Received: {}", text);
    }

    // Binary data
    Message::Binary(data) => {
        println!("Binary: {} bytes", data.len());
    }

    // Ping - respond with Pong
    Message::Ping(data) => {
        let _ = socket.send(Message::Pong(data)).await;
    }

    // Pong - response to our Ping
    Message::Pong(_) => {
        // Connection is alive
    }

    // Close request
    Message::Close(frame) => {
        if let Some(cf) = frame {
            println!("Close: {} - {}", cf.code, cf.reason);
        }
    }
}
```

---

## Best Practices

### Use Structured Message Formats

Define clear message types with serde:

```rust
// Good - typed messages with serde
#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum WsMessage {
    Chat { content: String },
    Presence { user_id: String, status: String },
    Error { code: u32, message: String },
}

// Parse incoming
let msg: WsMessage = serde_json::from_str(&text)?;

// Send outgoing
let json = serde_json::to_string(&msg)?;
socket.send(Message::Text(json.into())).await?;
```

### Handle Connection Lifecycle

Always clean up on disconnect:

```rust
async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let connection_id = ConnectionId::new();

    // Register
    state.broadcaster.register(connection_id, tx).await;

    // ... handle messages ...

    // Always cleanup, even on error
    state.broadcaster.unregister(&connection_id).await;
    state.room_manager.send(ConnectionDisconnected { connection_id }).await;
}
```

### Implement Heartbeats

Keep connections alive with ping/pong:

```rust
use tokio::time::{interval, Duration};

let mut ping_interval = interval(Duration::from_secs(30));

loop {
    tokio::select! {
        _ = ping_interval.tick() => {
            if socket.send(Message::Ping(vec![])).await.is_err() {
                break; // Connection lost
            }
        }
        msg = receiver.next() => {
            match msg {
                Some(Ok(Message::Pong(_))) => {
                    // Connection confirmed alive
                }
                // ... handle other messages
            }
        }
    }
}
```

### Limit Message Size

Validate incoming message sizes:

```rust
if text.len() > config.websocket.max_message_size_bytes {
    let error = OutgoingMessage::Error {
        message: "Message too large".to_string(),
    };
    let _ = tx.send(Message::Text(serde_json::to_string(&error)?.into())).await;
    continue;
}
```

### Use Appropriate Channel Buffer Sizes

Balance memory usage and throughput:

```rust
// Small buffer for low-traffic connections
let (tx, rx) = mpsc::channel::<Message>(16);

// Larger buffer for high-throughput scenarios
let (tx, rx) = mpsc::channel::<Message>(256);
```

---

## Error Handling

Handle WebSocket errors gracefully:

```rust
while let Some(result) = receiver.next().await {
    match result {
        Ok(Message::Text(text)) => {
            // Handle message
        }
        Ok(Message::Close(frame)) => {
            tracing::info!("Client closed connection");
            break;
        }
        Err(e) => {
            tracing::warn!(error = %e, "WebSocket error");
            break; // Exit on error
        }
        _ => {}
    }
}
```

---

## Combining with Other Features

### WebSocket + JWT Authentication

Authenticate during the upgrade:

```rust
use acton_service::middleware::Claims;

async fn authenticated_ws_handler(
    ws: WebSocketUpgrade,
    claims: Claims, // Extracted by JWT middleware
    Extension(broadcaster): Extension<Arc<Broadcaster>>,
) -> impl IntoResponse {
    let user_id = claims.sub.clone();
    ws.on_upgrade(move |socket| handle_authenticated_socket(socket, user_id, broadcaster))
}

async fn handle_authenticated_socket(
    socket: WebSocket,
    user_id: String,
    broadcaster: Arc<Broadcaster>,
) {
    tracing::info!(user_id = %user_id, "Authenticated WebSocket connection");
    // ... handle connection with known user identity
}
```

### WebSocket + Database

Access database in WebSocket handlers:

```rust
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let db = state.db().await.expect("Database available");

    // Load user's chat history, etc.
    let history = sqlx::query!("SELECT * FROM messages LIMIT 50")
        .fetch_all(db)
        .await
        .unwrap_or_default();

    // ... send history to client
}
```

---

## Troubleshooting

### Connection Immediately Closes

**Cause**: Handler panics or returns early.

**Solution**: Add error handling and logging:

```rust
async fn handle_socket(socket: WebSocket) {
    tracing::info!("New connection");

    // ... your code ...

    tracing::info!("Connection closed");
}
```

### Messages Not Broadcasting

**Cause**: Connection not registered with broadcaster.

**Solution**: Verify registration:

```rust
broadcaster.register(connection_id, tx.clone()).await;
tracing::debug!(connection_id = %connection_id, "Registered with broadcaster");
```

### Memory Growing Unbounded

**Cause**: Connections not unregistered on disconnect.

**Solution**: Always clean up:

```rust
// Use Drop guard or explicit cleanup
broadcaster.unregister(&connection_id).await;
```

### "ws" Feature Not Found

**Cause**: Axum's `ws` feature not enabled.

**Solution**: Ensure workspace Cargo.toml has:

```toml
axum = { version = "0.8", features = ["macros", "ws"] }
```

---

## Related Features

- **[gRPC Guide](/docs/grpc-guide)** - Bidirectional streaming alternative
- **[Events (NATS)](/docs/events)** - Pub/sub messaging between services
- **[JWT Authentication](/docs/jwt-auth)** - Authenticate WebSocket connections
- **[Reactive Architecture](/docs/reactive-architecture)** - Actor-based room management details
- **[Observability](/docs/observability)** - Trace WebSocket connections
