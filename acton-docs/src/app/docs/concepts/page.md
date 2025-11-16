---
title: Core Concepts
nextjs:
  metadata:
    title: Core Concepts
    description: Fundamental concepts and patterns in acton-service - what happens automatically, when things occur, and how the framework works.
---

Understanding the fundamental concepts behind acton-service helps you make better architectural decisions and debug issues faster.

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is and why to use it, then return here to learn the foundational concepts.
{% /callout %}

---

## Relationship to Axum

### What is Axum?

[Axum](https://github.com/tokio-rs/axum) is a web framework built on top of Tower and Tokio. It provides:
- HTTP routing (`Router`, route handlers)
- Request extractors (`Path`, `Query`, `Json`, `State`)
- Middleware system (`Layer` trait)
- Type-safe handler functions

### How acton-service Builds on Axum

**acton-service IS Axum**, with batteries included:

```rust
// Raw Axum
let app = Router::new()
    .route("/hello", get(|| async { "Hello" }));

// acton-service adds:
// - Type-enforced API versioning (prevents unversioned routes)
// - Automatic health/readiness endpoints
// - Built-in observability (tracing, metrics, logging)
// - Production middleware (auth, rate limiting, resilience)
// - Connection pool management
// - Configuration system
```

When you see `Router`, `get()`, `post()`, handler functions - **these come from Axum**. acton-service wraps Axum's router in `VersionedRoutes` to enforce versioning and adds automatic features via `ServiceBuilder`.

**Think of it as:** `acton-service = Axum + production best practices + type enforcement`

---

## What Happens Automatically

A core feature of acton-service is reducing boilerplate by handling cross-cutting concerns automatically. Here's what happens without any code from you:

### At Compile Time

**1. API Version Enforcement**
- The type system prevents creating unversioned routes
- `VersionedRoutes` is an opaque type you can't inspect or modify
- Compiler error if you try to use raw `Router` with `ServiceBuilder`

```rust
// ❌ Won't compile
let app = Router::new().route("/unversioned", get(handler));
ServiceBuilder::new().with_routes(app)
//                    ^^^ expected VersionedRoutes, found Router
```

**2. SQL Query Verification** (with SQLx)
- If you use the `database` feature, SQL queries are verified against your schema
- Requires `DATABASE_URL` environment variable during `cargo build`
- Catches SQL errors at compile time instead of runtime

### At Service Startup

**3. Configuration Loading**
- Reads config from multiple sources in order:
  1. Environment variables (`ACTON_*` prefix)
  2. `./config.toml` (current directory)
  3. `~/.config/acton-service/{service_name}/config.toml`
  4. `/etc/acton-service/{service_name}/config.toml`
  5. Defaults
- No code required - happens automatically in `ServiceBuilder::new()`

**4. Connection Pool Creation**
- If `database`, `cache`, or `events` features enabled AND configured, pools are created
- With `lazy_init: true` (default), connections happen in background (non-blocking)
- With `lazy_init: false`, service waits for connections before starting

**5. Health Endpoint Registration**
- `/health` endpoint automatically created (always returns 200 if service is alive)
- `/ready` endpoint automatically created (checks dependency health)
- No code required - inspects your config to determine dependencies

**6. Observability Initialization**
- OpenTelemetry tracing initialized
- Metrics collection started
- JSON structured logging configured
- Request ID generation enabled
- All automatic based on config

### During Request Processing

**7. Request Tracking**
- Unique request ID generated for each request
- Added to response headers as `x-request-id`
- Included in all log entries for correlation
- Propagated to downstream services automatically

**8. Distributed Tracing**
- Span created for each request
- Parent trace ID extracted from headers if present
- Context propagated to downstream calls
- Sent to OTLP endpoint automatically

**9. Metrics Collection**
- HTTP request count, latency, status codes tracked
- Exported via OpenTelemetry automatically
- Histograms for percentile calculations

**10. Middleware Execution**
- Configured middleware layers run in order
- Authentication, rate limiting, compression, CORS, etc.
- Applied automatically via `ServiceBuilder`

---

## Configuration vs Code vs Runtime

Understanding when to use configuration files vs code, and what changes require service restart vs recompilation:

### Configuration Files (Runtime)

**Requires: Service restart** | **Does NOT require: Recompilation or redeployment**

Use `config.toml` or environment variables for:
- Connection strings (database URL, Redis URL, NATS URL)
- Pool sizes (max connections, timeouts)
- Middleware settings (rate limits, circuit breaker thresholds)
- Observability endpoints
- Feature toggles (enable/disable middleware)

```toml
# config.toml changes require RESTART but not rebuild
[database]
max_connections = 50  # Change and restart

[middleware.resilience]
circuit_breaker_threshold = 0.5  # Change and restart
```

**Exception:** Some config supports hot-reload (Cedar policies with `hot_reload: true`)

### Code Changes (Compile-time)

**Requires: Recompilation and redeployment**

Use code for:
- API routes and handlers
- Business logic
- Data structures
- Cargo feature flags

```toml
# Cargo.toml feature changes require REBUILD
[dependencies]
{% dep("databaseCache") %}
# Adding/removing features = recompile + redeploy
```

### What Happens When

| Change | Compile | Restart | Redeploy | Example |
|--------|---------|---------|----------|---------|
| Database pool size | ❌ | ✅ | ❌ | `max_connections = 100` |
| Circuit breaker threshold | ❌ | ✅ | ❌ | `circuit_breaker_threshold = 0.7` |
| Rate limit values | ❌ | ✅ | ❌ | `per_user_rpm = 500` |
| Add new route | ✅ | ✅ | ✅ | `router.route("/new", get(handler))` |
| Enable feature flag | ✅ | ✅ | ✅ | `features = ["database"]` |
| Cedar policies (hot-reload) | ❌ | ❌ | ❌ | Policy file changed, auto-reloaded |
| Environment variables | ❌ | ✅ | Depends | `ACTON_SERVICE_PORT=9000` |

---

## The Middleware Concept

### What is Middleware?

Middleware is code that runs **before** and/or **after** your request handlers to provide cross-cutting functionality. Think of it as layers wrapped around your handler:

```
Request → [Middleware 1] → [Middleware 2] → [Handler] → [Middleware 2] → [Middleware 1] → Response
          ↓ CORS          ↓ Auth          Your Code   ↑ Compression   ↑ Metrics
```

### Common Middleware Functions

- **Authentication** - Verify JWT, extract user claims
- **Authorization** - Check permissions before allowing access
- **Rate Limiting** - Prevent abuse by limiting request rates
- **Compression** - Compress responses to save bandwidth
- **CORS** - Allow cross-origin requests from browsers
- **Logging** - Record requests and responses
- **Metrics** - Collect performance data
- **Resilience** - Circuit breakers, retries, bulkheads
- **Request Tracking** - Add correlation IDs

### How acton-service Uses Middleware

```rust
ServiceBuilder::new()
    .with_routes(routes)
    .with_middleware(|router| {
        router
            .layer(JwtAuthLayer::new(config.jwt))      // Auth
            .layer(ResilienceLayer::new())              // Circuit breaker
            .layer(RateLimitLayer::new(config.rate))    // Rate limiting
    })
    .build()
```

Or use config-driven middleware (automatic):

```toml
[middleware.resilience]
circuit_breaker_enabled = true

[middleware.metrics]
enabled = true
```

---

## API Versioning Philosophy

### The Problem

Most frameworks make API versioning optional:

```rust
// Easy to forget versioning
app.route("/users", get(get_users))  // No version!
```

Months later, you need to change the response format but can't without breaking clients.

### The acton-service Solution

**Type-enforced versioning** makes it impossible to create unversioned routes:

```rust
let routes = VersionedApiBuilder::new()
    .with_base_path("/api")
    .add_version(ApiVersion::V1, |router| {
        router.route("/users", get(get_users_v1))
    })
    .add_version(ApiVersion::V2, |router| {
        router.route("/users", get(get_users_v2))  // Breaking change? New version!
    })
    .build_routes();
// Returns opaque type `VersionedRoutes` - can't add unversioned routes
```

### What is an Opaque Type?

An opaque type is a type whose internal structure is hidden from you. You can create it and pass it around, but you can't inspect or modify it:

```rust
// You can create VersionedRoutes
let routes: VersionedRoutes = builder.build_routes();

// You can pass it to ServiceBuilder
ServiceBuilder::new().with_routes(routes)

// But you CAN'T:
// - Add more routes to it
// - Extract routes from it
// - Merge with unversioned Router
// - Bypass versioning in any way
```

This compile-time enforcement prevents the common mistake of forgetting to version new endpoints.

---

## Observability: The Three Pillars

Observability means understanding your system's internal state by examining outputs. Three pillars work together:

### 1. Logging (What Happened)

**Structured logs** in JSON format with context:

```json
{
  "timestamp": "2024-01-15T10:30:45Z",
  "level": "INFO",
  "message": "Request processed",
  "request_id": "req_abc123",
  "trace_id": "trace_xyz789",
  "path": "/api/v1/users",
  "method": "GET",
  "status": 200,
  "latency_ms": 45
}
```

**Automatic in acton-service** - every request logged with correlation IDs.

### 2. Metrics (How Much / How Fast)

**Numerical measurements** over time:
- Request rate (requests/second)
- Error rate (errors/second)
- Latency (p50, p95, p99 percentiles)
- Resource usage (CPU, memory)

**Automatic in acton-service** - HTTP metrics collected via OpenTelemetry.

### 3. Distributed Tracing (Where Did It Go)

**Follows requests** across services:

```
[API Gateway] →  [Auth Service] →  [User Service] →  [Database]
   span1            span2             span3            span4
       └───────── Single Trace (trace_xyz789) ──────────┘
```

**Automatic in acton-service** - traces generated and propagated via OTLP.

### How They Work Together

```
User reports: "Request abc123 failed"

1. Search logs for request_id=abc123
   → Find trace_id=xyz789

2. View trace xyz789 in Jaeger
   → See it called Auth Service, then User Service, failed at Database

3. Check metrics for Database service
   → Circuit breaker opened at 10:30 AM

4. Search logs for Database errors around 10:30 AM
   → Connection pool exhausted

Root cause: Database connection pool too small
```

---

## Health vs Readiness

A common source of confusion in Kubernetes deployments:

### Health (`/health`) - Liveness Probe

**Question:** "Is the service alive?"

**Returns 200 if:**
- Service process is running
- HTTP server is responding

**Kubernetes action if failing:**
- Restart the pod (it's dead)

**Use for:**
- Detecting crashed processes
- Detecting deadlocks

### Readiness (`/ready`) - Readiness Probe

**Question:** "Is the service ready to handle traffic?"

**Returns 200 if:**
- Service is alive AND
- All required dependencies are healthy (database connected, redis available, etc.)

**Returns 503 if:**
- Service is alive but dependencies are down

**Kubernetes action if failing:**
- Remove from load balancer (don't send traffic)
- Don't restart (it's alive, just not ready)

**Use for:**
- Graceful startups (warming caches)
- Dependency outages
- Maintenance mode

### Example Scenario

```
Database goes down:
- `/health` → 200 (service is alive)
- `/ready` → 503 (database dependency failed)

Kubernetes action:
- Removes pod from load balancer
- Doesn't restart pod
- Traffic routed to healthy pods
- When database recovers, pod automatically returns to load balancer
```

---

## Connection Pooling

### The Problem

Creating database connections is expensive (hundreds of milliseconds):

```rust
// BAD: New connection per request
async fn get_user(id: i64) -> User {
    let conn = Database::connect("postgres://...").await?;  // Slow!
    let user = sqlx::query_as("SELECT * FROM users WHERE id = $1")
        .bind(id)
        .fetch_one(&conn)
        .await?;
    user
}
```

### The Solution

**Connection pool** maintains a cache of reusable connections:

```rust
// GOOD: Reuse pooled connections
async fn get_user(State(pool): State<PgPool>, id: i64) -> User {
    let user = sqlx::query_as("SELECT * FROM users WHERE id = $1")
        .bind(id)
        .fetch_one(&pool)  // Fast! Reuses existing connection
        .await?;
    user
}
```

### How It Works

1. **Startup:** Create pool of N connections (e.g., 50)
2. **Request arrives:** Check out a connection from pool
3. **Execute query:** Use the connection
4. **Request completes:** Return connection to pool for reuse
5. **If pool full:** Wait or create temporary connection (depending on config)

### Configuration

```toml
[database]
max_connections = 50    # Pool size
min_connections = 5     # Keep at least 5 warm
connection_timeout_secs = 10  # Wait up to 10s for available connection
```

**In multi-replica deployments:**

```
3 replicas × 50 connections each = 150 total connections to database
Make sure your database can handle it!
```

---

## Resilience Patterns: When to Use Which

Three resilience patterns solve different problems:

### Circuit Breaker

**Problem:** Dependency is failing, overwhelming it makes it worse

**Solution:** Detect failures, fail fast to let dependency recover

**When to use:**
- External API calls
- Downstream microservices
- Database during outages

**Don't use for:**
- In-memory operations
- Local file access

### Retry

**Problem:** Transient errors that might succeed if retried

**Solution:** Automatically retry with exponential backoff

**When to use:**
- Network timeouts
- Connection refused
- 503 Service Unavailable
- Database deadlocks

**Don't use for:**
- 400 Bad Request (won't fix itself)
- 401 Unauthorized (won't fix itself)
- Non-idempotent operations (POST without idempotency key)

### Bulkhead

**Problem:** Slow operations blocking all requests

**Solution:** Limit concurrent requests to prevent thread exhaustion

**When to use:**
- Expensive report generation
- External API calls with rate limits
- CPU-intensive operations
- File uploads/downloads

**Don't use for:**
- Fast, lightweight operations (<10ms)
- Already rate-limited endpoints

### Using All Three Together

```rust
ResilienceConfig::new()
    .with_circuit_breaker(true)  // Detect failures
    .with_retry(true)             // Handle transient errors
    .with_bulkhead(true)          // Prevent resource exhaustion
```

Execution order:
1. **Bulkhead** - Check if capacity available
2. **Circuit Breaker** - Fail fast if dependency is down
3. **Retry** - Retry if request failed
4. **Request** - Execute actual handler

---

## Next Steps

Now that you understand the core concepts, explore specific features:

- **[API Versioning](/docs/api-versioning)** - Deep dive into type-enforced versioning
- **[Resilience Patterns](/docs/resilience)** - Circuit breakers, retries, bulkheads in detail
- **[Observability](/docs/observability)** - Complete tracing, metrics, logging setup
- **[Middleware](/docs/middleware)** - Available middleware and how to use them
- **[Glossary](/docs/glossary)** - Quick reference for all technical terms
