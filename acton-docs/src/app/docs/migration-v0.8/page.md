---
title: Migration Guide v0.7 to v0.8
nextjs:
  metadata:
    title: Migration Guide v0.7 to v0.8
    description: Guide for migrating acton-service applications from v0.7 to v0.8, covering new features and breaking changes.
---

This guide covers migrating from acton-service v0.7.x to v0.8.x. Version 0.8 introduces reactive agent-based pool management, type-safe request IDs, and the BackgroundWorker agent.

{% callout type="note" title="Non-Breaking Changes" %}
Most changes in v0.8 are **additive** and do not require code changes. Your existing v0.7 code should compile and work with v0.8. The reactive agent system works transparently behind the scenes.
{% /callout %}

---

## What's New in v0.8

### Reactive Agent Architecture

Connection pools (database, Redis, NATS) are now managed by internal agents:

- **Automatic health monitoring** - Agents report pool health to `/ready` endpoint
- **Graceful shutdown** - Coordinated connection cleanup on shutdown
- **Transparent operation** - Pools are still accessed via `state.db()`, `state.redis()`, etc.

You don't need to change any code - agents work behind the scenes. See [Reactive Architecture](/docs/reactive-architecture) for details.

### TypeID Request IDs

Request IDs now use the TypeID specification with UUIDv7:

```
# Old format (UUID)
550e8400-e29b-41d4-a716-446655440000

# New format (TypeID with req_ prefix)
req_01h455vb4pex5vsknk084sn02q
```

**Benefits:**
- Time-sortable (UUIDv7)
- Type-safe prefix (`req_`)
- Better log correlation

See [Request IDs](/docs/request-ids) for the full API.

### BackgroundWorker Agent

New managed background task execution:

```rust
use acton_service::agents::{BackgroundWorker, TaskStatus};

let worker = BackgroundWorker::spawn(&mut runtime).await?;

worker.submit("my-task", || async {
    do_work().await?;
    Ok(())
}).await;

let status = worker.get_task_status("my-task").await;
```

See [Background Worker](/docs/background-worker) for the complete guide.

### Event Broker Support

`AppState` now includes an optional broker handle for event-driven architectures:

```rust
// Access the broker (if configured)
if let Some(broker) = state.broker() {
    broker.send(MyEvent { ... }).await;
}
```

---

## Migration Steps

### Step 1: Update Dependencies

Update `Cargo.toml`:

```toml
[dependencies]
acton-service = "0.8"  # Was: "0.7"
```

### Step 2: Verify Compilation

Run `cargo check` to verify your code compiles. Most v0.7 code works unchanged:

```bash
cargo check
```

If you get errors, see the "Breaking Changes" section below.

### Step 3: Update Pool Access (Optional)

The pool access methods are now async and return `Option`:

```rust
// v0.7 pattern (still works)
async fn handler(State(state): State<AppState>) -> Result<Json<Data>> {
    let db = state.db().await.ok_or(Error::Internal("DB unavailable"))?;
    // ...
}

// v0.8 preferred pattern (identical, but clearer intent)
async fn handler(State(state): State<AppState>) -> Result<Json<Data>> {
    let db = state.db()
        .ok_or_else(|| Error::Internal("Database unavailable".into()))?;
    // ...
}
```

### Step 4: Use TypeID Request IDs (Optional)

If you were manually creating request IDs, update to use `RequestId`:

```rust
// v0.7 - manual UUID
use uuid::Uuid;
let request_id = Uuid::new_v4().to_string();

// v0.8 - TypeID with req_ prefix
use acton_service::ids::RequestId;
let request_id = RequestId::new();  // e.g., req_01h455vb4pex5vsknk084sn02q
```

### Step 5: Test Your Application

Run your tests to verify everything works:

```bash
cargo test
```

---

## Breaking Changes

### None in v0.8

Version 0.8 has **no breaking changes** for typical usage. The following changes are technically breaking but unlikely to affect most users:

#### Internal Module Reorganization

If you were importing internal types directly, paths may have changed:

```rust
// If you were using internal paths (unlikely)
// use acton_service::internal_module::SomeType;

// Use public re-exports instead
use acton_service::prelude::*;
```

#### Request ID Header Value Format

The `x-request-id` header now uses TypeID format. If your clients parse request IDs, they should handle both formats:

```javascript
// Client-side handling
const requestId = response.headers.get('x-request-id');
// Old: "550e8400-e29b-41d4-a716-446655440000"
// New: "req_01h455vb4pex5vsknk084sn02q"
```

---

## New Features to Adopt

### TypeID Request IDs

Access the typed request ID in handlers:

```rust
use acton_service::ids::RequestId;
use axum::Extension;

async fn handler(
    Extension(request_id): Extension<RequestId>,
) -> impl IntoResponse {
    // Time-sortable, typed request ID
    tracing::info!(request_id = %request_id, "Processing");
    // ...
}
```

### BackgroundWorker for Tasks

Replace ad-hoc `tokio::spawn` with managed workers:

```rust
use acton_service::agents::{BackgroundWorker, TaskStatus};

// Setup
let worker = BackgroundWorker::spawn(&mut runtime).await?;

// Submit tasks
worker.submit("cleanup", || async {
    cleanup_old_data().await
}).await;

// Monitor
let status = worker.get_task_status("cleanup").await;

// Cancel if needed
worker.cancel("cleanup").await;
```

### Health Monitoring

Pool health is now automatically monitored. Check aggregated status:

```bash
curl http://localhost:8080/ready
# Returns pool health for all configured connections
```

---

## Dependency Updates

Version 0.8 updates these dependencies:

| Dependency | v0.7 | v0.8 |
|------------|------|------|
| acton-reactive | - | 0.1+ |
| mti (TypeID) | - | 0.1+ |
| dashmap | - | 5.5+ |
| tokio-util | 0.7 | 0.7+ |

The `acton-reactive` dependency is new and provides the agent framework.

---

## Configuration Changes

No configuration changes are required. The following new options are available:

### ServiceBuilder Changes

`ServiceBuilder::build()` now automatically spawns pool agents when connections are configured:

```rust
// This is unchanged - agents are spawned automatically
ServiceBuilder::new()
    .with_config(config)  // If database/redis/nats configured...
    .with_routes(routes)
    .build()  // ...agents are spawned here
    .serve()
    .await
```

---

## Rollback Plan

If you encounter issues with v0.8, you can rollback to v0.7:

```toml
[dependencies]
acton-service = "0.7"  # Rollback version
```

---

## Getting Help

- **[Troubleshooting](/docs/troubleshooting)** - Common issues and solutions
- **[GitHub Issues](https://github.com/Govcraft/acton-service/issues)** - Report bugs or ask questions
- **[FAQ](/docs/faq)** - Frequently asked questions
