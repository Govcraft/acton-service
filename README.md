# acton-service

**Production-grade Rust microservice framework with type-enforced API versioning**

Build microservices that can't ship unversioned APIs. The compiler won't let you.

---

## What is this?

Most microservice frameworks make API versioning optional. You *should* version your APIs, they say. But when deadlines loom, versioning gets skipped. Six months later, you're maintaining breaking changes in production.

acton-service uses Rust's type system to make unversioned APIs **impossible**. Your service won't compile without proper versioning. It's opinionated, batteries-included, and designed for teams shipping to production.

**Current Status**: acton-service is under active development. Core features (HTTP, versioning, health checks, observability) are production-ready. Some advanced features are in progress.

## Quick Start

```rust
use acton_service::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Routes MUST be versioned - this is the only way to create them
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/hello", get(|| async { "Hello, V1!" }))
        })
        .add_version(ApiVersion::V2, |router| {
            router.route("/hello", get(|| async { "Hello, V2!" }))
        })
        .build_routes();

    // Zero-config service startup
    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await
}
```

```bash
cargo run
curl http://localhost:8080/api/v1/hello
curl http://localhost:8080/api/v2/hello
curl http://localhost:8080/health  # automatic health checks
```

**Try to create an unversioned route? Won't compile.**

```rust
// ❌ This won't compile
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

- **API Versioning**: Most frameworks make it optional. Teams skip it until it's too late.
- **Health Checks**: Every orchestrator needs them. Every team implements them differently.
- **Observability**: Tracing, metrics, and logging should be standard, not afterthoughts.
- **Configuration**: Environment-based config that doesn't require boilerplate.
- **Dual Protocols**: HTTP and gRPC on the same port (modern K8s deployments need both).

### The Solution

acton-service provides:

1. **Type-enforced versioning** - The compiler prevents unversioned APIs ✅
2. **Automatic health endpoints** - Kubernetes-ready liveness and readiness probes ✅
3. **Structured logging** - JSON logging with distributed request tracing ✅
4. **Zero-config defaults** - XDG-compliant configuration with sensible defaults ✅
5. **HTTP + gRPC support** - Run both protocols (currently on separate ports) ✅

Most importantly: **it's designed for teams**. Individual contributors can't accidentally break production API contracts.

## Core Features

### Type-Safe API Versioning

Routes are versioned at compile time. The type system enforces it:

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

Production-ready middleware stack included:

```rust
ServiceBuilder::new()
    .with_routes(routes)
    .with_middleware(|router| {
        router
            .layer(JwtAuth::new("your-secret"))
            .layer(RequestTrackingConfig::default().layer())
            .layer(RateLimit::new(100, Duration::from_secs(60)))
    })
    .build()
    .serve()
    .await?;
```

Available middleware:

- **JWT Authentication** - Token validation with configurable algorithms
- **Rate Limiting** - Token bucket and sliding window strategies (governor)
- **Request Tracking** - Request ID generation and propagation
- **Compression** - gzip, br, deflate, zstd
- **CORS** - Configurable cross-origin policies
- **Timeouts** - Configurable request timeouts
- **Body Size Limits** - Prevent oversized payloads
- **Panic Recovery** - Graceful handling of panics

### HTTP + gRPC Support

Run HTTP and gRPC services together:

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

// Serve both protocols
// Currently on separate ports (HTTP: 8080, gRPC: 9090)
// Single-port multiplexing coming soon
ServiceBuilder::new()
    .with_routes(http_routes)
    .with_grpc_service(my_service::MyServiceServer::new(MyGrpcService))
    .build()
    .serve()
    .await?;
```

Configure gRPC port in `config.toml`:

```toml
[grpc]
enabled = true
port = 9090              # Separate port for gRPC
use_separate_port = true # Currently required
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
] }
```

**Note**: Some feature flags (like `resilience`, `otel-metrics`) are defined but not fully implemented yet. See the roadmap below.

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

Run examples:

```bash
cargo run --example simple-api
cargo run --example users-api
cargo run --example ping-pong --features grpc
cargo run --example event-driven --features grpc
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

- [Configuration Guide](./acton-service/CONFIG.md) - Environment and file-based configuration
- [API Versioning](./acton-service/docs/API_VERSIONING.md) - Type-safe versioning patterns
- [Health Endpoints](./HEALTH_ENDPOINTS_GUIDE.md) - Kubernetes liveness and readiness
- [Examples](./acton-service/examples/) - Complete working examples

API documentation: `cargo doc --open`

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
- Type-enforced API versioning with deprecation support
- Automatic health/readiness checks with dependency monitoring
- Structured JSON logging with distributed request tracing
- XDG-compliant configuration
- HTTP + gRPC on separate ports
- Core middleware (JWT, rate limiting, compression, CORS, timeouts)
- CLI scaffolding tool with service generation
- Database (PostgreSQL), Cache (Redis), Events (NATS) support

**In Progress** 🚧
- Single-port HTTP + gRPC multiplexing
- OpenTelemetry integration (OTLP exporter)
- Circuit breaker, retry, and bulkhead middleware
- HTTP metrics collection
- Enhanced CLI commands (add endpoint, worker, etc.)

**Planned** 📋
- GraphQL support
- WebSocket support
- Service mesh integration
- Observability dashboards
- Additional database backends

## FAQ

**Q: Why enforce versioning so strictly?**

A: API versioning is critical in production but easy to skip. Making it impossible to bypass ensures consistent team practices. The type system is the enforcement mechanism.

**Q: Can I use this without versioning?**

A: No. If you need unversioned routes, use axum directly. acton-service is opinionated about API evolution.

**Q: Does this work with existing axum middleware?**

A: Yes. Tower middleware works unchanged. Use `.layer()` with any tower middleware.

**Q: What about REST vs gRPC?**

A: Both are first-class. Run them simultaneously (currently on separate ports; single-port multiplexing coming soon), or choose one.

**Q: How does this compare to other frameworks?**

A: acton-service is opinionated where others are flexible. We trade flexibility for safety and consistency. If you need maximum control, use axum or tonic directly.

**Q: Is this production-ready?**

A: Partially. Core features (versioning, health checks, HTTP/gRPC, database support) are production-ready and battle-tested via underlying libraries (axum, tonic, sqlx). Some advanced features (OpenTelemetry integration, resilience patterns) are in progress. Review the roadmap and test thoroughly for your use case.

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
