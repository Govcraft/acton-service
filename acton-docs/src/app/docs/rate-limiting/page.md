---
title: Rate Limiting
nextjs:
  metadata:
    title: Rate Limiting
    description: Protect your services from abuse and overload with distributed Redis-backed or local Governor rate limiting, supporting per-user and per-client limits.
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


acton-service provides two rate limiting implementations: Redis-backed for distributed systems and Governor for single-instance deployments. Both support per-user and per-client limits via token claims.

## Quick Start

Rate limiting in acton-service is configured declaratively via `config.toml` and automatically applied to all endpoints when the `governor` feature is enabled. No code changes required.

### Configuration

```toml
[rate_limit]
per_user_rpm = 100          # Per-user rate limit: 100 requests per minute
per_client_rpm = 1000       # Per-client rate limit: 1000 requests per minute
auto_apply = true           # Auto-attach the middleware (default: true)
trust_forwarded_headers = false  # Trust X-Forwarded-For / X-Real-IP (default: false)
```

Rate limits are automatically applied based on token claims:
- **Per-user limits** use the `sub` claim as identifier
- **Per-client limits** use the `client_id` claim as identifier
- **Anonymous requests** fall back to IP address-based limiting

The middleware is wired by `ServiceBuilder` to the **outer** router (before any `Router::nest` strips the path prefix), so route-rate-limit keys like `"POST /api/v1/uploads"` match the full request path as documented.

### Auto-apply opt-out

Set `auto_apply = false` to wire the middleware manually inside your route closure. Useful when you need to scope rate-limiting to a subset of routes or apply it after a custom middleware.

### Trusting forwarded-for headers

`trust_forwarded_headers` defaults to `false`. **Only enable it when running behind a proxy you trust** (nginx, ALB, Cloudflare). Direct clients can otherwise spoof their IP via `X-Forwarded-For` and bypass per-IP limits.

When enabled, the middleware resolves the client IP in this order:
1. First entry of `X-Forwarded-For` (left-most = original client)
2. `X-Real-IP`
3. Direct TCP peer address (`ConnectInfo<SocketAddr>`)

When disabled, only the direct peer address is used.

{% callout type="warning" title="Breaking change in 0.23" %}
Route-rate-limit keys now match the **full** request path (e.g. `"POST /api/v1/uploads"`). Configurations that relied on the previous bug — using post-nest keys like `"POST /uploads"` for routes registered under `add_version(V1, ...)` — must be updated. See the [CHANGELOG](https://github.com/Govcraft/acton-service/blob/main/CHANGELOG.md) for details.
{% /callout %}

## Per-Route Configuration

Different endpoints often need different rate limits. A report generation endpoint might allow 10 requests/minute while read-only endpoints allow 1000. Configure per-route limits declaratively in `config.toml`:

### Basic Per-Route Limits

```toml
[rate_limit]
per_user_rpm = 200          # Default for all routes
per_client_rpm = 1000       # Default for service clients
window_secs = 60

# Expensive report endpoint - 10 req/min
[rate_limit.routes."/api/v1/reports/generate"]
requests_per_minute = 10
burst_size = 2
per_user = true             # Each user gets 10/min (default)

# High-traffic read endpoint - 500 req/min
[rate_limit.routes."/api/v1/users"]
requests_per_minute = 500
burst_size = 50
per_user = true
```

### Method-Specific Limits

Prefix the route with an HTTP method to apply limits only to that method:

```toml
# POST uploads are expensive - limit strictly
[rate_limit.routes."POST /api/v1/uploads"]
requests_per_minute = 5
burst_size = 1
per_user = true

# GET uploads (list/download) can be more generous
[rate_limit.routes."GET /api/v1/uploads"]
requests_per_minute = 100
burst_size = 10
per_user = true
```

### Wildcard Patterns

Use wildcards to match multiple routes:

```toml
# Single segment wildcard: * matches one path segment
# Matches: /api/v1/admin/users, /api/v1/admin/settings
# Does NOT match: /api/v1/admin/users/123
[rate_limit.routes."/api/v1/admin/*"]
requests_per_minute = 50
burst_size = 5
per_user = true

# Multi-segment wildcard: ** matches any number of segments
# Matches: /api/v1/admin/users, /api/v1/admin/users/123, /api/v1/admin/a/b/c
[rate_limit.routes."/api/**/heavy"]
requests_per_minute = 10
burst_size = 2
per_user = true
```

### Path Normalization

Routes with dynamic IDs are automatically normalized:

```toml
# This config matches ALL of these request paths:
#   /api/v1/users/123
#   /api/v1/users/456
#   /api/v1/users/550e8400-e29b-41d4-a716-446655440000 (UUID)
[rate_limit.routes."/api/v1/users/{id}"]
requests_per_minute = 100
burst_size = 10
per_user = true

# Nested dynamic segments work too:
# Matches: /api/v1/users/123/posts/456
[rate_limit.routes."/api/v1/users/{id}/posts/{id}"]
requests_per_minute = 50
burst_size = 5
per_user = true
```

**Normalization Rules:**
- Numeric IDs (`123`, `456789`) → `{id}`
- UUIDs (`550e8400-e29b-41d4-a716-446655440000`) → `{id}`
- Version strings (`v1`, `v2`) are preserved (not normalized)

### Global vs Per-User Route Limits

By default, route limits are per-user. Set `per_user = false` for a global limit shared across all users:

```toml
# Per-user (default): Each user gets 10 req/min
[rate_limit.routes."/api/v1/expensive"]
requests_per_minute = 10
per_user = true

# Global: 100 req/min total, shared by ALL users
[rate_limit.routes."/api/v1/shared-resource"]
requests_per_minute = 100
per_user = false            # All users share this limit!
```

### Pattern Priority

Routes are matched in priority order:

1. **Method-prefixed exact match** (highest priority)
   - `POST /api/v1/uploads` matches before `/api/v1/uploads`
2. **Exact path match**
   - `/api/v1/users` matches before `/api/v1/*`
3. **Wildcard patterns** (sorted by specificity)
   - More specific patterns match first
   - `/api/v1/users/*` matches before `/api/v1/*`
   - `/api/v1/*` matches before `/api/**`
4. **Global defaults** (lowest priority)
   - Falls back to `per_user_rpm` / `per_client_rpm`

### Redis Key Structure for Per-Route Limits

Per-route limits use a distinct Redis key structure:

```text
# Per-user route limit
route:/api/v1/users/{id}:user:user123 → counter (expires in window_secs)

# Global route limit (per_user = false)
route:/api/v1/shared:global → counter (expires in window_secs)
```

## Redis vs Governor: Which Should You Use?

{% callout type="note" title="Decision Guide" %}
Use **Redis** for multi-instance deployments. Use **Governor** for single-instance services or local development. The wrong choice can lead to limit multiplication or single points of failure.
{% /callout %}

### Comparison Matrix

| Feature | Redis (Distributed) | Governor (Local) |
|---------|-------------------|------------------|
| **Use Case** | Production multi-instance | Single instance / dev |
| **Consistency** | ✅ Shared across replicas | ❌ Per-instance only |
| **Latency** | ~1ms (network call) | ~0.01ms (in-memory) |
| **Dependencies** | Requires Redis server | None (built-in) |
| **Failure Mode** | Depends on Redis availability | Always available |
| **Persistence** | Survives restarts | Lost on restart |
| **Complexity** | Higher (external service) | Lower (self-contained) |

### Use Redis When...

✅ **Running multiple service instances** (Kubernetes, Docker Swarm, load balancer)
```
Load Balancer
    ├─ Service Instance 1 ──┐
    ├─ Service Instance 2 ──┼─→ Shared Redis (100 req/min total)
    └─ Service Instance 3 ──┘
```

✅ **Need consistent limits across replicas**
- User makes 60 requests to Instance 1
- User makes 40 requests to Instance 2
- Total: 100 requests (limit enforced correctly)

✅ **Production deployments with horizontal scaling**

✅ **Need limits to survive service restarts**

### Use Governor When...

✅ **Running single instance only** (dev laptop, small internal tool)
```
Single Service Instance (100 req/min)
```

✅ **Local development** (no Redis dependency needed)

✅ **Testing rate limiting behavior** (faster, simpler)

✅ **Don't need distributed coordination**

### What Happens with Wrong Choice?

**Problem 1: Using Governor in Multi-Instance** (Limit Multiplication)
```
Load Balancer
    ├─ Instance 1: 100 req/min ──┐
    ├─ Instance 2: 100 req/min ──┼─→ User gets 300 req/min total!
    └─ Instance 3: 100 req/min ──┘

❌ Each instance has its own limit
❌ User can bypass by hitting different instances
❌ 3 instances × 100 = 300 effective limit (not 100!)
```

**Problem 2: Using Redis for Single Instance** (Unnecessary Complexity)
```
Single Instance ──→ Redis ──→ Extra network latency
                  └─ Extra failure point
                  └─ Extra infrastructure to manage

❌ Adds latency for no benefit
❌ Redis failure breaks rate limiting
❌ More infrastructure to maintain
```

### How to Choose: Decision Tree

```
Do you run multiple instances of this service?
│
├─ YES → How many instances?
│  │
│  ├─ 2+ instances → Use Redis
│  │                 (prevents limit multiplication)
│  │
│  └─ Autoscaling? → Use Redis
│                    (instance count varies)
│
└─ NO → Single instance confirmed?
   │
   ├─ YES, always 1 instance → Use Governor
   │                           (simpler, faster)
   │
   └─ Might scale later → Use Redis now
                          (easier to start with Redis than migrate later)
```

### How the Limiter Is Selected

There is no `backend` key in `[rate_limit]`. Both limiters read the **same** `[rate_limit]` configuration — the same `per_user_rpm`, `per_client_rpm`, `window_secs`, and `[rate_limit.routes]` entries apply to either. What differs is which limiter is wired up:

| Limiter | Type | Feature | How it is wired |
| --- | --- | --- | --- |
| Governor (in-memory) | `GovernorRateLimit` | `governor` | Auto-applied by `ServiceBuilder` when `auto_apply = true` (the default) |
| Redis (distributed) | `RateLimit` | `cache` | Wired manually in your route closure |

**Governor (single-instance) — enable the feature and configure:**

```toml
[rate_limit]
per_user_rpm = 100       # 100 per this instance
per_client_rpm = 1000
auto_apply = true        # Default
```

```toml
# Cargo.toml
[dependencies]
acton-service = { version = "{% version() %}", features = ["governor"] }
```

**Redis (multi-instance) — configure Redis, then attach `RateLimit` yourself:**

```toml
[redis]
url = "redis://redis-cluster:6379"
max_connections = 10

[rate_limit]
per_user_rpm = 100       # 100 total across all instances
per_client_rpm = 1000
auto_apply = false       # Don't also auto-attach the in-memory limiter
```

See [Redis-Backed Rate Limiting](#redis-backed-rate-limiting) below for the wiring code.

### When Limits Feel Wrong

**Symptom:** "I set 100 req/min but users can make 300 requests!"

**Diagnosis:**
```bash
# Check how many instances are running
kubectl get pods -l app=my-service

# If you see 3 pods and using Governor:
# 3 instances × 100 req/min = 300 effective limit
```

**Fix:** Switch to the Redis-backed `RateLimit` middleware for distributed coordination.

**Symptom:** "Rate limiting is slow and sometimes fails"

**Diagnosis:**
```bash
# Check if Redis is reachable
redis-cli ping

# Check latency
redis-cli --latency
```

**Fix:**
- If Redis is down, check Redis deployment
- If latency is high, consider Redis location (same datacenter)
- If single instance, consider switching to Governor

## Redis-Backed Rate Limiting

Redis-backed rate limiting is essential for multi-instance deployments where consistent limits must apply across all service replicas.

### Features

**Distributed Consistency**
- Shared rate limit counters across all service instances
- No risk of limit multiplication in horizontal scaling
- Atomic increment operations prevent race conditions

**Production Ready**
- Handles service restarts gracefully (counters persist in Redis)
- Low latency overhead (<1ms for Redis operations)
- Automatic key expiration with sliding windows

**Flexible Configuration**
- Per-endpoint limits with different windows
- Global and user-specific limits
- Customizable limit buckets

### Configuration

The Redis-backed limiter requires the `cache` feature and reads the same `[rate_limit]` section as the governor limiter:

```toml
[redis]
url = "redis://localhost:6379"
max_connections = 10

[rate_limit]
per_user_rpm = 100
per_client_rpm = 1000
window_secs = 60
auto_apply = false       # Don't also auto-attach the in-memory governor limiter
```

### Wiring the Middleware

`RateLimit::new()` takes the `RateLimitConfig` and a Redis pool. Attach it with `axum::middleware::from_fn_with_state`:

```rust
use acton_service::prelude::*;
use acton_service::middleware::RateLimit;
use axum::middleware::from_fn_with_state;

let config = Config::load()?;
let state = AppState::default();

// The pool is managed by the framework's Redis pool agent
let redis_pool = state.redis().await.expect("redis pool configured");

let rate_limit = RateLimit::new(config.rate_limit.clone(), redis_pool);

let routes = VersionedApiBuilder::new()
    .with_base_path("/api")
    .add_version(ApiVersion::V1, |router| {
        router
            .route("/users", get(list_users))
            .route("/reports/generate", post(generate_report))
            .layer(from_fn_with_state(rate_limit, RateLimit::middleware))
    })
    .build_routes();

ServiceBuilder::new()
    .with_config(config)
    .with_state(state)
    .with_routes(routes)
    .build()
    .serve()
    .await?;
```

### Per-Endpoint Limits

Per-endpoint limits are **configuration**, not code. `RateLimit` reads the `[rate_limit.routes]` table and applies the matching override — one middleware instance covers every route:

```toml
# Public endpoints - stricter limits
[rate_limit.routes."/api/v1/public/info"]
requests_per_minute = 10

# Authenticated endpoints - more generous limits
[rate_limit.routes."/api/v1/users"]
requests_per_minute = 100

# Admin endpoints - higher limits
[rate_limit.routes."/api/v1/admin/settings"]
requests_per_minute = 1000
```

See [Per-Route Configuration](#per-route-configuration) above for wildcards, method prefixes, and path normalization.

### Redis Key Structure

```text
Key Format: rate_limit:{identifier}:{window}
Value: Request count
TTL: Window duration

Examples:
rate_limit:global:1700000000 → 42 (expires in 60s)
rate_limit:user:123:1700000000 → 15 (expires in 60s)
rate_limit:client:app1:1700000000 → 8 (expires in 60s)
```

## Governor Rate Limiting

Governor provides in-memory rate limiting for single-instance deployments with zero external dependencies.

### Features

**Zero Dependencies**
- No Redis or external services required
- Perfect for development and small deployments
- Lower latency (no network calls)

**Simple Configuration**
- Configured entirely through `[rate_limit]` in `config.toml`
- Auto-applied by `ServiceBuilder` — no code required
- Reads the same per-route table as the Redis limiter

**Limitations**
- Not suitable for multi-instance deployments (limits multiply)
- Counters reset on service restart
- No cross-service synchronization

### Automatic Wiring

With the `governor` feature enabled and `auto_apply = true` (the default), `ServiceBuilder` constructs `GovernorRateLimit::new(config.rate_limit)` and attaches it for you. Nothing to write:

```toml
[rate_limit]
per_user_rpm = 100
per_client_rpm = 1000
window_secs = 60
```

### Manual Wiring

Set `auto_apply = false` when you need to scope the limiter to a subset of routes:

```rust
use acton_service::prelude::*;
use acton_service::middleware::GovernorRateLimit;
use axum::middleware::from_fn_with_state;

let gov = GovernorRateLimit::new(config.rate_limit.clone());

let routes = VersionedApiBuilder::new()
    .with_base_path("/api")
    .add_version(ApiVersion::V1, |router| {
        router
            .route("/users", get(list_users))
            .layer(from_fn_with_state(gov, GovernorRateLimit::middleware))
    })
    .build_routes();
```

### Quota Presets

`GovernorConfig` carries per-second, per-minute, and per-hour quota presets for building a standalone quota outside the `[rate_limit]` config:

```rust
use acton_service::middleware::GovernorConfig;

GovernorConfig::per_second(10);   // 10 requests/second
GovernorConfig::per_minute(100);  // 100 requests/minute
GovernorConfig::per_hour(1000);   // 1000 requests/hour
```

### When to Use Governor

**Good For:**
- Development and testing environments
- Single-instance production deployments
- Internal services with low traffic
- Prototype and proof-of-concept projects

**Not Suitable For:**
- Kubernetes deployments with multiple replicas
- Auto-scaling environments
- Services requiring persistent rate limits
- Multi-region deployments

## Per-User Rate Limiting

Per-user limits are automatically applied when rate limiting is enabled. They use the `sub` claim from PASETO or JWT tokens to identify each user.

### How It Works

1. Token middleware validates PASETO/JWT and extracts claims
2. Rate limiter uses `sub` claim as identifier
3. Each user gets independent rate limit bucket
4. Anonymous requests use IP address as fallback

**Example:**
```text
User "user:123" → rate_limit:user:123:1700000000
User "user:456" → rate_limit:user:456:1700000000
Anonymous (IP 1.2.3.4) → rate_limit:ip:1.2.3.4:1700000000
```

### Per-User vs Per-Client Buckets

Both limiters key their buckets from the validated token claims — there is no `with_user_limits()` switch to flip. The bucket is chosen automatically:

- `sub` claim present → per-user bucket, limited by `per_user_rpm`
- `client_id` claim present → per-client bucket, limited by `per_client_rpm`
- No claims → per-IP bucket

Token authentication runs before rate limiting in the `ServiceBuilder` middleware order, so claims are always available to the limiter when a valid token is presented.

## Per-Client Rate Limiting

Service-to-service authentication often uses client IDs instead of user IDs. Per-client limits are automatically applied when the token includes a `client_id` claim.

### Token for Service Clients

```json
{
  "sub": "service:api-gateway",
  "client_id": "api-gateway-prod",
  "roles": ["service"],
  "perms": ["read:all", "write:all"],
  "exp": 1735689600,
  "iat": 1735603200,
  "jti": "service-token-abc123"
}
```

Rate limiter uses `client_id` claim: `rate_limit:client:api-gateway-prod:1700000000`

## Response Headers

Rate limit middleware adds standard headers to responses:

```http
HTTP/1.1 200 OK
X-RateLimit-Limit: 100
X-RateLimit-Remaining: 87
X-RateLimit-Reset: 1700000060
```

**Header Meanings:**
- `X-RateLimit-Limit`: Maximum requests allowed in window
- `X-RateLimit-Remaining`: Requests remaining in current window
- `X-RateLimit-Reset`: Unix timestamp when limit resets

### 429 Too Many Requests

When limit exceeded:

```http
HTTP/1.1 429 Too Many Requests
X-RateLimit-Limit: 100
X-RateLimit-Remaining: 0
X-RateLimit-Reset: 1700000060
Retry-After: 45

{
  "error": "Rate limit exceeded",
  "code": "RATE_LIMIT_EXCEEDED",
  "status": 429,
  "retry_after": 45
}
```

## Audit Integration

When the `audit` feature is enabled, the rate-limit middleware automatically emits an `HttpRequestDenied` audit event at `Warning` severity whenever a request is rejected because the configured limit was exceeded. Only the limit-exceeded path emits — Redis connection failures and other infrastructure errors do not, since they aren't security signal.

The event records:

- The authenticated subject (from JWT `sub`, if claims are present)
- Client user-agent, request-id, and IP (from `X-Forwarded-For` / `X-Real-IP`)

No additional code is required — emission is on by default when `audit_auth_events: true` is set in the audit configuration (the default). See [Audit Logging](/docs/audit) for storage and SIEM export.

This event is well-suited to anomaly-detection rules: a sudden burst of `http.request.denied` events for a single subject is a strong abuse signal that an unauthenticated 429 response alone doesn't surface.

## Choosing the Right Limiter

| Feature | Redis-Backed | Governor |
|---------|-------------|----------|
| Multi-instance support | ✅ Yes | ❌ No |
| Zero dependencies | ❌ Requires Redis | ✅ Yes |
| Persistent counters | ✅ Yes | ❌ Memory only |
| Latency | ~1ms | <0.1ms |
| Horizontal scaling | ✅ Consistent | ❌ Multiplies limits |
| Development | ✅ Good | ✅ Better |
| Production (single) | ✅ Good | ✅ Good |
| Production (scaled) | ✅ Required | ❌ Not suitable |

**Decision Guide:**

Choose **Redis-backed** if:
- Running multiple service instances
- Using Kubernetes with replicas > 1
- Implementing auto-scaling
- Need persistent limits across restarts

Choose **Governor** if:
- Single instance deployment
- Development environment
- No Redis infrastructure available
- Sub-millisecond latency required

## Advanced Patterns

### Combined Global + Per-User Limits

A single limiter instance handles both. Set the global defaults with `per_user_rpm` / `per_client_rpm`, then add a `per_user = false` route entry for any endpoint that needs one shared bucket across all callers:

```toml
[rate_limit]
per_user_rpm = 100          # Each user: 100 req/min
per_client_rpm = 1000       # Each service client: 1000 req/min

# One shared 10,000 req/min budget for this endpoint, across ALL callers
[rate_limit.routes."/api/v1/search"]
requests_per_minute = 10_000
per_user = false
```

Token authentication is configured in the same file — no `JwtAuth::new(...)` call and no manual layer ordering. `ServiceBuilder` always runs token auth before rate limiting, so claims are populated by the time the limiter picks a bucket:

```toml
[token]
format = "jwt"
public_key_path = "./keys/jwt-public.pem"
algorithm = "RS256"
```

See [Token Authentication](/docs/token-auth) for PASETO and the other JWT algorithms.

### Endpoint-Specific Overrides

```toml
# Most endpoints: 100 req/min
[rate_limit.routes."/api/v1/users"]
requests_per_minute = 100

[rate_limit.routes."/api/v1/documents"]
requests_per_minute = 100

# Expensive endpoint: 10 req/min
[rate_limit.routes."POST /api/v1/reports/generate"]
requests_per_minute = 10
burst_size = 2
```

## Troubleshooting

### Rate Limits Not Applied

**Symptom**: Clients can exceed configured limits

**Possible Causes:**
1. Multiple service instances with Governor limiter
2. Redis connection issues
3. `auto_apply = false` with no manually-wired limiter
4. The `governor` feature is not enabled

**Solutions:**
- Switch to the Redis-backed limiter for multi-instance
- Verify Redis connectivity with `redis-cli PING`
- Confirm `auto_apply = true`, or that you attached the middleware yourself
- Confirm the `governor` (or `cache`) feature is enabled in `Cargo.toml`

### Redis Connection Errors

**Symptom**: 500 errors with Redis connection failures

**Solutions:**
- Verify Redis is running: `docker ps | grep redis`
- Check Redis URL in configuration
- Ensure network connectivity to Redis
- Check Redis logs: `docker logs <redis-container>`

### Inconsistent Limits Across Instances

**Symptom**: Different instances report different remaining counts

**Possible Causes:**
1. Using Governor instead of Redis-backed limiter
2. Clock skew between instances

**Solutions:**
- Switch to Redis-backed limiter
- Synchronize server clocks with NTP

## Performance Considerations

**Redis Overhead:**
- Typical latency: 0.5-2ms per request
- Connection pooling reduces overhead
- Pipelining available for bulk operations

**Governor Overhead:**
- Typical latency: <0.1ms per request
- No network calls required
- Hash table lookup only

**Optimization Tips:**
1. Use connection pooling for Redis
2. Set appropriate pool sizes (10-50 connections)
3. Monitor Redis CPU and memory usage
4. Consider caching rate limit checks (if very high traffic)

## Next Steps

- [Configure Redis](/docs/cache) - Set up Redis for distributed rate limiting
- [Token Authentication](/docs/token-auth) - Enable per-user limits with PASETO/JWT
- [Resilience Patterns](/docs/resilience) - Combine with circuit breakers
- [Observability](/docs/observability) - Monitor rate limit metrics
