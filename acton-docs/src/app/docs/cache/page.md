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

acton-service provides production-ready Redis integration through `redis-rs` with Deadpool connection pooling, automatic health checks, and built-in support for distributed rate limiting. Redis connections are managed automatically through the `AppState` with zero configuration required for development.

{% callout type="note" title="Agent-Managed Pools" %}
Redis connection pools are managed internally by a **RedisPoolAgent** that handles connection lifecycle, health monitoring, and graceful shutdown. You interact with pools via `state.redis()` - the agent works transparently behind the scenes. See [Reactive Architecture](/docs/reactive-architecture) for implementation details.
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

### Environment Variable Override

```bash
ACTON_REDIS_URL=redis://localhost:6379 cargo run
```

### Connection Pool Settings

The framework uses Deadpool with sensible production defaults:

- **max_connections**: Maximum number of connections in the pool (default: 50)
- **connection_timeout**: Maximum time to wait for connection (default: 30s)
- **recycle_timeout**: Time before connections are recycled (default: 5m)

## Basic Usage

Access Redis through `AppState` in your handlers:

```rust
use acton_service::prelude::*;
use redis::AsyncCommands;

#[derive(Serialize, Deserialize)]
struct User {
    id: i64,
    name: String,
    email: String,
}

async fn get_user_cached(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<User>> {
    let cache = state.cache()?;
    let mut conn = cache.get().await?;

    // Try cache first
    let cache_key = format!("user:{}", id);
    if let Ok(cached) = conn.get::<_, String>(&cache_key).await {
        if let Ok(user) = serde_json::from_str(&cached) {
            return Ok(Json(user));
        }
    }

    // Cache miss - fetch from database
    let db = state.database()?;
    let user = sqlx::query_as!(User, "SELECT * FROM users WHERE id = $1", id)
        .fetch_one(db)
        .await?;

    // Store in cache with 5 minute TTL
    let serialized = serde_json::to_string(&user)?;
    conn.set_ex(&cache_key, serialized, 300).await?;

    Ok(Json(user))
}
```

## Common Use Cases

### Session Storage

Store user sessions with automatic expiration:

```rust
use redis::AsyncCommands;

async fn create_session(
    State(state): State<AppState>,
    Json(login): Json<LoginRequest>,
) -> Result<Json<SessionResponse>> {
    let cache = state.cache()?;
    let mut conn = cache.get().await?;

    // Generate session token
    let session_id = uuid::Uuid::new_v4().to_string();
    let session_key = format!("session:{}", session_id);

    // Store session data with 1 hour TTL
    let session_data = serde_json::to_string(&SessionData {
        user_id: login.user_id,
        created_at: chrono::Utc::now(),
    })?;

    conn.set_ex(&session_key, session_data, 3600).await?;

    Ok(Json(SessionResponse { session_id }))
}
```

### Distributed Rate Limiting

Redis-backed rate limiting works across multiple service instances:

```rust
use acton_service::middleware::RedisRateLimitLayer;
use std::time::Duration;

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
        .with_middleware(|router| {
            router.layer(
                RedisRateLimitLayer::new(
                    100,  // 100 requests
                    Duration::from_secs(60)  // per minute
                )
            )
        })
        .build()
        .serve()
        .await
}
```

### Cache-Aside Pattern

Implement cache-aside (lazy loading) for expensive computations:

```rust
async fn get_analytics(
    State(state): State<AppState>,
    Path(report_id): Path<String>,
) -> Result<Json<Analytics>> {
    let cache = state.cache()?;
    let mut conn = cache.get().await?;

    let cache_key = format!("analytics:{}", report_id);

    // Check cache
    if let Ok(cached) = conn.get::<_, String>(&cache_key).await {
        if let Ok(analytics) = serde_json::from_str(&cached) {
            return Ok(Json(analytics));
        }
    }

    // Compute analytics (expensive operation)
    let analytics = compute_analytics(&state, &report_id).await?;

    // Cache result for 15 minutes
    let serialized = serde_json::to_string(&analytics)?;
    conn.set_ex(&cache_key, serialized, 900).await?;

    Ok(Json(analytics))
}
```

### Cache Invalidation

Invalidate cache entries when data changes:

```rust
async fn update_user(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(update): Json<UserUpdate>,
) -> Result<Json<User>> {
    let db = state.database()?;
    let cache = state.cache()?;

    // Update database
    let user = sqlx::query_as!(
        User,
        "UPDATE users SET name = $1, email = $2 WHERE id = $3 RETURNING *",
        update.name,
        update.email,
        id
    )
    .fetch_one(db)
    .await?;

    // Invalidate cache
    let mut conn = cache.get().await?;
    let cache_key = format!("user:{}", id);
    conn.del(&cache_key).await?;

    Ok(Json(user))
}
```

## Health Checks

Redis health is automatically monitored by the `/ready` endpoint:

```toml
[redis]
optional = false  # Service not ready if Redis is down
```

The readiness probe executes a PING command to verify connectivity:

```bash
curl http://localhost:8080/ready
# Returns 200 OK if Redis is healthy
# Returns 503 Service Unavailable if Redis is down
```

### Graceful Degradation

Configure Redis as optional to allow service operation even when cache is unavailable:

```toml
[redis]
optional = true  # Service remains ready, cache operations fail gracefully
```

```rust
async fn get_data(State(state): State<AppState>) -> Result<Json<Data>> {
    // Try cache if available
    if let Ok(cache) = state.cache() {
        if let Ok(mut conn) = cache.get().await {
            if let Ok(cached) = conn.get::<_, String>("data").await {
                if let Ok(data) = serde_json::from_str(&cached) {
                    return Ok(Json(data));
                }
            }
        }
    }

    // Fallback to database
    let db = state.database()?;
    let data = fetch_from_db(db).await?;
    Ok(Json(data))
}
```

## JWT Token Revocation

Redis-backed token revocation for authentication middleware:

```rust
use acton_service::middleware::JwtAuth;

async fn revoke_token(
    State(state): State<AppState>,
    claims: JwtClaims,
) -> Result<StatusCode> {
    let cache = state.cache()?;
    let mut conn = cache.get().await?;

    // Store revoked token JTI (JWT ID) with TTL matching token expiration
    let revocation_key = format!("revoked:{}", claims.jti);
    let ttl = (claims.exp - chrono::Utc::now().timestamp()) as usize;

    conn.set_ex(&revocation_key, "revoked", ttl).await?;

    Ok(StatusCode::NO_CONTENT)
}
```

Configure JWT middleware to check revocation:

```rust
ServiceBuilder::new()
    .with_routes(routes)
    .with_middleware(|router| {
        router.layer(
            JwtAuth::new("your-secret")
                .with_redis_revocation()  // Enable Redis-backed revocation checks
        )
    })
    .build()
    .serve()
    .await
```

## Cedar Policy Caching

Accelerate Cedar authorization decisions with Redis caching:

```rust
use acton_service::middleware::CedarAuthLayer;

ServiceBuilder::new()
    .with_routes(routes)
    .with_middleware(|router| {
        router.layer(
            CedarAuthLayer::builder()
                .with_policy_store("policies/")
                .with_redis_caching()  // Cache policy decisions for sub-5ms latency
                .build()
        )
    })
    .build()
    .serve()
    .await
```

## Connection Pool Monitoring

Monitor connection pool health:

```rust
async fn cache_stats(State(state): State<AppState>) -> Result<Json<PoolStats>> {
    let cache = state.cache()?;

    let stats = PoolStats {
        max_size: cache.status().max_size,
        size: cache.status().size,
        available: cache.status().available,
    };

    Ok(Json(stats))
}
```

## Best Practices

### Set Appropriate TTLs

Always set expiration times to prevent unbounded cache growth:

```rust
// ✅ Good - explicit TTL
conn.set_ex("key", "value", 3600).await?;

// ❌ Bad - no expiration
conn.set("key", "value").await?;
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

Never let cache failures break your service:

```rust
// ✅ Good - fallback on cache failure
if let Ok(cache) = state.cache() {
    if let Ok(data) = try_cache(&cache).await {
        return Ok(Json(data));
    }
}
// Always have a fallback
fetch_from_source().await
```

### Configure Pool Size Appropriately

Match Redis connection pool size to your workload:

```toml
[redis]
max_connections = 50  # Adjust based on concurrent request load
```

### Use Pipelining for Batch Operations

Reduce round trips for multiple operations:

```rust
use redis::pipe;

let mut conn = cache.get().await?;
let results: Vec<String> = pipe()
    .get("key1")
    .get("key2")
    .get("key3")
    .query_async(&mut *conn)
    .await?;
```

## Production Deployment

### Environment Configuration

```bash
# Production environment
export ACTON_REDIS_URL=redis://cache.prod.example.com:6379
export ACTON_REDIS_MAX_CONNECTIONS=100
```

### Redis Cluster Support

For high-availability deployments:

```toml
[redis]
url = "redis://node1:6379,redis://node2:6379,redis://node3:6379"
max_connections = 100
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

- **[Rate Limiting](/docs/rate-limiting)** - Redis-backed distributed rate limiting
- **[Authentication](/docs/authentication)** - JWT token revocation with Redis
- **[Authorization](/docs/authorization)** - Cedar policy caching with Redis
- **[Health Checks](/docs/health-checks)** - Automatic Redis health monitoring
