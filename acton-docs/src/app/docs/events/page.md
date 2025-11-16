---
title: Events (NATS)
nextjs:
  metadata:
    title: Events (NATS)
    description: NATS JetStream integration for event-driven microservices with pub/sub patterns, stream processing, and automatic connection management
---

Build event-driven microservices with NATS JetStream for reliable message delivery, stream processing, and service decoupling.

---

## Overview

acton-service provides production-ready NATS integration through `async-nats` with automatic connection management, JetStream support, and health monitoring. NATS enables event-driven architectures with publish/subscribe patterns, stream processing, and guaranteed message delivery.

## Installation

Enable the events feature:

```toml
[dependencies]
acton-service = { version = "0.2", features = ["events", "http", "observability"] }
```

## Configuration

NATS configuration follows XDG standards with environment variable overrides:

```toml
# ~/.config/acton-service/my-service/config.toml
[nats]
url = "nats://localhost:4222"
max_reconnects = 10
optional = false  # Readiness fails if NATS is unavailable
```

### Environment Variable Override

```bash
ACTON_NATS_URL=nats://localhost:4222 cargo run
```

### Connection Settings

NATS connection with automatic retry and reconnection:

- **url**: NATS server URL (default: `nats://localhost:4222`)
- **max_reconnects**: Maximum reconnection attempts (default: unlimited)
- **reconnect_delay**: Delay between reconnect attempts (default: 1s)

## Basic Usage

Access NATS through `AppState` in your application:

```rust
use acton_service::prelude::*;

async fn process_event(msg: async_nats::Message) -> Result<()> {
    let payload: serde_json::Value = serde_json::from_slice(&msg.payload)?;
    info!("Processing event: {:?}", payload);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;
    init_tracing(&config)?;

    let state = AppState::builder()
        .config(config.clone())
        .build()
        .await?;

    let nats = state.nats().await.ok_or(Error::Internal("NATS not available"))?;
    let mut subscriber = nats.subscribe("events.>").await?;

    while let Some(msg) = subscriber.next().await {
        if let Err(e) = process_event(msg).await {
            error!("Event processing failed: {}", e);
        }
    }

    Ok(())
}
```

## Publish/Subscribe Pattern

### Publishing Events

Publish events to NATS subjects:

```rust
use acton_service::prelude::*;

#[derive(Serialize)]
struct UserCreatedEvent {
    user_id: i64,
    email: String,
    created_at: String,
}

async fn create_user(
    State(state): State<AppState>,
    Json(request): Json<CreateUserRequest>,
) -> Result<Json<User>> {
    let db = state.db().await.ok_or(Error::Internal("Database not available"))?;

    // Create user in database
    let user = sqlx::query_as!(
        User,
        "INSERT INTO users (name, email) VALUES ($1, $2) RETURNING *",
        request.name,
        request.email
    )
    .fetch_one(db)
    .await?;

    // Publish event
    let nats = state.nats().await.ok_or(Error::Internal("NATS not available"))?;
    let event = UserCreatedEvent {
        user_id: user.id,
        email: user.email.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    nats.publish(
        "users.created",
        serde_json::to_vec(&event)?.into()
    ).await?;

    Ok(Json(user))
}
```

### Subscribing to Events

Subscribe to subjects with wildcard support:

```rust
async fn subscribe_user_events(state: AppState) -> Result<()> {
    let nats = state.nats().await.ok_or(Error::Internal("NATS not available"))?;

    // Subscribe to all user events
    let mut subscriber = nats.subscribe("users.*").await?;

    while let Some(msg) = subscriber.next().await {
        let subject = &msg.subject;

        match subject.as_str() {
            "users.created" => handle_user_created(msg).await?,
            "users.updated" => handle_user_updated(msg).await?,
            "users.deleted" => handle_user_deleted(msg).await?,
            _ => warn!("Unknown event type: {}", subject),
        }
    }

    Ok(())
}
```

## JetStream Integration

JetStream provides guaranteed message delivery and stream processing:

```rust
use async_nats::jetstream;

async fn setup_jetstream(state: &AppState) -> Result<()> {
    let nats = state.nats().await.ok_or(Error::Internal("NATS not available"))?;
    let js = jetstream::new(nats.clone());

    // Create or update stream
    js.create_stream(jetstream::stream::Config {
        name: "EVENTS".to_string(),
        subjects: vec!["events.>".to_string()],
        max_messages: 1_000_000,
        max_age: std::time::Duration::from_secs(86400 * 7), // 7 days
        ..Default::default()
    })
    .await?;

    Ok(())
}
```

### Durable Consumers

Create durable consumers for reliable event processing:

```rust
async fn consume_events(state: AppState) -> Result<()> {
    let nats = state.nats().await.ok_or(Error::Internal("NATS not available"))?;
    let js = jetstream::new(nats.clone());

    // Create durable consumer
    let consumer = js
        .create_consumer_on_stream(
            jetstream::consumer::Config {
                durable_name: Some("event-processor".to_string()),
                ack_policy: jetstream::consumer::AckPolicy::Explicit,
                ..Default::default()
            },
            "EVENTS",
        )
        .await?;

    let mut messages = consumer.messages().await?;

    while let Some(msg) = messages.next().await {
        match msg {
            Ok(msg) => {
                if let Err(e) = process_jetstream_event(&msg).await {
                    error!("Event processing failed: {}", e);
                    msg.nak().await?;
                } else {
                    msg.ack().await?;
                }
            }
            Err(e) => error!("Message receive error: {}", e),
        }
    }

    Ok(())
}
```

## Event-Driven Architecture Patterns

### Service Decoupling

Use NATS to decouple services:

```rust
// Order Service - publishes events
async fn create_order(
    State(state): State<AppState>,
    Json(order): Json<CreateOrderRequest>,
) -> Result<Json<Order>> {
    let db = state.db().await.ok_or(Error::Internal("Database not available"))?;
    let nats = state.nats().await.ok_or(Error::Internal("NATS not available"))?;

    // Create order
    let order = insert_order(db, order).await?;

    // Publish event - other services react independently
    let event = OrderCreatedEvent {
        order_id: order.id,
        user_id: order.user_id,
        total: order.total,
    };

    nats.publish("orders.created", serde_json::to_vec(&event)?.into())
        .await?;

    Ok(Json(order))
}

// Email Service - consumes events
async fn email_worker(state: AppState) -> Result<()> {
    let nats = state.nats().await.ok_or(Error::Internal("NATS not available"))?;
    let mut subscriber = nats.subscribe("orders.created").await?;

    while let Some(msg) = subscriber.next().await {
        let event: OrderCreatedEvent = serde_json::from_slice(&msg.payload)?;
        send_order_confirmation_email(&event).await?;
    }

    Ok(())
}
```

### Request/Reply Pattern

Implement synchronous request/reply over NATS:

```rust
// Service A - makes request
async fn get_user_info(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
) -> Result<Json<UserInfo>> {
    let nats = state.nats().await.ok_or(Error::Internal("NATS not available"))?;

    let request = serde_json::to_vec(&UserInfoRequest { user_id })?;

    // Send request and wait for reply (with timeout)
    let reply = nats
        .request("users.info", request.into())
        .await
        .map_err(|e| Error::ServiceError(format!("User service unavailable: {}", e)))?;

    let info: UserInfo = serde_json::from_slice(&reply.payload)?;
    Ok(Json(info))
}

// Service B - handles requests
async fn handle_user_info_requests(state: AppState) -> Result<()> {
    let nats = state.nats().await.ok_or(Error::Internal("NATS not available"))?;
    let mut subscriber = nats.subscribe("users.info").await?;

    while let Some(msg) = subscriber.next().await {
        if let Some(reply_subject) = msg.reply {
            let request: UserInfoRequest = serde_json::from_slice(&msg.payload)?;

            if let Ok(info) = fetch_user_info(&state, request.user_id).await {
                let response = serde_json::to_vec(&info)?;
                nats.publish(reply_subject, response.into()).await?;
            }
        }
    }

    Ok(())
}
```

## Health Checks

NATS health is automatically monitored by the `/ready` endpoint:

```toml
[nats]
optional = false  # Service not ready if NATS is down
```

The readiness probe verifies NATS connectivity:

```bash
curl http://localhost:8080/ready
# Returns 200 OK if NATS is healthy
# Returns 503 Service Unavailable if NATS is down
```

## Error Handling and Retry

Handle transient failures with retry logic:

```rust
use backoff::{ExponentialBackoff, Error as BackoffError};

async fn publish_with_retry(
    nats: &async_nats::Client,
    subject: &str,
    payload: Vec<u8>,
) -> Result<()> {
    let operation = || async {
        nats.publish(subject, payload.clone().into())
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotConnected {
                    BackoffError::Transient { err: e, retry_after: None }
                } else {
                    BackoffError::Permanent(e)
                }
            })
    };

    backoff::future::retry(ExponentialBackoff::default(), operation).await?;
    Ok(())
}
```

## Best Practices

### Use Hierarchical Subjects

Organize subjects with hierarchical naming:

```rust
// ✅ Good - hierarchical subjects
"users.created"
"users.updated"
"orders.created"
"orders.shipped"
"notifications.email.sent"

// ❌ Bad - flat subjects
"user_created"
"order_made"
"email"
```

### Set Appropriate Stream Retention

Configure stream retention policies:

```rust
js.create_stream(jetstream::stream::Config {
    name: "EVENTS".to_string(),
    subjects: vec!["events.>".to_string()],
    retention: jetstream::stream::RetentionPolicy::WorkQueue, // Or Limits, Interest
    max_age: Duration::from_secs(86400 * 7), // 7 days
    max_messages: 1_000_000,
    ..Default::default()
})
.await?;
```

### Acknowledge Messages Explicitly

Always acknowledge JetStream messages:

```rust
// ✅ Good - explicit ack after processing
if process_message(&msg).await.is_ok() {
    msg.ack().await?;
} else {
    msg.nak().await?; // Negative ack for retry
}

// ❌ Bad - no acknowledgment
process_message(&msg).await?;
```

### Use Durable Consumers

Create durable consumers for reliable processing:

```rust
jetstream::consumer::Config {
    durable_name: Some("my-processor".to_string()), // ✅ Survives restarts
    ack_policy: jetstream::consumer::AckPolicy::Explicit,
    ..Default::default()
}
```

### Handle Poison Messages

Implement dead letter queue for failed messages:

```rust
const MAX_RETRIES: usize = 3;

async fn process_with_dlq(msg: jetstream::Message) -> Result<()> {
    let info = msg.info()?;

    if info.num_delivered > MAX_RETRIES {
        // Send to dead letter queue
        let nats = get_nats_client();
        nats.publish("dlq.events", msg.payload.clone()).await?;
        msg.ack().await?; // Acknowledge to remove from stream
    } else if let Err(e) = process_message(&msg).await {
        error!("Processing failed (attempt {}): {}", info.num_delivered, e);
        msg.nak().await?; // Negative ack for retry
    } else {
        msg.ack().await?;
    }

    Ok(())
}
```

## Production Deployment

### Environment Configuration

```bash
# Production environment
export ACTON_NATS_URL=nats://nats.prod.example.com:4222
```

### NATS Cluster Support

For high-availability deployments:

```toml
[nats]
url = "nats://nats1.prod.example.com:4222,nats://nats2.prod.example.com:4222,nats://nats3.prod.example.com:4222"
```

### Kubernetes Deployment

```yaml
env:
  - name: ACTON_NATS_URL
    value: "nats://nats-cluster.nats.svc.cluster.local:4222"
```

### TLS/Authentication

Secure NATS connections:

```toml
[nats]
url = "nats://user:password@nats.prod.example.com:4222"
# Or use TLS certificates
tls_cert = "/path/to/client-cert.pem"
tls_key = "/path/to/client-key.pem"
tls_ca = "/path/to/ca-cert.pem"
```

## Related Features

- **[Health Checks](/docs/health-checks)** - Automatic NATS health monitoring
- **[Observability](/docs/observability)** - Distributed tracing across event flows
- **[Database](/docs/database)** - Combine database writes with event publishing
