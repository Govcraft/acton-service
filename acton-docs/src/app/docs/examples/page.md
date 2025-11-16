---
title: Examples
nextjs:
  metadata:
    title: Examples
    description: Complete code examples demonstrating acton-service features and patterns
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


Browse complete, runnable examples showing how to build different types of services with acton-service.

## Minimal HTTP Service

The simplest possible REST API with type-enforced versioning.

```rust
use acton_service::prelude::*;

async fn hello() -> &'static str {
    "Hello, world!"
}

#[tokio::main]
async fn main() -> Result<()> {
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/hello", get(hello))
        })
        .build_routes();

    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await
}
```

This creates a service with:
- Health endpoints at `/health` and `/ready`
- API endpoint at `/api/v1/hello`
- Automatic observability and tracing
- Production-ready defaults

---

## Production Service with Database

A complete CRUD API with PostgreSQL connection pooling and error handling.

```rust
use acton_service::prelude::*;

#[derive(Serialize)]
struct User {
    id: i64,
    name: String,
}

async fn list_users(State(state): State<AppState>) -> Result<Json<Vec<User>>> {
    let db = state.database()?;
    let users = sqlx::query_as!(User, "SELECT id, name FROM users")
        .fetch_all(db)
        .await?;
    Ok(Json(users))
}

#[tokio::main]
async fn main() -> Result<()> {
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/users", get(list_users))
        })
        .build_routes();

    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await
}
```

Configuration in `~/.config/acton-service/my-service/config.toml`:

```toml
[service]
name = "my-service"
port = 8080

[database]
url = "postgres://localhost/mydb"
max_connections = 50
```

Features:
- Automatic database connection pooling
- Health checks verify database connectivity
- Retry logic on connection failures
- Structured error handling

---

## Event-Driven Service

Background worker processing NATS JetStream events.

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

    let nats = state.nats()?;
    let mut subscriber = nats.subscribe("events.>").await?;

    while let Some(msg) = subscriber.next().await {
        if let Err(e) = process_event(msg).await {
            error!("Event processing failed: {}", e);
        }
    }

    Ok(())
}
```

Use cases:
- Asynchronous message processing
- Event-driven microservices
- Background job processing
- Distributed system integration

---

## Example Repository

The [`examples/`](https://github.com/Govcraft/acton-service/tree/main/acton-service/examples) directory contains complete working examples:

### Simple Versioned API
**File**: `simple-api.rs`

Demonstrates basic API versioning with multiple versions and deprecation notices.

```bash
cargo run --example simple-api
```

### User Management API
**File**: `users-api.rs`

Full CRUD API showing version deprecation, database integration, and migration paths.

```bash
cargo run --example users-api
```

### Dual-Protocol Service
**File**: `ping-pong.rs`

HTTP and gRPC on the same port with automatic protocol detection.

```bash
cargo run --example ping-pong --features grpc
```

### Event-Driven Architecture
**File**: `event-driven.rs`

NATS JetStream integration for pub/sub messaging patterns.

```bash
cargo run --example event-driven --features grpc
```

### Cedar Authorization
**File**: `cedar-authz.rs`

Policy-based authorization with fine-grained access control.

```bash
cargo run --example cedar-authz --features cedar-authz,cache
```

See the [Cedar Example Guide](https://github.com/Govcraft/acton-service/blob/main/acton-service/examples/CEDAR_EXAMPLE_README.md) for detailed documentation on implementing policy-based authorization.

---

## Running Examples

All examples can be run from the repository root:

```bash
# Basic examples
cargo run --example simple-api
cargo run --example users-api

# Examples requiring feature flags
cargo run --example ping-pong --features grpc
cargo run --example event-driven --features grpc
cargo run --example cedar-authz --features cedar-authz,cache
```

Each example includes:
- Complete source code
- Configuration file templates
- Test requests you can copy/paste
- Inline documentation explaining key concepts

---

## Next Steps

- Review [Feature Flags](/docs/feature-flags) to understand which features to enable
- Check [Troubleshooting](/docs/troubleshooting) if you encounter issues
- Explore [API Reference](/docs/api-reference) for detailed documentation
