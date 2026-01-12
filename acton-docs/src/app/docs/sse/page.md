---
title: Server-Sent Events (SSE)
nextjs:
  metadata:
    title: Server-Sent Events
    description: Real-time server-to-client updates with SSE broadcaster, HTMX integration, and connection management
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---

The `sse` feature provides Server-Sent Events for real-time server-to-client updates. Unlike WebSockets, SSE is one-way (server to client only), works over standard HTTP, and includes automatic reconnection—making it ideal for notifications, live data updates, and HTMX-powered real-time UIs.

## SSE vs WebSocket

| Aspect | SSE | WebSocket |
|--------|-----|-----------|
| **Direction** | Server to client only | Bidirectional |
| **Protocol** | HTTP/2 | Separate WebSocket protocol |
| **Reconnection** | Automatic (browser handles it) | Manual implementation needed |
| **Complexity** | Simple | More complex |
| **Best for** | Notifications, live feeds, progress | Chat, games, collaborative editing |

**Use SSE when** you need real-time updates pushed from server to client. **Use WebSocket when** clients need to send frequent messages back to the server.

For HTMX applications, SSE is usually the better choice—HTMX already handles client-to-server communication via HTTP requests.

## Quick Start

### 1. Enable the Feature

```toml
[dependencies]
acton-service = { version = "{{version}}", features = ["sse"] }
```

Or include it with other HTMX features:

```toml
acton-service = { version = "{{version}}", features = ["htmx-full"] }
```

### 2. Create a Broadcaster

```rust
use acton_service::prelude::*;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let broadcaster = Arc::new(SseBroadcaster::new());

    let app = Router::new()
        .route("/events", get(events))
        .route("/notify", post(send_notification))
        .layer(Extension(broadcaster));

    // ...
}
```

### 3. Create the SSE Endpoint

```rust
use std::convert::Infallible;
use futures::stream::{self, Stream};

async fn events(
    Extension(broadcaster): Extension<Arc<SseBroadcaster>>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let rx = broadcaster.subscribe();

    let stream = stream::unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Ok(msg) => {
                let mut event = SseEvent::default().data(msg.data);
                if let Some(event_type) = msg.event_type {
                    event = event.event(event_type);
                }
                Some((Ok(event), rx))
            }
            Err(_) => None,
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}
```

### 4. Broadcast Events

```rust
async fn send_notification(
    Extension(broadcaster): Extension<Arc<SseBroadcaster>>,
    Json(payload): Json<NotificationPayload>,
) -> impl IntoResponse {
    let msg = BroadcastMessage::named("notification", &payload.message);
    broadcaster.broadcast(msg).ok();

    StatusCode::OK
}
```

### 5. Connect from the Client

With HTMX's SSE extension:

```html
<script src="https://unpkg.com/htmx-ext-sse@2.2.2/sse.js"></script>

<div hx-ext="sse" sse-connect="/events">
    <div id="notifications" sse-swap="notification">
        <!-- Notifications appear here -->
    </div>
</div>
```

---

## SseBroadcaster API

### Creating a Broadcaster

```rust
// Default capacity (256 messages)
let broadcaster = SseBroadcaster::new();

// Custom capacity
let broadcaster = SseBroadcaster::with_capacity(1024);

// Share across handlers with Arc
let broadcaster = Arc::new(SseBroadcaster::new());
```

Store the broadcaster in application state and share it via `Extension`.

### Subscribing Clients

Each client that connects to your SSE endpoint subscribes to receive broadcasts:

```rust
// In your SSE endpoint handler
let rx = broadcaster.subscribe();  // Returns a broadcast receiver
```

The receiver is a `tokio::sync::broadcast::Receiver<BroadcastMessage>`.

### Broadcasting Events

Send events to all connected clients:

```rust
// Simple message
broadcaster.broadcast(BroadcastMessage::new("Hello, world!")).ok();

// Named event (matches sse-swap="event-name" in HTMX)
broadcaster.broadcast(BroadcastMessage::named("task-created", html)).ok();

// JSON data
let msg = BroadcastMessage::json_named("stats", &StatsPayload { count: 42 })?;
broadcaster.broadcast(msg).ok();
```

### BroadcastMessage Construction

```rust
// Plain data
BroadcastMessage::new("Hello")

// Named event with data
BroadcastMessage::named("notification", "<div>New message!</div>")

// JSON serialization
BroadcastMessage::json(&data)?
BroadcastMessage::json_named("update", &data)?

// With event ID (for reconnection)
BroadcastMessage::new("data").with_id("msg-123")
```

### Channel-Based Broadcasting

For user-specific or topic-based messaging, use channels:

```rust
// Subscribe to a specific channel
let rx = broadcaster.subscribe_channel("user-123").await;

// Broadcast to channel subscribers only
broadcaster.broadcast_to_channel("user-123", msg).await.ok();

// Check channel existence
if broadcaster.has_channel("user-123").await {
    // ...
}
```

### Connection Management

```rust
// Get connection count
let count = broadcaster.connection_count().await;

// Get channel count
let channels = broadcaster.channel_count().await;

// Register connection with metadata
let id = ConnectionId::new();
broadcaster.register(id).await;
broadcaster.register_with_channels(id, vec!["channel1".into()]).await;

// Unregister on disconnect
broadcaster.unregister(&id).await;
```

---

## HTMX Integration

HTMX's SSE extension makes it easy to update page elements when events arrive.

### Basic Setup

Include the SSE extension in your HTML:

```html
<script src="https://unpkg.com/htmx.org@2.0.4"></script>
<script src="https://unpkg.com/htmx-ext-sse@2.2.2/sse.js"></script>
```

Connect to your SSE endpoint:

```html
<div hx-ext="sse" sse-connect="/events">
    <!-- Elements that receive SSE updates -->
</div>
```

### Swapping Content

The `sse-swap` attribute specifies which event updates which element:

```html
<div hx-ext="sse" sse-connect="/events">
    <!-- Updates when "notification" event arrives -->
    <div id="notifications" sse-swap="notification"></div>

    <!-- Updates when "task-list" event arrives -->
    <ul id="tasks" sse-swap="task-list"></ul>
</div>
```

Server sends:
```rust
broadcaster.broadcast(BroadcastMessage::named(
    "notification",
    "<div class='alert'>New notification!</div>"
)).ok();
```

### HTMX SSE Helpers

Use the provided helpers to create properly formatted SSE events:

```rust
use acton_service::prelude::*;

// Create event that triggers HTMX swap
let event = htmx_event("task-update", "<li>New task</li>");

// Create event with JSON data
let event = htmx_json_event("stats", &StatsData { count: 5 })?;

// Trigger client-side HTMX event (no content)
let event = htmx_trigger("refresh-stats");

// Out-of-band swap event
let event = htmx_oob_event("notification", r#"<div id="alert" hx-swap-oob="true">Alert!</div>"#);

// Close event (tells client to disconnect)
let event = htmx_close_event();
```

---

## Common Patterns

### Live Notifications

**Server:**
```rust
async fn create_task(
    Extension(broadcaster): Extension<Arc<SseBroadcaster>>,
    Extension(store): Extension<SharedStore>,
    Form(form): Form<CreateTaskForm>,
) -> impl IntoResponse {
    let task = store.write().await.add(form.title);

    // Broadcast new task to all clients
    let html = TaskItemTemplate { task: &task }.render().unwrap();
    broadcaster.broadcast(BroadcastMessage::named("task-created", html)).ok();

    HxRedirect::to("/tasks")
}
```

**Client:**
```html
<div hx-ext="sse" sse-connect="/events">
    <ul id="task-list">
        {​% for task in tasks %}
            {​% include "tasks/item.html" %}
        {​% endfor %}
    </ul>

    <!-- New tasks appear via SSE -->
    <template sse-swap="task-created" hx-swap="beforeend" hx-target="#task-list">
    </template>
</div>
```

### Real-Time Statistics

**Server:**
```rust
async fn toggle_task(
    Extension(broadcaster): Extension<Arc<SseBroadcaster>>,
    Extension(store): Extension<SharedStore>,
    Path(id): Path<u64>,
) -> impl IntoResponse {
    store.write().await.toggle(id);
    let (total, completed, pending) = store.read().await.stats();

    // Broadcast stats update
    let stats_html = format!(
        r#"<span id="completed-count">{}</span>"#,
        completed
    );
    broadcaster.broadcast(BroadcastMessage::named("stats-update", stats_html)).ok();

    // Return task item response for HTMX
    // ...
}
```

**Client:**
```html
<div hx-ext="sse" sse-connect="/events">
    <div class="stats">
        <span id="completed-count" sse-swap="stats-update">{{ completed }}</span>
        completed
    </div>
</div>
```

### Progress Indicators

**Server:**
```rust
async fn process_file(
    Extension(broadcaster): Extension<Arc<SseBroadcaster>>,
) -> impl IntoResponse {
    for i in 0..=100 {
        // Simulate work
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Broadcast progress
        let html = format!(r#"<div class="progress-bar" style="width: {}%">{}</div>"#, i, i);
        broadcaster.broadcast(BroadcastMessage::named("progress", html)).ok();
    }

    broadcaster.broadcast(BroadcastMessage::named("progress", "<div>Complete!</div>")).ok();

    StatusCode::OK
}
```

**Client:**
```html
<div hx-ext="sse" sse-connect="/events">
    <div id="progress" sse-swap="progress">
        <div class="progress-bar" style="width: 0%">0</div>
    </div>
</div>
```

### User-Specific Updates

Use channels for user-targeted messages:

**Server:**
```rust
async fn events(
    Extension(broadcaster): Extension<Arc<SseBroadcaster>>,
    auth: TypedSession<AuthSession>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let user_id = auth.data().user_id.clone().unwrap_or_default();

    // Subscribe to user-specific channel
    let rx = broadcaster.subscribe_channel(&user_id).await;

    // ... create stream from rx
}

async fn send_dm(
    Extension(broadcaster): Extension<Arc<SseBroadcaster>>,
    Path(user_id): Path<String>,
    Json(message): Json<MessagePayload>,
) -> impl IntoResponse {
    // Send only to this user's channel
    broadcaster.broadcast_to_channel(&user_id, BroadcastMessage::named(
        "dm",
        format!("<div class='message'>{}</div>", message.text)
    )).await.ok();

    StatusCode::OK
}
```

---

## Connection Lifecycle

### Initial Connection

When a client connects to your SSE endpoint:

1. Browser sends GET request to `/events`
2. Your handler creates a receiver from the broadcaster
3. You return `Sse<Stream>` with keep-alive enabled
4. Browser receives `200 OK` with `Content-Type: text/event-stream`
5. Connection stays open, receiving events as they're broadcast

### Keep-Alive

SSE connections can be dropped by proxies if idle. Use keep-alive to send periodic ping events:

```rust
Sse::new(stream).keep_alive(
    KeepAlive::new()
        .interval(Duration::from_secs(15))
        .text("ping")
)
```

### Automatic Reconnection

If the connection drops, the browser automatically reconnects. Use event IDs to support replay:

```rust
// Include ID in broadcast
broadcaster.broadcast(
    BroadcastMessage::new("data")
        .with_id(format!("msg-{}", msg_id))
).ok();
```

The browser sends `Last-Event-ID` header on reconnect. You can use this to replay missed events.

### Cleanup

When a client disconnects, the broadcast receiver is dropped. The `SseBroadcaster` automatically handles cleanup—no manual unsubscribe needed for basic usage.

For tracked connections:

```rust
async fn events(
    Extension(broadcaster): Extension<Arc<SseBroadcaster>>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let id = ConnectionId::new();
    broadcaster.register(id).await;

    // Create stream with cleanup on drop
    let rx = broadcaster.subscribe();
    let stream = // ... create stream

    // Note: For proper cleanup, use a wrapper that calls unregister
    // This is a simplified example
    Sse::new(stream)
}
```

---

## Error Handling

### Broadcast Errors

`broadcast()` returns a `Result`. Handle the error if needed:

```rust
match broadcaster.broadcast(msg) {
    Ok(count) => tracing::debug!("Sent to {} clients", count),
    Err(e) => tracing::warn!("Broadcast failed: {:?}", e),
}
```

If no clients are connected, the broadcast "succeeds" with count 0.

### Connection Drops

The stream automatically ends when a client disconnects. The receiver returns an error, which ends the stream gracefully.

### Fallback for Non-SSE Clients

For clients that don't support SSE, provide a polling fallback:

```rust
async fn get_notifications(
    Extension(store): Extension<SharedStore>,
) -> impl IntoResponse {
    // Polling endpoint for non-SSE clients
    let notifications = store.read().await.recent_notifications();
    Json(notifications)
}
```

---

## Performance

### Connection Limits

Browsers limit SSE connections per domain (typically 6 for HTTP/1.1, more for HTTP/2). Share a single SSE connection across your entire page rather than connecting from multiple elements.

### Memory Usage

Each connection holds a broadcast receiver. Monitor connection count in production:

```rust
async fn health(
    Extension(broadcaster): Extension<Arc<SseBroadcaster>>,
) -> Json<HealthStatus> {
    Json(HealthStatus {
        sse_connections: broadcaster.connection_count().await,
    })
}
```

### Event Size

Keep events small. Send IDs or minimal data, letting the client fetch details if needed:

```rust
// Instead of sending full task data
broadcaster.broadcast(BroadcastMessage::named(
    "task-created",
    json!({"id": task.id}).to_string()
)).ok();

// Let HTMX fetch the rendered task
// Client: <div sse-swap="task-created" hx-get="/tasks/{id}" hx-trigger="load">
```

### Scaling

For multi-server deployments, use Redis pub/sub to synchronize broadcasts:

```rust
// Pseudocode for multi-server setup
async fn broadcast_all_servers(msg: BroadcastMessage) {
    // Publish to Redis
    redis.publish("sse-events", msg).await;
}

// Each server subscribes to Redis
async fn redis_listener(broadcaster: Arc<SseBroadcaster>) {
    let mut sub = redis.subscribe("sse-events").await;
    while let Some(msg) = sub.next().await {
        broadcaster.broadcast(msg).ok();
    }
}
```

---

## Security

### Authentication

Authenticate SSE connections the same as regular requests:

```rust
async fn events(
    auth: TypedSession<AuthSession>,
    Extension(broadcaster): Extension<Arc<SseBroadcaster>>,
) -> impl IntoResponse {
    if !auth.data().is_authenticated() {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    // ... create SSE stream
}
```

Session cookies are sent with SSE connections automatically.

### Authorization

Filter broadcasts based on user permissions:

```rust
// Subscribe to user's channels only
let user_channels = get_user_channels(&user_id).await;
for channel in user_channels {
    broadcaster.subscribe_channel(&channel).await;
}
```

### Rate Limiting

Limit SSE connection rate to prevent abuse:

```rust
// Use your rate limiting middleware
.route("/events", get(events))
    .layer(governor::governor(
        governor::Config::default()
            .per_second(10)  // 10 connection attempts per second max
    ))
```

---

## Troubleshooting

### "Connection immediately closes"

Check:
- CORS headers if connecting cross-origin
- Authentication middleware (401/403 responses close SSE)
- Response headers include `Content-Type: text/event-stream`

### "No events received"

Verify:
- Broadcaster is shared correctly (wrapped in `Arc`, same instance)
- Event names match `sse-swap` attributes
- Events are being broadcast (add logging)

### "Events not triggering HTMX"

Check:
- SSE extension is loaded (`<script src="...sse.js">`)
- Event format is correct (`data: <content>\n\n`)
- `sse-swap` attribute matches event name

### "High memory usage"

Monitor connection count. If connections aren't being cleaned up:
- Check that streams properly end on disconnect
- Verify `KeepAlive` is configured
- Look for leaked receivers

### "Events arrive out of order"

SSE guarantees order per connection. If you're seeing out-of-order:
- Check for multiple SSE connections (only use one per page)
- Use event IDs if order is critical
- Consider buffering/sequencing on the client

---

## Complete Example

The Task Manager example demonstrates SSE with HTMX. Key files:

**SSE Endpoint** (`examples/htmx/task-manager.rs`):
```rust
async fn events(
    Extension(broadcaster): Extension<Arc<SseBroadcaster>>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let rx = broadcaster.subscribe();

    let stream = stream::unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Ok(msg) => {
                let mut event = SseEvent::default().data(msg.data);
                if let Some(event_type) = msg.event_type {
                    event = event.event(event_type);
                }
                Some((Ok(event), rx))
            }
            Err(_) => None,
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}
```

**Client Connection** (`templates/index.html`):
```html
<div hx-ext="sse" sse-connect="/events">
    <ul id="task-list" sse-swap="task-update">
        {​% for task in tasks %}
            {​% include "tasks/item.html" %}
        {​% endfor %}
    </ul>
</div>
```

Run the example:
```bash
cargo run --manifest-path=acton-service/Cargo.toml --example task-manager --features htmx-full
```

See {% link href=githubUrl("/tree/main/acton-service/examples/htmx") %}examples/htmx/{% /link %} for the complete source.

---

## Next Steps

- [HTMX Integration](/docs/htmx) - Overview of all HTMX features
- [Askama Templates](/docs/askama) - Render HTML events with templates
- [WebSocket](/docs/websocket) - For bidirectional real-time communication
- [Examples](/docs/examples#htmx) - Complete working examples
