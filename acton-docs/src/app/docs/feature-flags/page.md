---
title: Feature Flags
nextjs:
  metadata:
    title: Feature Flags
    description: Choose the right feature flags to keep compile times fast and binaries small
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


acton-service uses feature flags to keep compile times fast and binary sizes small. Enable only what you need.

## Quick Decision Tree

```text
┌─────────────────────────────────────────┐
│ What are you building?                  │
└─────────────────────────────────────────┘
                    │
        ┌───────────┴───────────┐
        │                       │
   REST API                gRPC Service
        │                       │
        ▼                       ▼
   ["http",              ["grpc",
    "observability"]      "observability"]
        │                       │
        ├───────────────────────┤
        │
        ▼
┌─────────────────────────────────────────┐
│ Do you need real-time communication?    │
└─────────────────────────────────────────┘
        │
        ├─── Yes ──▶ Add "websocket"
        └─── No  ──▶ Skip
        │
        ▼
┌─────────────────────────────────────────┐
│ Do you need a database?                 │
└─────────────────────────────────────────┘
        │
        ├─── Yes ──▶ Add "database"
        └─── No  ──▶ Skip
        │
        ▼
┌─────────────────────────────────────────┐
│ Do you need caching?                    │
└─────────────────────────────────────────┘
        │
        ├─── Yes ──▶ Add "cache"
        └─── No  ──▶ Skip
        │
        ▼
┌─────────────────────────────────────────┐
│ Do you need events/messaging?           │
└─────────────────────────────────────────┘
        │
        ├─── Yes ──▶ Add "events"
        └─── No  ──▶ Skip
        │
        ▼
┌─────────────────────────────────────────┐
│ Do you need HTTP sessions (HTMX/SSR)?   │
└─────────────────────────────────────────┘
        │
        ├─── Dev ──▶ Add "session-memory"
        └─── Prod ─▶ Add "session-redis"
        │
        ▼
┌─────────────────────────────────────────┐
│ Do you need advanced features?          │
└─────────────────────────────────────────┘
        │
        ├─── Fine-grained authorization ──▶ Add "cedar-authz"
        ├─── Rate limiting ───────────────▶ Add "governor"
        ├─── Resilience ──────────────────▶ Add "resilience"
        ├─── Metrics ─────────────────────▶ Add "otel-metrics"
        └─── OpenAPI ─────────────────────▶ Add "openapi"
```

---

## Core Features

### `http`
**Included in default features**

Enables HTTP REST API support via Axum.

**When to use**: Building REST APIs (most common use case)

**Dependencies**: Axum, Tower

```toml
{% $dep.httpOnly %}
```

### `observability`
**Included in default features**

Enables structured logging and OpenTelemetry tracing.

**When to use**: Always (highly recommended for production)

**Dependencies**: tracing, tracing-subscriber, OpenTelemetry

```toml
{% dep("observability") %}
```

---

## Protocol Features

### `grpc`

Enables gRPC support via Tonic. Can run on the same port as HTTP with automatic protocol detection.

**When to use**: Building gRPC services or dual HTTP+gRPC services

**Dependencies**: tonic, prost

```toml
{% dep("grpcOnly") %}
```

### `websocket`

Enables WebSocket support for real-time bidirectional communication. Uses Axum's built-in WebSocket support.

**When to use**: Building real-time applications (chat, live updates, gaming)

**Dependencies**: None (uses axum's ws feature)

**Provides**:
- WebSocket upgrade handlers
- Connection management with unique IDs
- Broadcaster for message distribution
- Actor-based room management

```toml
{% dep("websocketOnly") %}
```

See the [WebSocket Guide](/docs/websocket) for detailed usage.

---

## Data Layer Features

### `database`

PostgreSQL connection pooling via SQLx with automatic health checks and retry logic.

**When to use**: Your service needs a SQL database

**Dependencies**: sqlx with postgres feature

**Provides**:
- Automatic connection pool management
- Health checks for database connections
- Retry logic on connection failures

```toml
{% dep("databaseOnly") %}
```

### `turso`

Turso/libsql database support for edge-friendly SQLite with cloud sync capabilities.

**When to use**: Building edge applications, mobile backends, or need SQLite with cloud durability

**Dependencies**: libsql

**Provides**:
- Local, Remote, and EmbeddedReplica connection modes
- Automatic retry with exponential backoff
- Optional encryption (AES-256-CBC)
- Background sync for embedded replicas

```toml
{% dep("tursoOnly") %}
```

See the [Turso Guide](/docs/turso) for detailed usage.

### `cache`

Redis connection pooling with support for JWT token revocation and distributed rate limiting.

**When to use**: Need caching, session storage, or rate limiting

**Dependencies**: redis, deadpool-redis

**Provides**:
- Redis connection pool
- JWT token revocation support
- Distributed rate limiting

```toml
{% dep("cacheOnly") %}
```

### `events`

NATS JetStream client for event-driven architecture and pub/sub messaging.

**When to use**: Building event-driven microservices

**Dependencies**: async-nats

**Provides**:
- NATS connection management
- JetStream support
- Pub/sub messaging

```toml
{% dep("eventsOnly") %}
```

---

## Session Features

### `session`

Base session support. This feature is automatically included when using `session-memory` or `session-redis`.

**When to use**: Building HTMX or server-rendered applications with session state

**Dependencies**: tower-sessions, time

### `session-memory`

In-memory session storage for development and single-instance deployments.

**When to use**: Local development, testing, or single-server applications

**Dependencies**: tower-sessions-memory-store

**Provides**:
- In-memory session store
- Cookie-based session IDs
- Flash messages
- CSRF protection
- TypedSession for type-safe session data

```toml
acton-service = { version = "{% version() %}", features = ["session-memory"] }
```

### `session-redis`

Redis-backed session storage for production multi-instance deployments.

**When to use**: Production deployments with multiple application instances

**Dependencies**: tower-sessions-redis-store (fred)

**Provides**:
- Distributed session storage
- Session persistence across restarts
- All features from `session-memory`

```toml
acton-service = { version = "{% version() %}", features = ["session-redis"] }
```

**Note**: Uses `fred` Redis client internally (separate from `cache` feature's `deadpool-redis`).

See the [Session Management Guide](/docs/session) for detailed usage.

---

## Middleware & Resilience Features

### `cedar-authz`

AWS Cedar policy-based authorization for fine-grained access control.

**When to use**: Need fine-grained access control with declarative policies

**Dependencies**: cedar-policy

**Provides**:
- Declarative Cedar policy files for resource-based permissions
- Role-based and attribute-based access control (RBAC/ABAC)
- Manual policy reload endpoint (automatic hot-reload in progress)
- Optional Redis caching for sub-5ms policy decisions
- HTTP and gRPC support with customizable path normalization
- Layered security with JWT authentication

```toml
{% dep("cedarAuthz") %}
```

**Note**: Works best with `cache` feature for policy decision caching.

### `resilience`

Circuit breaker, retry, and bulkhead patterns for production services.

**When to use**: Production services calling external dependencies

**Dependencies**: tower-resilience

**Provides**:
- Circuit breaker (prevent cascading failures)
- Exponential backoff retry
- Bulkhead (concurrency limiting)

```toml
{% dep("resilience") %}
```

### `governor`

Advanced rate limiting with per-user limits via JWT claims.

**When to use**: Need sophisticated rate limiting beyond basic throttling

**Dependencies**: tower_governor

**Provides**:
- Per-second/minute/hour rate limits
- Per-user rate limiting via JWT claims
- In-memory rate limiting

```toml
{% dep("governor") %}
```

### `otel-metrics`

HTTP metrics collection via OpenTelemetry for detailed monitoring.

**When to use**: Need detailed request metrics for monitoring

**Dependencies**: tower-otel-http-metrics

**Provides**:
- Request count, duration, size metrics
- Active request tracking
- HTTP status code distribution

```toml
{% dep("otelMetrics") %}
```

---

## Documentation Features

### `openapi`

OpenAPI/Swagger documentation generation with multiple UI options.

**When to use**: Need API documentation UI

**Dependencies**: utoipa, utoipa-swagger-ui

**Provides**:
- Swagger UI
- ReDoc UI
- RapiDoc UI
- Auto-generated OpenAPI specs

```toml
{% dep("openapiOnly") %}
```

---

## Common Configurations

### Minimal REST API
**Use case**: Simple REST API, no database

```toml
[dependencies]
{% $dep.http %}
tokio = { version = "1", features = ["full"] }
```

**Binary size**: ~10MB (stripped)
**Compile time**: ~30s (clean build)

### REST API with Database
**Use case**: Standard CRUD API with PostgreSQL

```toml
[dependencies]
{% $dep.database %}
tokio = { version = "1", features = ["full"] }
```

**Binary size**: ~12MB (stripped)
**Compile time**: ~45s (clean build)

### Full-Featured REST API
**Use case**: Production API with all bells and whistles

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = [
    "http",
    "observability",
    "database",
    "cache",
    "resilience",
    "governor",
    "otel-metrics",
    "openapi"
] }
tokio = { version = "1", features = ["full"] }
```

**Binary size**: ~18MB (stripped)
**Compile time**: ~90s (clean build)

### REST API with Cedar Authorization
**Use case**: Secure API with fine-grained policy-based access control

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = [
    "http",
    "observability",
    "database",
    "cache",           # Required for Cedar policy caching
    "cedar-authz",     # Policy-based authorization
    "resilience"
] }
tokio = { version = "1", features = ["full"] }
```

**Binary size**: ~16MB (stripped)
**Compile time**: ~75s (clean build)

### Dual HTTP + gRPC Service
**Use case**: Service exposing both REST and gRPC APIs

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = [
    "http",
    "grpc",
    "observability",
    "database"
] }
tokio = { version = "1", features = ["full"] }
```

**Binary size**: ~15MB (stripped)
**Compile time**: ~60s (clean build)

### Event-Driven Microservice
**Use case**: Background worker processing NATS events

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = [
    "http",           # For health endpoints
    "observability",
    "events",         # NATS support
    "database",
    "cache"
] }
tokio = { version = "1", features = ["full"] }
```

**Binary size**: ~14MB (stripped)
**Compile time**: ~55s (clean build)

### HTMX / Server-Rendered App
**Use case**: Traditional web app with server-rendered HTML and HTMX

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = [
    "http",
    "observability",
    "database",
    "session-memory"   # Use "session-redis" in production
] }
tokio = { version = "1", features = ["full"] }
```

**Binary size**: ~13MB (stripped)
**Compile time**: ~50s (clean build)

**Provides**: Cookie-based sessions, flash messages, CSRF protection, TypedSession.

### Everything (Development/Prototyping)
**Use case**: Exploring all features, quick prototyping

```toml
[dependencies]
{% $dep.full %}
tokio = { version = "1", features = ["full"] }
```

**Binary size**: ~20MB (stripped)
**Compile time**: ~120s (clean build)

**⚠️ Warning**: `full` includes everything. For production, only enable what you need.

---

## Feature Dependencies

Some features work better together:

| Feature | Recommended Companions | Why |
|---------|----------------------|-----|
| `cedar-authz` | `cache` | Policy decision caching dramatically improves performance (10-50ms → 1-5ms) |
| `cache` | `governor` | Distributed rate limiting needs Redis |
| `otel-metrics` | `observability` | Metrics require tracing foundation |
| `resilience` | `http` or `grpc` | Resilience patterns apply to HTTP/gRPC calls |
| `openapi` | `http` | OpenAPI docs are for HTTP endpoints |
| `session-redis` | Production deployments | Sessions persist across restarts and work across multiple instances |
| `session-memory` | `session-redis` | Use memory for dev, Redis for production |

---

## Troubleshooting

### "cannot find type `AppState`"
**Solution**: You're probably missing required features. Add `http` and `observability`.

### "method `database` not found"
**Solution**: Add `database` feature flag.

### "could not find `tonic` in the list"
**Solution**: Add `grpc` feature flag.

### Very slow compile times
**Solution**: You might have `full` enabled. Only enable features you actually use.

### Large binary size
**Solution**:
1. Remove unused features
2. Build with `--release`
3. Strip symbols: `strip target/release/my-service`

---

## Best Practices

### Start Small

Begin with minimal features and add as needed:

```toml
# Start here
features = ["http", "observability"]

# Add as you grow
features = ["http", "observability", "database"]

# Production-ready
features = ["http", "observability", "database", "cache", "resilience"]
```

### Production Recommendations

**Minimum for production**:
```toml
features = ["http", "observability", "resilience"]
```

**Recommended for production**:
```toml
features = [
    "http",
    "observability",
    "database",        # If you need it
    "cache",          # For sessions/rate limiting/Cedar caching
    "cedar-authz",    # Fine-grained authorization (optional)
    "resilience",     # Circuit breaker, retry
    "otel-metrics"    # Monitoring
]
```

### CI/CD Optimization

Use different feature sets for different build stages:

```yaml
# Fast CI check
cargo check --features "http,observability"

# Full integration tests
cargo test --features "http,observability,database,cache"

# Production build
cargo build --release --features "http,observability,database,cache,resilience,otel-metrics"
```

---

## Need More Help?

- [Quickstart](/docs/quickstart) - Get started in 5 minutes
- [Tutorial](/docs/tutorial) - Step-by-step service guide
- [Examples](/docs/examples) - Working examples for each feature
- [Cargo.toml](https://github.com/Govcraft/acton-service/blob/main/acton-service/Cargo.toml) - Feature definitions
