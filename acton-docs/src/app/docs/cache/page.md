---
title: Cache (Redis)
nextjs:
  metadata:
    title: Cache (Redis)
    description: Redis integration with Deadpool connection pooling, health checks, distributed caching, and rate limiting for production services
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


Integrate Redis for distributed caching, session storage, and rate limiting with automatic connection pooling and health monitoring.

---

## Overview

acton-service provides production-ready Redis integration through `deadpool-redis` connection pooling, automatic health checks, and support for distributed rate limiting. Redis connections are managed automatically through the `AppState` with zero configuration required for development.

{% callout type="note" title="Agent-Managed Pools" %}
Redis connection pools are managed internally by a **RedisPoolAgent** that handles connection lifecycle, health monitoring, and graceful shutdown. You interact with pools via `state.redis().await` - the agent works transparently behind the scenes. See [Reactive Architecture](/docs/reactive-architecture) for implementation details.
{% /callout %}

## Installation

Enable the cache feature:

```toml
[dependencies]
{% $dep.cache %}
```

## Configuration

Redis configuration follows XDG standards with environment variable overrides:

```toml
# ~/.config/acton-service/my-service/config.toml
[redis]
url = "redis://localhost:6379"
max_connections = 50
optional = true  # Service remains ready even if Redis is down
```

### Full Configuration Options

```toml
[redis]
# Redis connection URL (required)
url = "redis://localhost:6379"

# Maximum number of connections in the pool (default: 50)
max_connections = 50

# Connection timeout in seconds (default: 30)
connection_timeout_secs = 30

# Maximum retry attempts when establishing the connection (default: 5)
max_retries = 5

# Delay between retry attempts in seconds (default: 2)
retry_delay_secs = 2

# Whether Redis is optional (default: false)
# If true, the service starts and stays ready even when Redis is unavailable
optional = false

# Lazy initialization (default: true)
# If true, the connection is established in the background after startup
lazy_init = true
```

Connection attempts use exponential backoff, doubling `retry_delay_secs` on each retry up to `max_retries`.

### Environment Variable Override

```bash
ACTON_REDIS_URL=redis://localhost:6379 cargo run
```

## Basic Usage

Access Redis through `AppState` in your handlers. `state.redis()` is **async** and returns
`Option<deadpool_redis::Pool>` - it is `None` when Redis is not configured or not yet connected.

```rust
use acton_service::prelude::*;
use deadpool_redis::redis::AsyncCommands;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
struct User {
    id: i64,
    name: String,
    email: String,
}

async fn get_user_cached(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<User>> {
    let cache_key = format!("user:{}", id);

    // Try the cache first (Redis is optional - skip the lookup if it is unavailable)
    if let Some(pool) = state.redis().await {
        let mut conn = pool
            .get()
            .await
            .map_err(|e| Error::Internal(format!("Redis connection failed: {}", e)))?;

        let cached: Option<String> = conn
            .get(&cache_key)
            .await
            .map_err(|e| Error::Internal(format!("Redis GET failed: {}", e)))?;

        if let Some(json) = cached {
            if let Ok(user) = serde_json::from_str::<User>(&json) {
                return Ok(Json(user));
            }
        }
    }

    // Cache miss - fetch from the database
    let db = state
        .db()
        .await
        .ok_or_else(|| Error::Internal("Database not available".into()))?;

    let user = sqlx::query_as::<_, User>("SELECT id, name, email FROM users WHERE id = $1")
        .bind(id)
        .fetch_one(&db)
        .await?;

    // Store in the cache with a 5 minute TTL
    if let Some(pool) = state.redis().await {
        if let Ok(mut conn) = pool.get().await {
            if let Ok(serialized) = serde_json::to_string(&user) {
                let _ = conn.set_ex::<_, _, ()>(&cache_key, serialized, 300).await;
            }
        }
    }

    Ok(Json(user))
}
```

{% callout type="note" title="Where the Redis types come from" %}
The pool type is `deadpool_redis::Pool`. The command traits live at `deadpool_redis::redis::AsyncCommands`,
so you do not need a separate `redis` entry in your `Cargo.toml` - the `cache` feature brings both in.
{% /callout %}

## Common Use Cases

### Session Storage

Store session data with automatic expiration:

```rust
use acton_service::prelude::*;
use deadpool_redis::redis::AsyncCommands;

async fn create_session(
    State(state): State<AppState>,
    Json(login): Json<LoginRequest>,
) -> Result<Json<SessionResponse>> {
    let pool = state
        .redis()
        .await
        .ok_or_else(|| Error::Internal("Redis not available".into()))?;

    let mut conn = pool
        .get()
        .await
        .map_err(|e| Error::Internal(format!("Redis connection failed: {}", e)))?;

    // Generate a session token
    let session_id = uuid::Uuid::new_v4().to_string();
    let session_key = format!("session:{}", session_id);

    // Store the session payload with a 1 hour TTL
    let session_data = serde_json::to_string(&SessionData {
        user_id: login.user_id,
        created_at: chrono::Utc::now(),
    })
    .map_err(|e| Error::Internal(format!("Serialization failed: {}", e)))?;

    conn.set_ex::<_, _, ()>(&session_key, session_data, 3600)
        .await
        .map_err(|e| Error::Internal(format!("Redis SETEX failed: {}", e)))?;

    Ok(Json(SessionResponse { session_id }))
}
```

{% callout type="note" title="Managed sessions" %}
For cookie-backed sessions with a managed store, enable the `session` feature instead of hand-rolling the
key layout. See [Session Management](/docs/session).
{% /callout %}

### Distributed Rate Limiting

Rate limiting is **configuration-driven**. With the `governor` feature enabled, the middleware is attached
automatically during `ServiceBuilder::build()` - there is no layer to wire up by hand:

```toml
[rate_limit]
# Global defaults
per_user_rpm = 100
per_client_rpm = 1000
window_secs = 60

# Attach the rate-limit middleware automatically during build() (default: true)
auto_apply = true

# Per-route overrides
[rate_limit.routes."POST /api/v1/uploads"]
requests_per_minute = 5
per_user = true
```

```rust
use acton_service::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/users", get(list_users))
        })
        .build_routes();

    // Rate limiting comes from the [rate_limit] config section - no explicit layer needed
    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await
}
```

See [Rate Limiting](/docs/rate-limiting) for the full set of options.

### Cache-Aside Pattern

Implement cache-aside (lazy loading) for expensive computations:

```rust
use acton_service::prelude::*;
use deadpool_redis::redis::AsyncCommands;

async fn get_analytics(
    State(state): State<AppState>,
    Path(report_id): Path<String>,
) -> Result<Json<Analytics>> {
    let cache_key = format!("analytics:{}", report_id);

    // Check the cache
    if let Some(pool) = state.redis().await {
        if let Ok(mut conn) = pool.get().await {
            let cached: Option<String> = conn
                .get(&cache_key)
                .await
                .map_err(|e| Error::Internal(format!("Redis GET failed: {}", e)))?;

            if let Some(json) = cached {
                if let Ok(analytics) = serde_json::from_str::<Analytics>(&json) {
                    return Ok(Json(analytics));
                }
            }
        }
    }

    // Compute the analytics (expensive operation)
    let analytics = compute_analytics(&state, &report_id).await?;

    // Cache the result for 15 minutes
    if let Some(pool) = state.redis().await {
        if let Ok(mut conn) = pool.get().await {
            if let Ok(serialized) = serde_json::to_string(&analytics) {
                let _ = conn.set_ex::<_, _, ()>(&cache_key, serialized, 900).await;
            }
        }
    }

    Ok(Json(analytics))
}
```

### Cache Invalidation

Invalidate cache entries when the underlying data changes:

```rust
use acton_service::prelude::*;
use deadpool_redis::redis::AsyncCommands;

async fn update_user(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(update): Json<UserUpdate>,
) -> Result<Json<User>> {
    let db = state
        .db()
        .await
        .ok_or_else(|| Error::Internal("Database not available".into()))?;

    // Update the source of truth
    let user = sqlx::query_as::<_, User>(
        "UPDATE users SET name = $1, email = $2 WHERE id = $3
         RETURNING id, name, email",
    )
    .bind(&update.name)
    .bind(&update.email)
    .bind(id)
    .fetch_one(&db)
    .await?;

    // Invalidate the cache entry (best effort - a cache miss is not a request failure)
    if let Some(pool) = state.redis().await {
        if let Ok(mut conn) = pool.get().await {
            let cache_key = format!("user:{}", id);
            let _ = conn.del::<_, ()>(&cache_key).await;
        }
    }

    Ok(Json(user))
}
```

## Health Checks

Redis health is automatically monitored by the `/ready` endpoint:

```toml
[redis]
optional = false  # Service not ready if Redis is down
```

The readiness probe verifies pool connectivity:

```bash
curl http://localhost:8080/ready
# Returns 200 OK if Redis is healthy
# Returns 503 Service Unavailable if Redis is down
```

### Graceful Degradation

Configure Redis as optional to allow service operation even when the cache is unavailable. Because
`state.redis()` returns an `Option`, degradation falls out of the type naturally:

```toml
[redis]
optional = true  # Service remains ready, cache operations are skipped
```

```rust
use acton_service::prelude::*;
use deadpool_redis::redis::AsyncCommands;

async fn get_data(State(state): State<AppState>) -> Result<Json<Data>> {
    // Try the cache if it is available
    if let Some(pool) = state.redis().await {
        if let Ok(mut conn) = pool.get().await {
            let cached: Option<String> = conn.get("data").await.unwrap_or(None);

            if let Some(json) = cached {
                if let Ok(data) = serde_json::from_str::<Data>(&json) {
                    return Ok(Json(data));
                }
            }
        }
    }

    // Fall back to the database
    let db = state
        .db()
        .await
        .ok_or_else(|| Error::Internal("Database not available".into()))?;

    let data = fetch_from_db(&db).await?;
    Ok(Json(data))
}
```

## Token Revocation

Redis-backed token revocation for authentication middleware. Works with both PASETO (default) and JWT tokens.

```rust
use acton_service::prelude::*;
use acton_service::middleware::{RedisTokenRevocation, TokenRevocation};

async fn revoke_token(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<StatusCode> {
    if let (Some(redis), Some(jti)) = (state.redis().await, &claims.jti) {
        let revocation = RedisTokenRevocation::new(redis);

        // Calculate TTL from token expiration
        let ttl = (claims.exp - chrono::Utc::now().timestamp()) as u64;

        // Revoke the token - key pattern: token:revoked:{jti}
        revocation.revoke(jti, ttl).await?;
    }

    Ok(StatusCode::NO_CONTENT)
}
```

Token revocation is **automatically checked** when Redis is configured. The middleware checks each token's `jti` claim against the revocation list.

**Key format**: `token:revoked:{jti}`

See [Token Authentication](/docs/token-auth#token-revocation) for detailed revocation configuration.

## Cedar Policy Caching

Accelerate Cedar authorization decisions with a Redis-backed decision cache. Cedar is constructed with
`CedarAuthz::builder(...)` and handed to the `ServiceBuilder` via `with_cedar()`:

```toml
[cedar]
enabled = true
policy_path = "policies/app.cedar"
cache_enabled = true
cache_ttl_secs = 300
```

```rust
use acton_service::prelude::*;
use acton_service::middleware::{CedarAuthz, RedisPolicyCache};

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;
    init_tracing(&config)?;

    let state = AppState::builder().config(config.clone()).build().await?;

    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/users", get(list_users))
        })
        .build_routes();

    let mut builder = ServiceBuilder::new()
        .with_config(config.clone())
        .with_state(state.clone())
        .with_routes(routes);

    // Wire Cedar with a Redis-backed decision cache
    if let Some(cedar_config) = config.cedar.clone() {
        let mut cedar = CedarAuthz::builder(cedar_config);

        if let Some(redis) = state.redis().await {
            cedar = cedar.with_cache(RedisPolicyCache::new(redis));
        }

        builder = builder.with_cedar(cedar.build().await?);
    }

    builder.build().serve().await
}
```

Cached decisions are keyed as `cedar:authz:{principal}:{action}:{resource}`. See
[Cedar Authorization](/docs/cedar-auth) for policy authoring.

## Connection Pool Monitoring

Deadpool exposes live pool status:

```rust
use acton_service::prelude::*;

async fn cache_stats(State(state): State<AppState>) -> Result<Json<PoolStats>> {
    let pool = state
        .redis()
        .await
        .ok_or_else(|| Error::Internal("Redis not available".into()))?;

    let status = pool.status();

    Ok(Json(PoolStats {
        max_size: status.max_size,
        size: status.size,
        available: status.available,
    }))
}
```

## Best Practices

### Set Appropriate TTLs

Always set expiration times to prevent unbounded cache growth:

```rust
// ✅ Good - explicit TTL
conn.set_ex::<_, _, ()>("key", "value", 3600).await?;

// ❌ Bad - no expiration
conn.set::<_, _, ()>("key", "value").await?;
```

### Use Structured Cache Keys

Organize cache keys with consistent naming conventions:

```rust
// ✅ Good - structured keys
format!("user:{}:profile", user_id)
format!("report:{}:analytics", report_id)
format!("session:{}", session_id)

// ❌ Bad - inconsistent keys
format!("user{}", user_id)
format!("report_{}", report_id)
```

### Handle Cache Failures Gracefully

Never let cache failures break your service. `state.redis()` returning `None` should behave like a cache
miss, not a `500`:

```rust
// ✅ Good - the cache is an optimization, not a dependency
if let Some(pool) = state.redis().await {
    if let Ok(mut conn) = pool.get().await {
        if let Ok(Some(json)) = conn.get::<_, Option<String>>(&key).await {
            if let Ok(data) = serde_json::from_str::<Data>(&json) {
                return Ok(Json(data));
            }
        }
    }
}
// Always have a fallback
fetch_from_source(&state).await
```

### Configure Pool Size Appropriately

Match the Redis connection pool size to your workload:

```toml
[redis]
max_connections = 50  # Adjust based on concurrent request load
```

### Use Pipelining for Batch Operations

Reduce round trips for multiple operations:

```rust
use deadpool_redis::redis::pipe;

let pool = state
    .redis()
    .await
    .ok_or_else(|| Error::Internal("Redis not available".into()))?;

let mut conn = pool
    .get()
    .await
    .map_err(|e| Error::Internal(format!("Redis connection failed: {}", e)))?;

let results: Vec<Option<String>> = pipe()
    .get("key1")
    .get("key2")
    .get("key3")
    .query_async(&mut conn)
    .await
    .map_err(|e| Error::Internal(format!("Redis pipeline failed: {}", e)))?;
```

## Production Deployment

### Environment Configuration

```bash
# Production environment
export ACTON_REDIS_URL=redis://cache.prod.example.com:6379
export ACTON_REDIS_MAX_CONNECTIONS=100
```

### Kubernetes Secret Integration

```yaml
env:
  - name: ACTON_REDIS_URL
    valueFrom:
      secretKeyRef:
        name: redis-credentials
        key: url
```

### TLS/SSL Connections

For secure Redis connections:

```toml
[redis]
url = "rediss://cache.prod.example.com:6380"  # Note: rediss:// for TLS
```

## Related Features

- **[Rate Limiting](/docs/rate-limiting)** - Config-driven distributed rate limiting
- **[Session Management](/docs/session)** - Managed, cookie-backed sessions
- **[Token Authentication](/docs/token-auth)** - PASETO/JWT token revocation with Redis
- **[Cedar Authorization](/docs/cedar-auth)** - Cedar policy decision caching with Redis
- **[Health Checks](/docs/health-checks)** - Automatic Redis health monitoring
