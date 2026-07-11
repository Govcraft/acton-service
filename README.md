# acton-service

**Production-ready Rust backend framework that scales from monolith to microservices**

Build production backends with enforced best practices, dual HTTP+gRPC support, and comprehensive observability out of the box.

**📚 [Full Documentation](https://govcraft.github.io/acton-service/)** | [Quick Start](#quick-start) | [Examples](https://govcraft.github.io/acton-service/docs/examples)

---

## What is this?

Building production backends requires solving the same problems repeatedly: API versioning, health checks, observability, resilience patterns, authentication, connection pooling, and configuration management. Most frameworks leave these as optional concerns or implementation details.

acton-service provides a **batteries-included, type-enforced framework** where production best practices are the default path:

- **Type-enforced API versioning** - Impossible to bypass, compiler-enforced versioning
- **Dual HTTP + gRPC** - Run both protocols on the same port with automatic detection
- **GraphQL transport** - Versioned schemas that inherit the same middleware stack
- **Complete authentication stack** - PASETO (default) and JWT tokens, Argon2 password hashing, API keys, key rotation, OAuth/OIDC, sessions
- **Cedar policy-based authorization** - AWS Cedar integration for fine-grained access control
- **Security & compliance built-in** - BLAKE3 hash-chained audit logging, login lockout, NIST AC-2 account lifecycle, FIPS 140-3 capable crypto
- **Production observability** - OpenTelemetry tracing, metrics, and structured logging built-in
- **Resilience patterns** - Circuit breaker, retry logic, and bulkhead patterns included
- **Multi-database support** - PostgreSQL, Turso/libsql, SurrealDB, plus ClickHouse for analytics
- **Zero-config defaults** - XDG-compliant configuration with sensible production defaults and custom config extensions
- **Kubernetes-ready** - Automatic health/readiness probes for orchestration

It's opinionated, comprehensive, and designed for teams where best practices can't be optional.

**Current Status**: acton-service is under active development but broadly feature-complete. Transports (HTTP/gRPC/GraphQL/WebSocket/SSE), versioning, health checks, observability, resilience, the authentication and authorization stack, sessions, audit logging, and multi-database support all ship today. See the [roadmap](#roadmap) for what's next.

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
acton-service = "0.27"
tokio = { version = "1", features = ["full"] }
```

The default features (`http`, `observability`, `crypto-aws-lc-rs`) give you a versioned HTTP service with tracing, metrics, and health checks. Enable more as you need them — see [Feature Flags](#feature-flags).

Or use the CLI to scaffold a complete service:

```bash
cargo install acton-cli
acton service new my-api --yes
cd my-api && cargo run
```

## Why acton-service?

### The Problem

Building production backends requires solving the same problems over and over:

- **Dual Protocols**: Modern deployments need both HTTP REST APIs and gRPC, but most frameworks make you choose one or run two separate servers
- **Observability**: Distributed tracing, metrics collection, and structured logging should be standard, not afterthoughts assembled from scattered libraries
- **Authentication**: Token validation, password hashing, key rotation, OAuth flows, and session management are security-critical and easy to get wrong
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
6. **Zero-config defaults** - XDG-compliant configuration with sensible defaults, environment variable overrides, and custom config extensions ✅
7. **Config-driven security** - PASETO/JWT authentication, Cedar policy authorization, rate limiting, sessions, and audit logging wired automatically from configuration ✅
8. **Full identity stack** - Password hashing (Argon2), API keys, signing-key rotation, OAuth/OIDC providers, login lockout, and account lifecycle management ✅
9. **Connection pool management** - PostgreSQL, Turso, SurrealDB, ClickHouse, Redis, and NATS support with automatic retry and health checks ✅

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

### Config-Driven Middleware

Production middleware is **applied automatically from configuration** — no manual layering required. Configure token authentication, rate limiting, sessions, and audit logging in `config.toml`, and `ServiceBuilder` wires the layers in the correct order:

```toml
# config.toml — PASETO v4 token authentication (the default token format)
[token.paseto]
version = "v4"
purpose = "public"                 # "local" (symmetric) or "public" (asymmetric)
key_path = "./keys/paseto.key"
issuer = "my-service"
# public_paths = ["/public/"]      # Skip token auth for these prefixes

# Or JWT instead (requires the `jwt` feature)
# [token.jwt]
# public_key_path = "./keys/jwt-public.pem"
# algorithm = "RS256"              # RS256, ES256, HS256
# issuer = "https://auth.mydomain.com"

[rate_limit]
auto_apply = true
```

Validated claims are placed in request extensions, available to handlers, Cedar policies, GraphQL resolvers, and gRPC interceptors alike. Because everything is standard Tower middleware, you can still `.layer()` any custom or third-party middleware on your routers.

Available middleware (all HTTP and gRPC compatible):

**Authentication & Authorization**
- **PASETO Authentication** (default) - v4 local (symmetric) and public (asymmetric) token validation
- **JWT Authentication** (`jwt` feature) - RS256, ES256, HS256/384/512 algorithms
- **Cedar Policy-Based Authorization** - AWS Cedar integration for fine-grained access control with:
  - Declarative policy files for resource-based permissions
  - Role-based and attribute-based access control (RBAC/ABAC)
  - Manual policy reload endpoint (automatic hot-reload in progress)
  - Optional Redis caching for sub-5ms policy decisions
  - HTTP and gRPC support with customizable path normalization
- Claims structure with roles, permissions, user/client identification
- Redis-backed token revocation (optional)

**Resilience & Reliability** (`resilience` feature)
- **Circuit Breaker** - Configurable failure rate monitoring with auto-recovery
- **Retry Logic** - Exponential backoff with configurable max attempts
- **Bulkhead** - Concurrency limiting with wait timeouts to prevent overload

**Rate Limiting**
- **Redis-backed rate limiting** - Distributed rate limiting for multi-instance deployments
- **Governor rate limiting** (`governor` feature) - Local in-memory limiting with per-route configuration
- Per-user and per-client limits via token claims

**Observability**
- **Request Tracking** - UUID-based request ID generation and propagation
- **Distributed Tracing Headers** - x-request-id, x-trace-id, x-span-id, x-correlation-id
- **OpenTelemetry Metrics** (`otel-metrics` feature) - HTTP request count, duration histograms, active requests, sizes
- **Sensitive Header Masking** - Automatic masking in logs (authorization, cookies, API keys)
- **Security Headers** - Sensible defaults applied automatically

**Standard HTTP Middleware**
- **Compression** - gzip, br, deflate, zstd content encoding
- **CORS** - Configurable cross-origin policies
- **Timeouts** - Configurable request timeouts
- **Body Size Limits** - Prevent oversized payloads
- **Panic Recovery** - Graceful handling of panics with error logging

### Authentication & Identity

Beyond token validation, the `auth` feature family provides a complete identity stack:

- **Password hashing** (`auth`) - Argon2id with secure defaults ([guide](https://govcraft.github.io/acton-service/docs/password-hashing))
- **Token generation** (`auth`) - Mint PASETO or JWT tokens with typed claims ([guide](https://govcraft.github.io/acton-service/docs/token-generation))
- **API keys** (`auth`) - BLAKE3-hashed API key issuance and validation ([guide](https://govcraft.github.io/acton-service/docs/api-keys))
- **Signing-key rotation** (`auth`) - Rotate token signing keys with a drain grace period for in-flight tokens
- **OAuth 2.0 / OIDC** (`oauth`) - Pluggable provider integration built on `oauth2` and `openidconnect` ([guide](https://govcraft.github.io/acton-service/docs/oauth))
- **Sessions** (`session-memory` / `session-redis`) - Cookie sessions via `tower-sessions` with in-memory or Redis stores ([guide](https://govcraft.github.io/acton-service/docs/session))
- **Login lockout** (`login-lockout`) - Progressive delays and account lockout on repeated failures ([guide](https://govcraft.github.io/acton-service/docs/login-lockout))
- **Account lifecycle** (`accounts` / `account-handlers`) - NIST AC-2 aligned account management with optional pre-built REST handlers

Enable `auth-full` to get the entire stack in one flag.

### Security, Audit & Compliance

- **Audit logging** (`audit`) - BLAKE3 hash-chained, tamper-evident audit trails for auth, account, and request events, with storage backends for PostgreSQL, Turso, SurrealDB, and ClickHouse ([guide](https://govcraft.github.io/acton-service/docs/audit))
- **TLS termination** (`tls`) - rustls-based HTTPS with automatic crypto-provider installation
- **FIPS 140-3 path** - `aws-lc-rs` is the default crypto provider; see [Choosing a Crypto Provider](#choosing-a-crypto-provider)
- **systemd journald** (`journald`) - Native journal integration for structured logs on Linux hosts

### HTTP + gRPC Support

Run HTTP and gRPC services together on a single port:

```rust
use acton_service::grpc::server::GrpcServicesBuilder;
use acton_service::prelude::*;

// HTTP handlers
let http_routes = VersionedApiBuilder::new()
    .add_version(ApiVersion::V1, |router| {
        router.route("/users", get(list_users))
    })
    .build_routes();

// gRPC services with health checks and reflection
let grpc_routes = GrpcServicesBuilder::new()
    .with_health()
    .with_reflection()
    .add_file_descriptor_set(hello::FILE_DESCRIPTOR_SET)
    .add_service(HelloServiceServer::new(HelloService::default()))
    .build(None);

// Serve both protocols on the same port (automatic protocol detection)
ServiceBuilder::new()
    .with_routes(http_routes)
    .with_grpc_services(grpc_routes)
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

### GraphQL Transport

Enable the `graphql` feature to expose schemas alongside REST and gRPC. Each
schema is mounted at `/{base}/v{n}/graphql` under the same versioned router,
so it inherits the framework middleware stack (auth, tracing, rate limiting,
Cedar, CORS). `GET` on the same path serves GraphiQL.

```rust
use acton_service::prelude::*;
use acton_service::graphql::{GraphQLContextExt, VersionedGraphQLBuilder};
use async_graphql::{Context, EmptyMutation, EmptySubscription, Object, Schema};

struct Query;

#[Object]
impl Query {
    async fn hello(&self) -> &'static str { "world" }

    async fn whoami(&self, ctx: &Context<'_>) -> String {
        // Claims placed in request extensions by PASETO/JWT middleware are
        // forwarded into the resolver Context automatically.
        ctx.claims().map(|c| c.sub.clone()).unwrap_or("anon".into())
    }
}

let routes = VersionedApiBuilder::new()
    .with_base_path("/api")
    .add_version(ApiVersion::V1, |r| r)
    .build_routes();

let schema = Schema::build(Query, EmptyMutation, EmptySubscription).finish();
let graphql = VersionedGraphQLBuilder::new()
    .with_base_path("/api")
    .add_version(ApiVersion::V1, schema)
    .build();

ServiceBuilder::new()
    .with_routes(routes)
    .with_versioned_graphql(graphql)
    .build()
    .serve()
    .await?;
```

With the `graphql-cedar` feature enabled, resolvers can call into the same
Cedar instance used by HTTP/gRPC middleware:

```rust
use acton_service::graphql::CedarResolverCheck;

async fn document(ctx: &Context<'_>, id: String) -> async_graphql::Result<String> {
    CedarResolverCheck::for_context(ctx)?
        .with_action("readDocument")
        .with_resource_type("Document")
        .with_resource_id(&id)
        .authorize()
        .await
        .map_err(|e| async_graphql::Error::new(e.to_string()))?;
    Ok(format!("Document {} contents", id))
}
```

Configure runtime knobs (depth, complexity, GraphiQL, introspection) under
`[graphql]` in `config.toml`:

```toml
[graphql]
enabled = true
graphiql_enabled = true
introspection_enabled = true
# max_query_depth = 12
# max_query_complexity = 200
```

Scaffold a graphql-ready service with the CLI:

```bash
acton service new my-svc --graphql                # new project
acton service add graphql                         # retrofit existing project
acton service add graphql --cedar                 # add Cedar resolver guard
```

### WebSocket, SSE & Server-Rendered Web Apps

The framework isn't limited to APIs:

- **WebSocket** (`websocket` feature) - Real-time bidirectional connections mounted under the versioned router ([guide](https://govcraft.github.io/acton-service/docs/websocket), [chat-server example](./acton-service/examples/websocket/chat-server.rs))
- **Server-Sent Events** (`sse` feature) - Streaming updates over plain HTTP ([guide](https://govcraft.github.io/acton-service/docs/sse))
- **HTMX integration** (`htmx` feature) - Request/response header extractors via `axum-htmx` ([guide](https://govcraft.github.io/acton-service/docs/htmx))
- **Askama templates** (`askama` feature) - Compile-time checked HTML templates ([guide](https://govcraft.github.io/acton-service/docs/askama))

Enable `htmx-full` (htmx + askama + sse + in-memory sessions) and you have everything needed for a hypermedia-driven web app — see the [task-manager example](./acton-service/examples/htmx/task-manager.rs).

### Data Layer

Choose exactly one primary database backend (enforced at compile time), and optionally add ClickHouse for analytics:

- **PostgreSQL** (`database`) - SQLx with compile-time checked queries ([guide](https://govcraft.github.io/acton-service/docs/database))
- **Turso / libsql** (`turso`) - Edge-replicated SQLite ([guide](https://govcraft.github.io/acton-service/docs/turso))
- **SurrealDB** (`surrealdb`) - Multi-model database
- **ClickHouse** (`clickhouse`) - Analytical database, composable with any primary backend ([guide](https://govcraft.github.io/acton-service/docs/clickhouse))
- **Redis** (`cache`) - Connection pooling via deadpool ([guide](https://govcraft.github.io/acton-service/docs/cache))
- **NATS JetStream** (`events`) - Event streaming ([guide](https://govcraft.github.io/acton-service/docs/events))

On top of the raw pools, opt into higher-level abstractions:

- **Repository traits** (`repository`) - Database-agnostic CRUD abstractions
- **Handler traits** (`handlers`) - Pre-built REST CRUD patterns on top of repositories
- **Pagination** (`pagination-axum` / `pagination-sqlx` / `pagination-full`) - Query-parameter extraction and SQL pagination helpers

All pools are managed by `AppState` with automatic connection retry and health-check integration:

```rust
async fn list_products(State(state): State<AppState>) -> Result<Json<Vec<Product>>> {
    let db = state.db().await.ok_or(Error::Internal("database unavailable".into()))?;
    let products = sqlx::query_as!(Product, "SELECT id, name FROM products")
        .fetch_all(&db)
        .await?;
    Ok(Json(products))
}
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

**Custom Config Extensions**: Extend the framework configuration with your own application-specific fields that are automatically loaded from the same `config.toml`:

```rust
#[derive(Serialize, Deserialize, Clone, Default)]
struct MyCustomConfig {
    api_key: String,
    feature_flags: HashMap<String, bool>,
}

// Framework automatically loads both framework and custom config
ServiceBuilder::<MyCustomConfig>::new()
    .with_routes(routes)
    .build()
    .serve()
    .await
```

See the [Configuration Guide](https://govcraft.github.io/acton-service/docs/configuration#custom-configuration-extensions) for details.

## Feature Flags

Defaults: `http`, `observability`, `crypto-aws-lc-rs`. Enable only what you need:

```toml
[dependencies]
acton-service = { version = "0.27", features = ["grpc", "database", "cache"] }
```

**Transports & protocols**

| Feature | Description |
|---|---|
| `http` | Axum HTTP framework (default) |
| `grpc` | Tonic gRPC with reflection, health checks, and interceptors |
| `graphql` | async-graphql transport, versioned alongside HTTP/gRPC |
| `graphql-cedar` | Cedar authorization callable from GraphQL resolvers |
| `websocket` | WebSocket support |
| `sse` | Server-Sent Events |
| `tls` | rustls-based HTTPS |
| `openapi` | Swagger UI / RapiDoc / ReDoc documentation |

**Data layer** (`database`, `turso`, and `surrealdb` are mutually exclusive — pick one)

| Feature | Description |
|---|---|
| `database` | PostgreSQL via SQLx |
| `turso` | Turso / libsql |
| `surrealdb` | SurrealDB |
| `clickhouse` | ClickHouse analytics (composable with any primary backend) |
| `cache` | Redis connection pooling |
| `events` | NATS JetStream |
| `repository` | Database-agnostic CRUD repository traits |
| `handlers` | REST CRUD handler traits (implies `repository`) |
| `pagination`, `pagination-axum`, `pagination-sqlx`, `pagination-full` | Pagination helpers |

**Authentication & security** (PASETO validation is always available; these add more)

| Feature | Description |
|---|---|
| `jwt` | JWT validation middleware |
| `auth` | Argon2 password hashing, token generation, API keys, key rotation |
| `oauth` | OAuth 2.0 / OIDC providers (implies `auth`) |
| `auth-full` | Everything: auth + oauth + jwt + cache + database + lockout + accounts |
| `cedar-authz` | AWS Cedar policy-based authorization |
| `session`, `session-memory`, `session-redis` | Cookie sessions (tower-sessions) |
| `login-lockout` | Progressive delay and account lockout |
| `accounts`, `account-handlers` | NIST AC-2 account lifecycle (+ pre-built REST handlers) |
| `audit` | BLAKE3 hash-chained audit logging |

**Web apps**

| Feature | Description |
|---|---|
| `htmx` | HTMX request/response integration |
| `askama` | Askama template engine |
| `htmx-full` | htmx + askama + sse + session-memory |

**Observability & resilience**

| Feature | Description |
|---|---|
| `observability` | OpenTelemetry tracing + structured logging (default) |
| `otel-metrics` | HTTP metrics middleware (implies `observability`) |
| `journald` | Native systemd journal logging |
| `resilience` | Circuit breaker + bulkhead middleware |
| `governor` | Local in-memory rate limiting |

**Crypto provider** (exactly one required — see below)

| Feature | Description |
|---|---|
| `crypto-aws-lc-rs` | aws-lc-rs rustls provider (default, FIPS 140-3 capable) |
| `crypto-ring` | ring rustls provider (no C toolchain requirement) |

Or use `full` to enable everything (with PostgreSQL as the database backend):

```toml
[dependencies]
acton-service = { version = "0.27", features = ["full"] }
```

See the [Feature Flags guide](https://govcraft.github.io/acton-service/docs/feature-flags) for a decision tree.

### Choosing a Crypto Provider

acton-service uses `rustls` for all TLS, with `aws-lc-rs` as the default
crypto provider. `aws-lc-rs` is FIPS 140-3 capable (via its `fips` feature),
aligned with rustls 0.23+, tonic 0.14+, and sqlx 0.8+, and faster than `ring`
on server hardware.

To use `ring` instead — for example, in environments without a C toolchain
that `aws-lc-rs` requires at build time:

```toml
[dependencies]
acton-service = { version = "0.27", default-features = false, features = [
    "http",
    "observability",
    "crypto-ring",
] }
```

Exactly one of `crypto-aws-lc-rs` (default) or `crypto-ring` must be enabled;
the build fails with a `compile_error!` otherwise. The chosen provider is
installed automatically when the TLS listener starts. Binaries that drive
`reqwest`, `sqlx`, or `tonic` TLS clients without using the framework's TLS
listener should call `acton_service::crypto::ensure_default_crypto_provider()`
once from `main`.

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

### All Bundled Examples

| Example | Shows | Run with |
|---|---|---|
| [`simple-api`](./acton-service/examples/basic/simple-api.rs) | Versioned API, zero config | `cargo run --example simple-api` |
| [`users-api`](./acton-service/examples/basic/users-api.rs) | Deprecation headers (RFC 8594) | `cargo run --example users-api` |
| [`custom-config`](./acton-service/examples/custom-config.rs) | Custom configuration extensions | `cargo run --example custom-config` |
| [`ping-pong`](./acton-service/examples/basic/ping-pong.rs) | Dual-protocol HTTP + gRPC | `cargo run --example ping-pong --features grpc` |
| [`single-port`](./acton-service/examples/grpc/single-port.rs) | Single-port protocol detection, reflection, gRPC health | `cargo run --example single-port --features grpc` |
| [`event-driven`](./acton-service/examples/events/event-driven.rs) | Event-driven architecture with NATS | `cargo run --example event-driven --features grpc` |
| [`cedar-authz`](./acton-service/examples/authorization/cedar-authz.rs) | Cedar policies ([guide](./acton-service/examples/authorization/README.md)) | `cargo run --example cedar-authz --features cedar-authz,cache` |
| [`database-api`](./acton-service/examples/database/database-api.rs) | PostgreSQL CRUD with SQLx | `cargo run --example database-api --features database` |
| [`chat-server`](./acton-service/examples/websocket/chat-server.rs) | WebSocket real-time chat | `cargo run --example chat-server --features websocket` |
| [`task-manager`](./acton-service/examples/htmx/task-manager.rs) | HTMX + Askama + SSE + sessions web app | `cargo run --example task-manager --features htmx-full` |
| [`graphql-basic`](./acton-service/examples/graphql/graphql-basic.rs) | Versioned GraphQL transport | `cargo run --example graphql-basic --features graphql` |
| [`test-observability`](./acton-service/examples/observability/test-observability.rs) | Tracing/logging setup | `cargo run --example test-observability` |
| [`test-metrics`](./acton-service/examples/observability/test-metrics.rs) | OpenTelemetry HTTP metrics | `cargo run --example test-metrics --features otel-metrics` |

See the [Examples documentation](https://govcraft.github.io/acton-service/docs/examples) for walkthroughs.

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
    --grpc \
    --database postgres \
    --cache redis \
    --events nats \
    --observability \
    --graphql

# Add components to an existing service
cd user-service
acton service add endpoint POST /users --handler create_user
acton service add worker email-worker --source nats --stream emails
acton service add grpc PaymentService
acton service add graphql --cedar
acton service add middleware rate-limit
acton service add version v2 --from v1

# Generate deployment and config artifacts
acton service generate deployment --hpa --monitoring
acton service generate config
acton service generate proto

# Validate and iterate
acton service validate
acton service dev
```

See the [CLI documentation](https://govcraft.github.io/acton-service/docs/cli-overview) for details.

## Architecture

acton-service is built on production-proven Rust libraries:

- **HTTP**: [axum](https://github.com/tokio-rs/axum) - Ergonomic web framework
- **gRPC**: [tonic](https://github.com/hyperium/tonic) - Native Rust gRPC
- **GraphQL**: [async-graphql](https://github.com/async-graphql/async-graphql) - GraphQL server library
- **Database**: [SQLx](https://github.com/launchbadge/sqlx) (PostgreSQL), [libsql](https://github.com/tursodatabase/libsql) (Turso), [SurrealDB](https://github.com/surrealdb/surrealdb), [clickhouse-rs](https://github.com/ClickHouse/clickhouse-rs)
- **Cache**: [redis-rs](https://github.com/redis-rs/redis-rs) - Redis client
- **Events**: [async-nats](https://github.com/nats-io/nats.rs) - NATS client
- **Tokens**: [rusty_paseto](https://github.com/rrrodzilla/rusty_paseto) - PASETO implementation
- **Observability**: [OpenTelemetry](https://github.com/open-telemetry/opentelemetry-rust) - Distributed tracing
- **Concurrency**: [acton-reactive](https://github.com/Govcraft/acton-reactive) - Actor-based background workers

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
- **[Database](https://govcraft.github.io/acton-service/docs/database)** - PostgreSQL, [Turso](https://govcraft.github.io/acton-service/docs/turso), and [ClickHouse](https://govcraft.github.io/acton-service/docs/clickhouse)
- **[Token Authentication](https://govcraft.github.io/acton-service/docs/token-auth)** - PASETO and JWT validation
- **[OAuth / OIDC](https://govcraft.github.io/acton-service/docs/oauth)** - Provider integration
- **[Sessions](https://govcraft.github.io/acton-service/docs/session)** - Cookie sessions with memory or Redis stores
- **[Cedar Authorization](https://govcraft.github.io/acton-service/docs/cedar-auth)** - Policy-based access control
- **[Audit Logging](https://govcraft.github.io/acton-service/docs/audit)** - Tamper-evident audit trails
- **[Rate Limiting](https://govcraft.github.io/acton-service/docs/rate-limiting)** - Distributed and local strategies
- **[Resilience](https://govcraft.github.io/acton-service/docs/resilience)** - Circuit breaker, retry, bulkhead
- **[GraphQL](https://govcraft.github.io/acton-service/docs/graphql-guide)** - Versioned GraphQL transport
- **[WebSocket](https://govcraft.github.io/acton-service/docs/websocket)** - Real-time connections
- **[HTMX](https://govcraft.github.io/acton-service/docs/htmx)** - Hypermedia-driven web apps
- **[Crypto Provider](https://govcraft.github.io/acton-service/docs/crypto-provider)** - aws-lc-rs vs ring, FIPS
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

See the [Examples documentation](https://govcraft.github.io/acton-service/docs/examples) for complete migration examples.

## Roadmap

**Implemented** ✅
- **Dual Protocol Support**: Single-port HTTP + gRPC multiplexing with automatic protocol detection
- **GraphQL Transport**: async-graphql integration with path-versioned schemas, GraphiQL, claims propagation, and Cedar resolver authorization
- **WebSocket & SSE**: Real-time connections and server-sent events under the versioned router
- **Complete Observability Stack**: OpenTelemetry tracing/metrics (OTLP exporter), structured JSON logging, journald integration, distributed request tracing
- **Production Resilience Patterns**: Circuit breaker, exponential backoff retry, bulkhead (concurrency limiting)
- **Token Authentication**: PASETO v4 (default) and JWT (RS256/ES256/HS256/384/512), config-driven and auto-applied, with Redis-backed revocation
- **Identity Stack**: Argon2 password hashing, API keys, token generation, signing-key rotation, OAuth 2.0/OIDC providers, cookie sessions (memory/Redis)
- **Security & Compliance**: BLAKE3 hash-chained audit logging with pluggable storage, login lockout, NIST AC-2 account lifecycle, TLS with FIPS 140-3 capable crypto provider
- **Cedar Policy-Based Authorization**: AWS Cedar integration with declarative policies, manual reload endpoint, Redis caching, HTTP/gRPC/GraphQL support, customizable path normalization
- **Type-Enforced API Versioning**: Compile-time enforcement with RFC 8594 deprecation headers
- **Automatic Health Checks**: Kubernetes-ready liveness/readiness probes with dependency monitoring (database, cache, events)
- **Multi-Database Support**: PostgreSQL (SQLx), Turso/libsql, SurrealDB, ClickHouse analytics, Redis (Deadpool), NATS JetStream — all with automatic retry and health checks
- **Persistence Abstractions**: Repository and REST handler traits, pagination helpers
- **Web App Stack**: HTMX integration, Askama templates, session-backed hypermedia apps
- **XDG-Compliant Configuration**: Multi-source config with environment variable overrides and sensible defaults
- **OpenAPI/Swagger Support**: Multiple UI options (Swagger UI, RapiDoc, ReDoc) with multi-version documentation
- **CLI Tooling**: Service scaffolding plus `add endpoint/worker/grpc/graphql/middleware/version`, `generate config/deployment/proto`, `validate`, and `dev` commands
- **gRPC Features**: Reflection service, health checks, interceptors, middleware parity with HTTP

**In Progress** 🚧
- Cedar automatic policy hot-reload (manual reload endpoint available today)
- Additional OpenAPI schema generation utilities

**Planned** 📋
- GraphQL subscriptions over WebSocket
- Service mesh integration (Istio, Linkerd)
- Additional database backends (MySQL, MongoDB)
- Observability dashboards and sample configurations
- Enhanced metrics (custom business metrics, SLO tracking)
- Advanced rate limiting strategies (sliding log, token bucket refinements)

## FAQ

**Q: How does this compare to using Axum or Tonic directly?**

A: acton-service is built on top of Axum (HTTP) and Tonic (gRPC) but adds production-ready features as defaults: type-enforced versioning, automatic health checks, observability stack, resilience patterns, a complete auth stack, and connection pool management. If you need maximum flexibility, use the underlying libraries directly. If you want production best practices enforced by the type system, use acton-service.

**Q: Can I run both HTTP and gRPC on the same port?**

A: Yes! This is a core feature. acton-service provides automatic protocol detection allowing HTTP and gRPC to share a single port, or you can configure separate ports if preferred.

**Q: Does this work with existing axum middleware?**

A: Yes. All Tower middleware works unchanged. Use `.layer()` with any tower middleware. The framework's own middleware (token auth, Cedar, rate limiting, audit, sessions) is applied automatically from configuration in the correct order.

**Q: Why PASETO instead of JWT by default?**

A: PASETO (Platform-Agnostic Security Tokens) eliminates JWT's algorithm-confusion pitfalls by fixing the algorithm per version. JWT remains fully supported behind the `jwt` feature — configure whichever your infrastructure requires under `[token]`.

**Q: Why enforce versioning so strictly?**

A: API versioning is critical in production but easy to skip when deadlines loom. Making it impossible to bypass via the type system ensures consistent team practices and prevents breaking changes from slipping through.

**Q: Can I use this without the enforced versioning?**

A: No. If you need unversioned routes, use axum directly. acton-service is opinionated about API evolution and production best practices.

**Q: Is this production-ready?**

A: Yes, with the usual caveats of a pre-1.0 crate: expect occasional breaking changes between minor versions (documented in the [CHANGELOG](./CHANGELOG.md)). The framework is built on battle-tested libraries (axum, tonic, sqlx) and its features — transports, auth, authorization, audit, resilience, pooling — ship complete with tests. Review the roadmap and test thoroughly for your use case.

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

See [`CHANGELOG.md`](./CHANGELOG.md) for version history.

## License

Licensed under the MIT License. See [`LICENSE`](./LICENSE) for details.

## Credits

Built with excellent open source libraries:

- [tokio](https://tokio.rs) - Async runtime
- [axum](https://github.com/tokio-rs/axum) - Web framework
- [tonic](https://github.com/hyperium/tonic) - gRPC implementation
- [tower](https://github.com/tower-rs/tower) - Middleware foundation
- [SQLx](https://github.com/launchbadge/sqlx) - Database client
- [rusty_paseto](https://github.com/rrrodzilla/rusty_paseto) - PASETO tokens

Inspired by production challenges at scale. Built by developers who've maintained backend services in production.

## Sponsor

Govcraft is a one-person shop—no corporate backing, no investors, just me building useful tools. If this project helps you, [sponsoring](https://github.com/sponsors/Govcraft) keeps the work going.

[![Sponsor on GitHub](https://img.shields.io/badge/Sponsor-%E2%9D%A4-%23db61a2?logo=GitHub)](https://github.com/sponsors/Govcraft)
