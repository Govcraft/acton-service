---
title: Reactive Architecture
nextjs:
  metadata:
    title: Reactive Architecture
    description: Understanding acton-service's agent-based architecture for connection pool management and background task execution.
---

acton-service uses a reactive, actor-based architecture powered by [`acton-reactive`](https://github.com/acton-rs/acton-reactive) to manage connection pools and background tasks. This guide explains how the architecture works and how it benefits your applications.

{% callout type="note" title="Transparent by Design" %}
The reactive architecture is an **internal implementation detail**. You interact with familiar APIs like `state.db()` and `state.redis()` - agents handle the complexity behind the scenes.
{% /callout %}

---

## Why Reactive Architecture?

Traditional connection pool management has challenges:

```rust
// Traditional approach - shared mutable state
pub struct AppState {
    db: Arc<RwLock<Option<PgPool>>>,  // Lock contention
    redis: Arc<RwLock<Option<RedisPool>>>,  // More lock contention
}

// Every request must acquire locks
async fn handler(State(state): State<AppState>) {
    let pool = state.db.read().await;  // Potential bottleneck
    // ...
}
```

**Problems:**
- Lock contention under high load
- Complex reconnection logic scattered across handlers
- No centralized health monitoring
- Difficult graceful shutdown coordination

**The reactive approach solves these:**
- **Message-passing** instead of shared mutable state
- **Centralized** connection lifecycle management
- **Automatic** health monitoring and reconnection
- **Coordinated** graceful shutdown

---

## How It Works

When you call `ServiceBuilder::build()`, the framework automatically:

1. **Spawns pool agents** to manage each configured connection type (database, Redis, NATS)
2. **Agents establish connections** and update shared state
3. **Handlers access pools** via `AppState` methods (no agent interaction needed)
4. **On shutdown**, agents gracefully close all connections

```
                    ┌─────────────────┐
                    │ ServiceBuilder  │
                    │    .build()     │
                    └────────┬────────┘
                             │
         ┌───────────────────┼───────────────────┐
         ▼                   ▼                   ▼
┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
│ DatabasePool    │ │ RedisPool       │ │ NatsPool        │
│ Agent           │ │ Agent           │ │ Agent           │
└────────┬────────┘ └────────┬────────┘ └────────┬────────┘
         │                   │                   │
         │ Updates           │ Updates           │ Updates
         ▼                   ▼                   ▼
┌─────────────────────────────────────────────────────────┐
│                      AppState                            │
│  state.db() ────► PgPool                                │
│  state.redis() ──► RedisPool                            │
│  state.nats() ───► NatsClient                           │
└─────────────────────────────────────────────────────────┘
```

---

## Pool Agents

Pool agents manage connection pools using the actor pattern. Each agent type handles a specific connection:

### Database Pool Agent

Manages PostgreSQL connections via SQLx:

```rust
// You write:
ServiceBuilder::new()
    .with_config(config)  // Contains database config
    .with_routes(routes)
    .build()
    .await?;

// Behind the scenes:
// 1. DatabasePoolAgent::spawn() creates the agent
// 2. Agent connects to PostgreSQL
// 3. Pool stored in shared state
// 4. state.db() returns the pool
```

**Features:**
- Automatic connection on startup
- Connection validation
- Graceful pool closure on shutdown
- Health status reporting

### Redis Pool Agent

Manages Redis connections via deadpool-redis:

```rust
// Same pattern - configure and go
[redis]
url = "redis://localhost:6379"
max_connections = 50
```

**Features:**
- Connection pooling with deadpool
- Automatic reconnection attempts
- Connection health checks

### NATS Pool Agent

Manages NATS connections for event streaming:

```rust
[nats]
url = "nats://localhost:4222"
```

**Features:**
- Async NATS client management
- Automatic reconnection
- Clean disconnection on shutdown

---

## Shared State Architecture

Pool agents use a "shared state with agent coordination" pattern:

```rust
// Internal architecture (you don't write this)
pub type SharedDbPool = Arc<RwLock<Option<PgPool>>>;

// Agent updates shared state when connected
agent.mutate_on::<DatabasePoolConnected>(|agent, envelope| {
    let pool = envelope.message().pool.clone();

    // Update shared storage for direct AppState access
    if let Some(shared) = &agent.model.shared_pool {
        shared.write().await = Some(pool);
    }
});
```

**Benefits:**
- **Minimal locking**: Pools are set once on connection, then read-only
- **Direct access**: `state.db()` reads the pool without message passing
- **Agent coordination**: Lifecycle managed by agents, not handlers

---

## Using Pools in Handlers

You don't interact with agents directly - use `AppState` methods:

```rust
use acton_service::prelude::*;
use axum::extract::State;

async fn get_users(
    State(state): State<AppState>,
) -> Result<Json<Vec<User>>, ApiError> {
    // Get pool directly - no agent interaction
    let pool = state.db()?;

    let users = sqlx::query_as::<_, User>("SELECT * FROM users")
        .fetch_all(pool)
        .await?;

    Ok(Json(users))
}
```

The `state.db()` method:
1. Reads from shared state (fast, minimal locking)
2. Returns `Result<&PgPool, DatabaseUnavailable>`
3. Fails fast if pool not yet connected

---

## Health Monitoring

Pool agents report health status to the framework's health system:

```rust
// /ready endpoint checks all pool agents
GET /ready

// Response when all pools healthy:
{
    "status": "ready",
    "checks": {
        "database": { "status": "up", "latency_ms": 5 },
        "redis": { "status": "up", "latency_ms": 2 },
        "nats": { "status": "up", "latency_ms": 3 }
    }
}

// Response when database is down:
{
    "status": "not_ready",
    "checks": {
        "database": { "status": "down", "error": "Connection refused" },
        "redis": { "status": "up", "latency_ms": 2 },
        "nats": { "status": "up", "latency_ms": 3 }
    }
}
```

See [Health Checks](/docs/health-checks) for more details.

---

## Graceful Shutdown

When the service receives a shutdown signal (SIGTERM, SIGINT), pool agents:

1. **Stop accepting new requests** (handled by server)
2. **Close active connections** gracefully
3. **Report shutdown status** via logging

```rust
// Agent shutdown handler (internal)
agent.before_stop(|agent| {
    if let Some(pool) = &agent.model.pool {
        pool.close().await;  // Wait for queries to complete
    }
});
```

**Kubernetes integration:**
- `/health` continues returning 200 during shutdown
- `/ready` returns 503, removing pod from load balancer
- Active requests complete before termination

---

## Configuration

Pool agents are configured via `config.toml` or environment variables:

```toml
# Database pool configuration
[database]
url = "postgres://user:pass@localhost/mydb"
max_connections = 50
min_connections = 5
connection_timeout_secs = 30
idle_timeout_secs = 600
max_lifetime_secs = 1800

# Redis pool configuration
[redis]
url = "redis://localhost:6379"
max_connections = 50

# NATS configuration
[nats]
url = "nats://localhost:4222"
```

Or via environment variables:

```bash
ACTON_DATABASE__URL=postgres://user:pass@localhost/mydb
ACTON_DATABASE__MAX_CONNECTIONS=50
ACTON_REDIS__URL=redis://localhost:6379
ACTON_NATS__URL=nats://localhost:4222
```

---

## Troubleshooting

### Pool Not Available

If `state.db()` returns an error, check:

1. **Configuration**: Is the database URL correct?
2. **Network**: Can the service reach the database?
3. **Logs**: Look for connection errors at startup
4. **Health endpoint**: Check `/ready` for pool status

```bash
# Check health status
curl http://localhost:8080/ready | jq

# Check logs for connection errors
grep -i "pool" /var/log/myservice/app.log
```

### Slow Startup

If the service takes a long time to start:

1. **Check `lazy_init`**: Set to `true` for non-blocking startup
2. **Connection timeout**: Reduce if network is slow
3. **Pool size**: Large `min_connections` means more initial connections

```toml
[database]
lazy_init = true  # Don't block startup waiting for connections
connection_timeout_secs = 10  # Fail faster on unreachable DB
min_connections = 1  # Fewer initial connections
```

### Connection Exhaustion

If you see "connection pool exhausted" errors:

1. **Increase pool size**: Raise `max_connections`
2. **Check for leaks**: Ensure connections are returned to pool
3. **Add timeouts**: Set `acquire_timeout_secs`

```toml
[database]
max_connections = 100  # Increase from default
acquire_timeout_secs = 30  # Wait for available connection
```

---

## Event Broker

The reactive architecture includes an **Event Broker** that enables HTTP handlers to broadcast typed events to subscribed agents within your service. This is different from external event systems (like NATS) - the broker is for internal, in-process communication.

### Accessing the Broker

The broker is available via `AppState` when the reactive runtime is initialized:

```rust
use acton_service::prelude::*;
use acton_reactive::prelude::*;

async fn create_user_handler(
    State(state): State<AppState>,
    Json(user): Json<CreateUserRequest>,
) -> Result<Json<User>, ApiError> {
    // Create the user
    let user = create_user(&state, user).await?;

    // Broadcast event to subscribed agents
    if let Some(broker) = state.broker() {
        broker.broadcast(UserCreatedEvent {
            user_id: user.id,
            email: user.email.clone(),
        }).await;
    }

    Ok(Json(user))
}
```

### Defining Events

Events are simple structs that implement `Clone`:

```rust
#[derive(Clone, Debug)]
pub struct UserCreatedEvent {
    pub user_id: i64,
    pub email: String,
}

#[derive(Clone, Debug)]
pub struct OrderCompletedEvent {
    pub order_id: i64,
    pub user_id: i64,
    pub total: f64,
}
```

### Subscribing Agents

Create agents that react to broadcasted events:

```rust
use acton_reactive::prelude::*;

pub struct NotificationAgent {
    // Agent state
}

impl NotificationAgent {
    pub async fn spawn(runtime: &mut AgentRuntime) -> Result<AgentHandle> {
        let agent = runtime
            .new_agent::<Self>()
            .with_handler(|agent: &mut Self, event: UserCreatedEvent| async move {
                // React to user creation
                send_welcome_email(&event.email).await?;
                Ok(())
            })
            .spawn()
            .await?;

        Ok(agent)
    }
}
```

### Use Cases

The event broker is ideal for:

- **Background processing** - Trigger background work from HTTP handlers
- **Decoupled notifications** - Send emails/webhooks without blocking requests
- **Audit logging** - Record actions in a separate agent
- **Cache invalidation** - Notify cache agents of data changes
- **Real-time updates** - Push changes to WebSocket handlers

### Broker vs. NATS

| Feature | Event Broker | NATS |
|---------|--------------|------|
| Scope | In-process | Distributed |
| Use case | Internal agent communication | Cross-service messaging |
| Persistence | None | JetStream streams |
| Performance | Zero network overhead | Network latency |

Use the **Event Broker** for internal coordination and **NATS** for cross-service communication.

---

## Next Steps

- **[Background Worker](/docs/background-worker)** - Learn about managed background task execution
- **[Health Checks](/docs/health-checks)** - Configure health and readiness endpoints
- **[Database Integration](/docs/database)** - PostgreSQL-specific features
- **[Cache Integration](/docs/cache)** - Redis-specific features
- **[Events Integration](/docs/events)** - NATS-specific features
