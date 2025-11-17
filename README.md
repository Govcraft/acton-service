# acton-service

**Production-grade Rust microservice framework for teams shipping to production**

Build microservices with enforced best practices, dual HTTP+gRPC support, and comprehensive observability out of the box.

**📚 [Full Documentation](https://govcraft.github.io/acton-service/)** | [Quick Start](#quick-start) | [Examples](./acton-service/examples/)

---

## What is this?

Building production microservices requires solving the same problems repeatedly: API versioning, health checks, observability, resilience patterns, connection pooling, and configuration management. Most frameworks leave these as optional concerns or implementation details.

acton-service provides a **batteries-included, type-enforced framework** where production best practices are the default path:

- **Type-enforced API versioning** - Impossible to bypass, compiler-enforced versioning
- **Dual HTTP + gRPC** - Run both protocols on the same port with automatic detection
- **Cedar policy-based authorization** - AWS Cedar integration for fine-grained access control
- **Production observability** - OpenTelemetry tracing, metrics, and structured logging built-in
- **Resilience patterns** - Circuit breaker, retry logic, and bulkhead patterns included
- **Zero-config defaults** - XDG-compliant configuration with sensible production defaults
- **Kubernetes-ready** - Automatic health/readiness probes for orchestration

It's opinionated, comprehensive, and designed for teams where best practices can't be optional.

**Current Status**: acton-service is under active development. Core features (HTTP/gRPC, versioning, health checks, observability, resilience) are production-ready. Some advanced features are in progress.

---

**🚀 New to acton-service?** Start with the **[5-Minute Quickstart](https://govcraft.github.io/acton-service/docs/quickstart)** or follow the **[Complete Tutorial](https://govcraft.github.io/acton-service/docs/tutorial)** to build your first service.

---

## Quick Start

```rust
use acton_service::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Build versioned API routes - versioning enforced by type system
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/hello", get(|| async { "Hello, V1!" }))
        })
        .add_version(ApiVersion::V2, |router| {
            router.route("/hello", get(|| async { "Hello, V2!" }))
        })
        .build_routes();

    // Zero-config service startup with automatic features:
    // - Health/readiness endpoints (/health, /ready)
    // - OpenTelemetry tracing and metrics
    // - Structured JSON logging
    // - Request tracking and correlation IDs
    // - Configuration from environment or files
    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await
}
```

```bash
cargo run

# Versioned API endpoints
curl http://localhost:8080/api/v1/hello
curl http://localhost:8080/api/v2/hello

# Automatic health checks (Kubernetes-ready)
curl http://localhost:8080/health
curl http://localhost:8080/ready

# OpenTelemetry metrics automatically collected
# Structured logs in JSON format automatically emitted
# Request IDs automatically generated and tracked
```

**The type system enforces best practices:**

```rust
// ❌ This won't compile - unversioned routes rejected at compile time
let app = Router::new().route("/unversioned", get(handler));
ServiceBuilder::new().with_routes(app).build();
//                                   ^^^ expected VersionedRoutes, found Router
```

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
acton-service = { version = "0.2", features = ["http", "observability"] }
tokio = { version = "1", features = ["full"] }
```

Or use the CLI to scaffold a complete service:

```bash
cargo install acton-cli
acton service new my-api --yes
cd my-api && cargo run
```

## Why acton-service?

### The Problem

Building production microservices requires solving the same problems over and over:

- **Dual Protocols**: Modern deployments need both HTTP REST APIs and gRPC, but most frameworks make you choose one or run two separate servers
- **Observability**: Distributed tracing, metrics collection, and structured logging should be standard, not afterthoughts assembled from scattered libraries
- **Resilience Patterns**: Circuit breakers, retries, and bulkheads are critical for production but tedious to implement correctly
- **Health Checks**: Every orchestrator needs them, but every team implements them differently with varying quality
- **API Evolution**: Breaking changes slip through because versioning is optional and easily forgotten
- **Configuration**: Production deployments need environment-based config without requiring boilerplate for every service

### The Solution

acton-service provides a **comprehensive, opinionated framework** where production concerns are handled by default:

1. **Dual HTTP + gRPC** - Run both protocols on the same port with automatic protocol detection, or use separate ports ✅
2. **Complete observability stack** - OpenTelemetry tracing, HTTP metrics, and structured JSON logging configured out of the box ✅
3. **Production resilience patterns** - Circuit breaker, exponential backoff retry, and bulkhead middleware included ✅
4. **Automatic health endpoints** - Kubernetes-ready liveness and readiness probes with dependency monitoring ✅
5. **Type-enforced API versioning** - The compiler prevents unversioned APIs; impossible to bypass ✅
6. **Zero-config defaults** - XDG-compliant configuration with sensible defaults and environment variable overrides ✅
7. **Batteries-included middleware** - JWT auth, Cedar policy-based authorization, rate limiting, request tracking, compression, CORS, timeouts ✅
8. **Connection pool management** - PostgreSQL, Redis, and NATS support with automatic retry and health checks ✅

Most importantly: **it's designed for teams**. The type system enforces best practices that individual contributors can't accidentally bypass.

## Core Features

acton-service provides a comprehensive set of production-ready features that work together seamlessly:

### Type-Safe API Versioning

The framework enforces API versioning at compile time through the type system:

```rust
// Define your API versions
let routes = VersionedApiBuilder::new()
    .with_base_path("/api")
    .add_version_deprecated(
        ApiVersion::V1,
        |router| router.route("/users", get(list_users_v1)),
        DeprecationInfo::new(ApiVersion::V1, ApiVersion::V2)
            .with_sunset_date("2026-12-31T23:59:59Z")
            .with_message("V1 is deprecated. Migrate to V2.")
    )
    .add_version(ApiVersion::V2, |router| {
        router.route("/users", get(list_users_v2))
    })
    .build_routes();
```

Deprecated versions automatically include `Deprecation`, `Sunset`, and `Link` headers per [RFC 8594](https://datatracker.ietf.org/doc/html/rfc8594).

### Automatic Health Checks

Health and readiness endpoints are included automatically and follow Kubernetes best practices:

```rust
// Health checks are automatic - no code needed
ServiceBuilder::new()
    .with_routes(routes)
    .build()
    .serve()
    .await?;

// Endpoints available immediately:
// GET /health    - Liveness probe (process alive?)
// GET /ready     - Readiness probe (dependencies healthy?)
```

The readiness endpoint automatically checks configured dependencies:

```toml
# config.toml
[database]
url = "postgres://localhost/mydb"
optional = false  # Readiness fails if DB is down

[redis]
url = "redis://localhost"
optional = true   # Readiness succeeds even if Redis is down
```

### Batteries-Included Middleware

Production-ready middleware stack with comprehensive coverage:

```rust
ServiceBuilder::new()
    .with_routes(routes)
    .with_middleware(|router| {
        router
            .layer(JwtAuth::new("your-secret"))
            .layer(RequestTrackingConfig::default().layer())
            .layer(RateLimitLayer::new(100, Duration::from_secs(60)))
            .layer(ResilienceLayer::new()
                .with_circuit_breaker(0.5)  // 50% failure threshold
                .with_retry(3)               // max 3 retries
                .with_bulkhead(100))         // max 100 concurrent requests
    })
    .build()
    .serve()
    .await?;
```

Available middleware (all HTTP and gRPC compatible):

**Authentication & Authorization**
- **JWT Authentication** - Full validation with RS256, ES256, HS256/384/512 algorithms
- **Cedar Policy-Based Authorization** - AWS Cedar integration for fine-grained access control with:
  - Declarative policy files for resource-based permissions
  - Role-based and attribute-based access control (RBAC/ABAC)
  - Manual policy reload endpoint (automatic hot-reload in progress)
  - Optional Redis caching for sub-5ms policy decisions
  - HTTP and gRPC support with customizable path normalization
- Claims structure with roles, permissions, user/client identification
- Redis-backed token revocation (optional)

**Resilience & Reliability**
- **Circuit Breaker** - Configurable failure rate monitoring with auto-recovery
- **Retry Logic** - Exponential backoff with configurable max attempts
- **Bulkhead** - Concurrency limiting with wait timeouts to prevent overload

**Rate Limiting**
- **Redis-backed rate limiting** - Distributed rate limiting for multi-instance deployments
- **Governor rate limiting** - Local in-memory limiting with per-second/minute/hour presets
- Per-user and per-client limits via JWT claims

**Observability**
- **Request Tracking** - UUID-based request ID generation and propagation
- **Distributed Tracing Headers** - x-request-id, x-trace-id, x-span-id, x-correlation-id
- **OpenTelemetry Metrics** - HTTP request count, duration histograms, active requests, sizes
- **Sensitive Header Masking** - Automatic masking in logs (authorization, cookies, API keys)

**Standard HTTP Middleware**
- **Compression** - gzip, br, deflate, zstd content encoding
- **CORS** - Configurable cross-origin policies
- **Timeouts** - Configurable request timeouts
- **Body Size Limits** - Prevent oversized payloads
- **Panic Recovery** - Graceful handling of panics with error logging

### HTTP + gRPC Support

Run HTTP and gRPC services together on a single port:

```rust
// HTTP handlers
let http_routes = VersionedApiBuilder::new()
    .add_version(ApiVersion::V1, |router| {
        router.route("/users", get(list_users))
    })
    .build_routes();

// gRPC service
#[derive(Default)]
struct MyGrpcService;

#[tonic::async_trait]
impl my_service::MyService for MyGrpcService {
    async fn my_method(&self, req: Request<MyRequest>)
        -> Result<Response<MyResponse>, Status> {
        // ...
    }
}

// Serve both protocols on the same port (automatic protocol detection)
ServiceBuilder::new()
    .with_routes(http_routes)
    .with_grpc_service(my_service::MyServiceServer::new(MyGrpcService))
    .build()
    .serve()
    .await?;
```

Configure gRPC in `config.toml`:

```toml
[grpc]
enabled = true
use_separate_port = false  # Default: single-port mode with automatic protocol detection
# port = 9090              # Only used when use_separate_port = true
```

### Zero-Configuration Defaults

Configuration follows the [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/basedir-spec-latest.html):

```
~/.config/acton-service/
├── my-service/
│   └── config.toml
├── auth-service/
│   └── config.toml
└── user-service/
    └── config.toml
```

Services load configuration automatically with environment variable overrides:

```bash
# No config file needed for development
cargo run

# Override specific values
ACTON_SERVICE_PORT=9090 cargo run

# Production config location
~/.config/acton-service/my-service/config.toml
```

## Feature Flags

Enable only what you need:

```toml
[dependencies]
acton-service = { version = "0.2", features = [
    "http",          # Axum HTTP framework (default)
    "grpc",          # Tonic gRPC support
    "database",      # PostgreSQL via SQLx
    "cache",         # Redis connection pooling
    "events",        # NATS JetStream
    "observability", # Structured logging (default)
    "governor",      # Advanced rate limiting
    "openapi",       # Swagger/OpenAPI documentation
    "cedar-authz",   # AWS Cedar policy-based authorization
] }
```

**Note**: All major feature flags are implemented. Some CLI commands (like advanced endpoint generation) are still in progress. See the roadmap below.

Or use `full` to enable everything:

```toml
[dependencies]
acton-service = { version = "0.2", features = ["full"] }
```

## Examples

### Minimal HTTP Service

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

### Production Service with Database

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

### Event-Driven Service

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

See the [`examples/`](./acton-service/examples) directory for complete examples including:

- Simple versioned API - [`simple-api.rs`](./acton-service/examples/simple-api.rs)
- User management API with deprecation - [`users-api.rs`](./acton-service/examples/users-api.rs)
- Dual-protocol HTTP + gRPC - [`ping-pong.rs`](./acton-service/examples/ping-pong.rs)
- Event-driven architecture - [`event-driven.rs`](./acton-service/examples/event-driven.rs)
- Cedar policy-based authorization - [`cedar-authz.rs`](./acton-service/examples/cedar-authz.rs) | [Guide](./acton-service/examples/CEDAR_EXAMPLE_README.md)

Run examples:

```bash
cargo run --example simple-api
cargo run --example users-api
cargo run --example ping-pong --features grpc
cargo run --example event-driven --features grpc
cargo run --example cedar-authz --features cedar-authz,cache
```

## CLI Tool

The `acton` CLI scaffolds production-ready services:

```bash
# Install the CLI
cargo install acton-cli

# Create a new service
acton service new my-api --yes

# Full-featured service
acton service new user-service \
    --http \
    --database postgres \
    --cache redis \
    --events nats \
    --observability

# Add endpoints to existing service
cd user-service
acton service add endpoint POST /users --handler create_user
acton service add worker email-worker --source nats --stream emails

# Generate Kubernetes manifests
acton service generate deployment --hpa --monitoring
```

See the [CLI documentation](./acton-cli/README.md) for details.

## Architecture

acton-service is built on production-proven Rust libraries:

- **HTTP**: [axum](https://github.com/tokio-rs/axum) - Ergonomic web framework
- **gRPC**: [tonic](https://github.com/hyperium/tonic) - Native Rust gRPC
- **Database**: [SQLx](https://github.com/launchbadge/sqlx) - Compile-time checked queries
- **Cache**: [redis-rs](https://github.com/redis-rs/redis-rs) - Redis client
- **Events**: [async-nats](https://github.com/nats-io/nats.rs) - NATS client
- **Observability**: [OpenTelemetry](https://github.com/open-telemetry/opentelemetry-rust) - Distributed tracing

Design principles:

1. **Type safety over runtime checks** - Use the compiler to prevent mistakes
2. **Opinionated defaults** - Best practices should be the default path
3. **Explicit over implicit** - No magic, clear code flow
4. **Production-ready by default** - Health checks, config, observability included
5. **Modular features** - Only compile what you need

## Documentation

📚 **[Full Documentation Site](https://govcraft.github.io/acton-service/)** - Complete guides, API reference, and examples

### Getting Started

- **[Quickstart](https://govcraft.github.io/acton-service/docs/quickstart)** - Get a service running in 5 minutes
- **[Tutorial](https://govcraft.github.io/acton-service/docs/tutorial)** - Complete step-by-step guide to building a production service
- **[Installation](https://govcraft.github.io/acton-service/docs/installation)** - Setup and feature selection
- **[Feature Flags](https://govcraft.github.io/acton-service/docs/feature-flags)** - Decision tree for choosing the right features
- **[Comparison](https://govcraft.github.io/acton-service/docs/comparison)** - How acton-service compares to Axum, Actix-Web, and others

### Guides

- **[Configuration](https://govcraft.github.io/acton-service/docs/configuration)** - Environment and file-based configuration
- **[API Versioning](https://govcraft.github.io/acton-service/docs/api-versioning)** - Type-safe versioning patterns
- **[Health Checks](https://govcraft.github.io/acton-service/docs/health-checks)** - Kubernetes liveness and readiness
- **[Database](https://govcraft.github.io/acton-service/docs/database)** - PostgreSQL integration with SQLx
- **[JWT Authentication](https://govcraft.github.io/acton-service/docs/jwt-auth)** - Authentication patterns
- **[Cedar Authorization](https://govcraft.github.io/acton-service/docs/cedar-auth)** - Policy-based access control
- **[Observability](https://govcraft.github.io/acton-service/docs/observability)** - Metrics, tracing, and logging
- **[Production Deployment](https://govcraft.github.io/acton-service/docs/production)** - Production best practices

### Reference

- **[Examples](https://govcraft.github.io/acton-service/docs/examples)** - Complete working examples
- **[Troubleshooting](https://govcraft.github.io/acton-service/docs/troubleshooting)** - Common issues and solutions
- **[FAQ](https://govcraft.github.io/acton-service/docs/faq)** - Frequently asked questions
- **[Glossary](https://govcraft.github.io/acton-service/docs/glossary)** - Technical term definitions
- **API Documentation**: `cargo doc --open`

## Performance

acton-service is built on tokio and axum, which are known for excellent performance characteristics. The framework adds minimal abstraction overhead beyond the underlying libraries.

**Performance benchmarks will be published as the project matures.** Performance is primarily determined by your application logic and the underlying libraries (axum for HTTP, tonic for gRPC, sqlx for database operations).

## Deployment

### Docker

```dockerfile
FROM rust:1.84-slim as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/my-service /usr/local/bin/
EXPOSE 8080
CMD ["my-service"]
```

### Kubernetes

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: my-service
spec:
  replicas: 3
  selector:
    matchLabels:
      app: my-service
  template:
    metadata:
      labels:
        app: my-service
    spec:
      containers:
      - name: my-service
        image: my-service:latest
        ports:
        - containerPort: 8080
        env:
        - name: ACTON_SERVICE_PORT
          value: "8080"
        - name: ACTON_DATABASE_URL
          valueFrom:
            secretKeyRef:
              name: db-credentials
              key: url
        livenessProbe:
          httpGet:
            path: /health
            port: 8080
          initialDelaySeconds: 30
          periodSeconds: 10
        readinessProbe:
          httpGet:
            path: /ready
            port: 8080
          initialDelaySeconds: 5
          periodSeconds: 5
```

Generate complete Kubernetes manifests with the CLI:

```bash
acton service generate deployment --hpa --monitoring --ingress
```

## Migration Guide

### From Axum

acton-service is a thin layer over axum. Your existing handlers work unchanged:

```rust
// Your existing axum handler
async fn handler(State(state): State<MyState>, Json(body): Json<Request>)
    -> Result<Json<Response>, StatusCode> {
    // ...
}

// Works directly in acton-service
let routes = VersionedApiBuilder::new()
    .add_version(ApiVersion::V1, |router| {
        router.route("/endpoint", post(handler))
    })
    .build_routes();
```

Main changes:

1. Routes must be versioned (wrap in `VersionedApiBuilder`)
2. Use `ServiceBuilder` instead of `axum::serve()`
3. Configuration loaded automatically (optional)

### From Actix-Web

Similar handler patterns, different framework:

```rust
// Actix-web
#[post("/users")]
async fn create_user(user: web::Json<User>) -> impl Responder {
    HttpResponse::Created().json(user)
}

// acton-service
async fn create_user(Json(user): Json<User>) -> impl IntoResponse {
    (StatusCode::CREATED, Json(user))
}
```

See the [examples directory](./acton-service/examples/) for complete migration examples.

## Roadmap

**Implemented** ✅
- **Dual Protocol Support**: Single-port HTTP + gRPC multiplexing with automatic protocol detection
- **Complete Observability Stack**: OpenTelemetry tracing/metrics (OTLP exporter), structured JSON logging, distributed request tracing
- **Production Resilience Patterns**: Circuit breaker, exponential backoff retry, bulkhead (concurrency limiting)
- **Comprehensive Middleware**: JWT authentication (RS256/ES256/HS256/384/512), Redis-backed distributed rate limiting, local governor rate limiting, request tracking with correlation IDs, OpenTelemetry metrics middleware
- **Cedar Policy-Based Authorization**: AWS Cedar integration with declarative policies, manual reload endpoint, Redis caching, HTTP/gRPC support, customizable path normalization (automatic hot-reload in progress)
- **Type-Enforced API Versioning**: Compile-time enforcement with RFC 8594 deprecation headers
- **Automatic Health Checks**: Kubernetes-ready liveness/readiness probes with dependency monitoring (database, cache, events)
- **Connection Pool Management**: PostgreSQL (SQLx), Redis (Deadpool), NATS JetStream with automatic retry and health checks
- **XDG-Compliant Configuration**: Multi-source config with environment variable overrides and sensible defaults
- **OpenAPI/Swagger Support**: Multiple UI options (Swagger UI, RapiDoc, ReDoc) with multi-version documentation
- **CLI Scaffolding Tool**: Service generation with configurable features (database, cache, events, observability)
- **gRPC Features**: Reflection service, health checks, interceptors, middleware parity with HTTP

**In Progress** 🚧
- Enhanced CLI commands (add endpoint, worker generation, deployment manifest creation)
- Additional OpenAPI schema generation utilities

**Planned** 📋
- GraphQL support with versioning integration
- WebSocket support for real-time features
- Service mesh integration (Istio, Linkerd)
- Additional database backends (MySQL, MongoDB)
- Observability dashboards and sample configurations
- Enhanced metrics (custom business metrics, SLO tracking)
- Advanced rate limiting strategies (sliding log, token bucket refinements)

## FAQ

**Q: How does this compare to using Axum or Tonic directly?**

A: acton-service is built on top of Axum (HTTP) and Tonic (gRPC) but adds production-ready features as defaults: type-enforced versioning, automatic health checks, observability stack, resilience patterns, and connection pool management. If you need maximum flexibility, use the underlying libraries directly. If you want production best practices enforced by the type system, use acton-service.

**Q: Can I run both HTTP and gRPC on the same port?**

A: Yes! This is a core feature. acton-service provides automatic protocol detection allowing HTTP and gRPC to share a single port, or you can configure separate ports if preferred.

**Q: Does this work with existing axum middleware?**

A: Yes. All Tower middleware works unchanged. Use `.layer()` with any tower middleware. The framework includes comprehensive middleware for JWT auth, rate limiting, resilience patterns, request tracking, and metrics.

**Q: Why enforce versioning so strictly?**

A: API versioning is critical in production but easy to skip when deadlines loom. Making it impossible to bypass via the type system ensures consistent team practices and prevents breaking changes from slipping through.

**Q: Can I use this without the enforced versioning?**

A: No. If you need unversioned routes, use axum directly. acton-service is opinionated about API evolution and production best practices.

**Q: Is this production-ready?**

A: Core features are production-ready: HTTP/gRPC servers, type-enforced versioning, health checks, observability (OpenTelemetry tracing/metrics), resilience patterns (circuit breaker, retry, bulkhead), middleware stack, and connection pooling (PostgreSQL, Redis, NATS). The framework is built on battle-tested libraries (axum, tonic, sqlx). Some advanced CLI features are in progress. Review the roadmap and test thoroughly for your use case.

**Q: What's the performance overhead?**

A: Minimal. The framework is a thin abstraction layer over high-performance libraries (tokio, axum, tonic). The type-enforced patterns are compile-time checks with zero runtime cost. Middleware like circuit breakers and metrics add small overhead for production safety benefits.

## Contributing

Contributions are welcome! Areas of focus:

- Additional middleware patterns
- More comprehensive examples
- Documentation improvements
- Performance optimizations
- CLI enhancements

See [`CONTRIBUTING.md`](./CONTRIBUTING.md) for guidelines (coming soon).

## Changelog

See [`CHANGELOG.md`](./CHANGELOG.md) for version history (coming soon).

## License

Licensed under the MIT License. See [`LICENSE`](./LICENSE) for details.

## Credits

Built with excellent open source libraries:

- [tokio](https://tokio.rs) - Async runtime
- [axum](https://github.com/tokio-rs/axum) - Web framework
- [tonic](https://github.com/hyperium/tonic) - gRPC implementation
- [tower](https://github.com/tower-rs/tower) - Middleware foundation
- [SQLx](https://github.com/launchbadge/sqlx) - Database client

Inspired by production challenges at scale. Built by developers who've maintained microservice architectures in production.

---

**Start building production microservices with enforced best practices:**

```bash
cargo install acton-cli
acton service new my-api --yes
cd my-api && cargo run
```
