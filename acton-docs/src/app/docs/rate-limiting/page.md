---
title: Rate Limiting
nextjs:
  metadata:
    title: Rate Limiting
    description: Protect your services from abuse and overload with distributed Redis-backed or local Governor rate limiting, supporting per-user and per-client limits.
---

acton-service provides two rate limiting implementations: Redis-backed for distributed systems and Governor for single-instance deployments. Both support per-user and per-client limits via JWT claims.

## Quick Start

Rate limiting in acton-service is configured declaratively via `config.toml` and automatically applied to all endpoints. No code changes required.

### Configuration

```toml
[rate_limit]
enabled = true
per_user_rpm = 100          # Per-user rate limit: 100 requests per minute
per_client_rpm = 1000       # Per-client rate limit: 1000 requests per minute
```

Rate limits are automatically applied based on JWT claims:
- **Per-user limits** use the JWT `sub` claim as identifier
- **Per-client limits** use the JWT `client_id` claim as identifier
- **Anonymous requests** fall back to IP address-based limiting

No additional code needed. The rate limiter is automatically integrated into the middleware stack during service initialization.

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

```toml
[redis]
url = "redis://localhost:6379"
pool_size = 10

[rate_limit]
enabled = true
default_requests = 100
default_window_secs = 60
```

### Basic Usage

```rust
use acton_service::middleware::RateLimitLayer;
use std::time::Duration;

// Allow 100 requests per 60 seconds
let limiter = RateLimitLayer::new(100, Duration::from_secs(60));

ServiceBuilder::new()
    .with_routes(routes)
    .with_middleware(|router| {
        router.layer(limiter)
    })
    .build()
```

### Per-Endpoint Limits

Different endpoints often require different rate limits:

```rust
use tower::ServiceBuilder;

// Public endpoints - stricter limits
let public_routes = Router::new()
    .route("/api/public/*", get(public_handler))
    .layer(RateLimitLayer::new(10, Duration::from_secs(60)));

// Authenticated endpoints - more generous limits
let auth_routes = Router::new()
    .route("/api/private/*", get(private_handler))
    .layer(JwtAuth::new("secret"))
    .layer(RateLimitLayer::new(100, Duration::from_secs(60)));

// Admin endpoints - no rate limiting
let admin_routes = Router::new()
    .route("/api/admin/*", get(admin_handler))
    .layer(JwtAuth::new("secret"))
    .layer(CedarAuthzLayer::new(config));

let app = Router::new()
    .merge(public_routes)
    .merge(auth_routes)
    .merge(admin_routes);
```

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
- Predefined time window presets
- Minimal configuration required
- Drop-in replacement for Redis limiter

**Limitations**
- Not suitable for multi-instance deployments (limits multiply)
- Counters reset on service restart
- No cross-service synchronization

### Presets

```rust
use acton_service::middleware::GovernorRateLimitLayer;

// Per-second limits
GovernorRateLimitLayer::per_second(10);   // 10 requests/second

// Per-minute limits
GovernorRateLimitLayer::per_minute(100);  // 100 requests/minute

// Per-hour limits
GovernorRateLimitLayer::per_hour(1000);   // 1000 requests/hour
```

### Custom Windows

```rust
use std::time::Duration;

// Custom: 500 requests per 5 minutes
GovernorRateLimitLayer::new(500, Duration::from_secs(300));
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

Per-user limits are automatically applied when rate limiting is enabled. They use the JWT `sub` claim to identify each user.

### How It Works

1. JWT middleware validates token and extracts claims
2. Rate limiter uses `sub` claim as identifier
3. Each user gets independent rate limit bucket
4. Anonymous requests use IP address as fallback

**Example:**
```text
User "user:123" → rate_limit:user:123:1700000000
User "user:456" → rate_limit:user:456:1700000000
Anonymous (IP 1.2.3.4) → rate_limit:ip:1.2.3.4:1700000000
```

### Tiered Limits by Role

Implement different limits for different user tiers:

```rust
use acton_service::middleware::RateLimitLayer;

async fn custom_rate_limiter(
    claims: Claims,
) -> Result<(), RateLimitError> {
    let limit = if claims.roles.contains(&"premium".to_string()) {
        1000  // Premium users: 1000 req/hour
    } else if claims.roles.contains(&"user".to_string()) {
        100   // Regular users: 100 req/hour
    } else {
        10    // Anonymous: 10 req/hour
    };

    // Apply limit logic
    Ok(())
}
```

## Per-Client Rate Limiting

Service-to-service authentication often uses client IDs instead of user IDs. Per-client limits are automatically applied when the JWT includes a `client_id` claim.

### JWT Token for Service Clients

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

```rust
// Global limit: 10,000 req/minute total
let global_limiter = RateLimitLayer::new(10_000, Duration::from_secs(60));

// Per-user limit: 100 req/minute per user
let user_limiter = RateLimitLayer::new(100, Duration::from_secs(60))
    .with_user_limits(true);

ServiceBuilder::new()
    .with_routes(routes)
    .with_middleware(|router| {
        router
            .layer(JwtAuth::new("secret"))
            .layer(global_limiter)   // Apply global limit first
            .layer(user_limiter)     // Then per-user limit
    })
    .build()
```

### Endpoint-Specific Overrides

```rust
// Most endpoints: 100 req/min
let default_routes = Router::new()
    .route("/api/v1/users", get(list_users))
    .route("/api/v1/documents", get(list_documents))
    .layer(RateLimitLayer::new(100, Duration::from_secs(60)));

// Expensive endpoint: 10 req/min
let expensive_routes = Router::new()
    .route("/api/v1/reports/generate", post(generate_report))
    .layer(RateLimitLayer::new(10, Duration::from_secs(60)));

let app = Router::new()
    .merge(default_routes)
    .merge(expensive_routes);
```

## Troubleshooting

### Rate Limits Not Applied

**Symptom**: Clients can exceed configured limits

**Possible Causes:**
1. Multiple service instances with Governor limiter
2. Redis connection issues
3. Middleware order incorrect

**Solutions:**
- Switch to Redis-backed limiter for multi-instance
- Verify Redis connectivity with `redis-cli PING`
- Place rate limiter after authentication but before handlers

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
- [JWT Authentication](/docs/jwt-auth) - Enable per-user limits
- [Resilience Patterns](/docs/resilience) - Combine with circuit breakers
- [Observability](/docs/observability) - Monitor rate limit metrics
