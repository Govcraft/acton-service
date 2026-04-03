---
title: Actor Extensions
nextjs:
  metadata:
    title: Actor Extensions
    description: Add custom runtime state to your application using supervised actor extensions.
---

Actor extensions let you add custom runtime state to your application backed by supervised actors. Instead of raw `Arc<Mutex<T>>` or `axum::Extension`, your state lives inside actors that benefit from automatic restart on failure, broker event subscriptions, and built-in observability.

{% callout type="note" title="One Mechanism, One Mental Model" %}
All custom runtime state uses `with_actor()` on `ServiceBuilder`. Read-only state uses sync handlers (zero async overhead). Mutable state uses async handlers. Both get supervision, broker access, and tracing for free.
{% /callout %}

---

## Quick Start

**1. Define your actor state and implement `ActorExtension`:**

```rust
use acton_service::prelude::*;
use acton_reactive::prelude::*;
use std::collections::HashMap;

// Messages
#[derive(Clone, Debug)]
struct CacheSet { key: String, value: String }

#[derive(Clone, Debug)]
struct CacheGet { key: String }

#[derive(Clone, Debug)]
struct CacheGetResponse(Option<String>);

// Actor state
#[acton_actor]
pub struct MyCache {
    items: HashMap<String, String>,
}

impl ActorExtension for MyCache {
    fn configure(actor: &mut ManagedActor<Idle, Self>) {
        actor.mutate_on::<CacheSet>(|actor, envelope| {
            let msg = envelope.message();
            actor.model.items.insert(msg.key.clone(), msg.value.clone());
            Reply::ready()
        });

        actor.act_on::<CacheGet>(|actor, envelope| {
            let val = actor.model.items.get(&envelope.message().key).cloned();
            let reply = envelope.reply_envelope();
            Reply::pending(async move {
                reply.send(CacheGetResponse(val)).await;
            })
        });
    }
}
```

**2. Register with `ServiceBuilder`:**

```rust
let service = ServiceBuilder::new()
    .with_actor::<MyCache>()
    .with_routes(routes)
    .build();

service.serve().await?;
```

**3. Access in handlers via `state.actor()`:**

```rust
async fn set_cache(
    State(state): State<AppState>,
    Json(body): Json<SetCacheRequest>,
) -> impl IntoResponse {
    let cache = state.actor::<MyCache>().unwrap();
    cache.send(CacheSet {
        key: body.key,
        value: body.value,
    }).await;

    StatusCode::NO_CONTENT
}
```

---

## The ActorExtension Trait

Every actor extension implements the `ActorExtension` trait:

```rust
pub trait ActorExtension: Default + Debug + Send + 'static {
    /// Configure message handlers, lifecycle hooks, and broker subscriptions.
    fn configure(actor: &mut ManagedActor<Idle, Self>);

    /// Restart policy under supervision. Defaults to Permanent.
    fn restart_policy() -> RestartPolicy {
        RestartPolicy::Permanent
    }
}
```

The `configure` method receives the actor builder, giving you access to:

| Method | Purpose |
|--------|---------|
| `mutate_on::<M>()` | Async handler that can modify actor state |
| `mutate_on_sync::<M>()` | Sync handler that can modify actor state (zero async overhead) |
| `act_on::<M>()` | Async read-only handler (queries) |
| `act_on_sync::<M>()` | Sync read-only handler (zero async overhead) |
| `after_start()` | Lifecycle hook: runs after actor starts |
| `before_stop()` | Lifecycle hook: runs before actor stops |

---

## Handler Types

### Fire-and-Forget (mutate_on)

Use `mutate_on` when you need to update actor state and don't need a response:

```rust
actor.mutate_on::<Increment>(|actor, envelope| {
    actor.model.count += envelope.message().amount;
    Reply::ready()
});
```

### Request-Response (act_on)

Use `act_on` when callers need a response:

```rust
actor.act_on::<GetCount>(|actor, envelope| {
    let count = actor.model.count;
    let reply = envelope.reply_envelope();
    Reply::pending(async move {
        reply.send(CountResponse { count }).await;
    })
});
```

### Sync Handlers (zero overhead)

For simple operations that don't require async work, use the sync variants. These execute as direct function calls with no async machinery:

```rust
// Read-only sync: ideal for lookups, config access
actor.act_on_sync::<LookupKey>(|actor, envelope| {
    let key = &envelope.message().key;
    let value = actor.model.table.get(key).cloned();
    // No reply mechanism in sync handlers — use for side-effect-free reads
});

// Mutable sync: ideal for simple state updates
actor.mutate_on_sync::<SetFlag>(|actor, envelope| {
    actor.model.enabled = envelope.message().0;
});
```

---

## Supervision

All actor extensions run under a framework-managed supervisor with automatic restart on failure. Override `restart_policy()` to control the behavior:

```rust
impl ActorExtension for MyCache {
    fn configure(actor: &mut ManagedActor<Idle, Self>) {
        // ... handlers ...
    }

    fn restart_policy() -> RestartPolicy {
        RestartPolicy::Transient  // Only restart on abnormal termination
    }
}
```

| Policy | Behavior |
|--------|----------|
| `Permanent` (default) | Always restart, except during service shutdown |
| `Transient` | Restart only on panic or unexpected termination |
| `Temporary` | Never restart |

The supervision hierarchy:

```
broker (framework)
  ├── database-pool-agent
  ├── redis-pool-agent
  ├── audit-agent
  └── extensions-supervisor
        ├── my-cache          (Permanent)
        ├── rate-limiter      (Transient)
        └── http-client       (Permanent)
```

---

## Broker Subscriptions

Actor extensions can subscribe to broker events broadcast by handlers or other agents. Subscribe in the `after_start` lifecycle hook:

```rust
#[derive(Clone, Debug)]
struct OrderCompleted { order_id: i64, total: f64 }

impl ActorExtension for AnalyticsActor {
    fn configure(actor: &mut ManagedActor<Idle, Self>) {
        // Handle the broadcast event
        actor.mutate_on::<OrderCompleted>(|actor, envelope| {
            let order = envelope.message();
            actor.model.total_revenue += order.total;
            actor.model.order_count += 1;
            Reply::ready()
        });

        // Subscribe to broker on startup
        actor.after_start(|actor| {
            let handle = actor.handle().clone();
            Reply::pending(async move {
                handle.subscribe::<OrderCompleted>().await;
            })
        });
    }
}
```

Then broadcast from any handler:

```rust
async fn complete_order(
    State(state): State<AppState>,
    Json(order): Json<CompleteOrderRequest>,
) -> Result<Json<Order>, ApiError> {
    let order = process_order(&state, order).await?;

    // Broadcast — all subscribed actor extensions receive this
    if let Some(broker) = state.broker() {
        broker.broadcast(OrderCompleted {
            order_id: order.id,
            total: order.total,
        }).await;
    }

    Ok(Json(order))
}
```

---

## Lifecycle Hooks

Use lifecycle hooks for initialization and cleanup:

```rust
impl ActorExtension for MyActor {
    fn configure(actor: &mut ManagedActor<Idle, Self>) {
        actor.after_start(|actor| {
            let handle = actor.handle().clone();
            Reply::pending(async move {
                // Subscribe to broker events
                handle.subscribe::<SomeEvent>().await;
                // Start periodic work
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(Duration::from_secs(60));
                    loop {
                        interval.tick().await;
                        handle.send(PeriodicCleanup).await;
                    }
                });
            })
        });

        actor.before_stop(|actor| {
            // Flush buffers, close connections, etc.
            let buffer = actor.model.buffer.clone();
            Reply::pending(async move {
                flush_to_storage(&buffer).await;
            })
        });
    }
}
```

---

## Read-Only State (Services, Clients)

For immutable state like HTTP clients or lookup tables, use `act_on_sync` handlers. The actor wraps your data with zero async overhead while still benefiting from supervision:

```rust
#[acton_actor]
pub struct HttpClientActor {
    client: reqwest::Client,
}

impl ActorExtension for HttpClientActor {
    fn configure(actor: &mut ManagedActor<Idle, Self>) {
        // Sync read-only handler — direct function call, no async
        actor.act_on_sync::<GetClient>(|actor, _envelope| {
            // The client is available via the actor's state
            // For read-only data, sync handlers have zero overhead
        });
    }
}
```

---

## Multiple Extensions

Register as many actor extensions as you need:

```rust
let service = ServiceBuilder::new()
    .with_actor::<MyCache>()
    .with_actor::<RateLimiter>()
    .with_actor::<NotificationService>()
    .with_actor::<AnalyticsCollector>()
    .with_routes(routes)
    .build();
```

Each actor is independent and runs under the shared extensions supervisor. Access each by type:

```rust
async fn handler(State(state): State<AppState>) -> impl IntoResponse {
    let cache = state.actor::<MyCache>().unwrap();
    let limiter = state.actor::<RateLimiter>().unwrap();
    // ...
}
```

---

## Best Practices

**Use actors instead of `Arc<Mutex<T>>`**: The framework's core value proposition is concurrency without mutexes. Actor extensions enforce this pattern for your application-level state.

**Prefer sync handlers for simple operations**: `mutate_on_sync` and `act_on_sync` avoid async machinery when you don't need it. Use them for simple state reads and writes.

**Subscribe to broker events for decoupled communication**: Instead of calling services directly from handlers, broadcast events and let actor extensions react independently.

**Choose restart policies deliberately**:
- `Permanent` for stateless or recoverable actors (caches, rate limiters)
- `Transient` for actors where restarts should only happen on unexpected failures
- `Temporary` for one-shot actors that should not restart

**Keep actors focused**: Each actor should own a single concern. Prefer multiple small actors over one large actor with many message types.

---

## Next Steps

- **[Reactive Architecture](/docs/reactive-architecture)** - How the framework's internal agents work
- **[Background Worker](/docs/background-worker)** - Managed background task execution
- **[Health Checks](/docs/health-checks)** - Health and readiness endpoints
