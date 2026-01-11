---
title: Getting started
nextjs:
  metadata:
    title: acton-service - Production-ready Rust backend framework
    description: Build production backends with enforced best practices, dual HTTP+gRPC support, and comprehensive observability out of the box. Scales from monolith to microservices.
---

**Rust backend framework with type-enforced patterns**

Build production backends with dual HTTP+gRPC support, automatic health checks, and built-in observability. Works equally well for monoliths and microservices.

---

## What is this?

Building production backends often involves implementing the same patterns: API versioning, health checks, observability, resilience patterns, connection pooling, and configuration management. Many frameworks leave these as optional concerns or implementation details.

acton-service provides a **type-enforced framework** with built-in implementations of these patterns:

- **[Type-enforced API versioning](/docs/api-versioning)** - Impossible to bypass, compiler-enforced versioning
- **[Dual HTTP + gRPC](/docs/dual-protocol)** - Run both protocols on the same port with automatic detection
- **[Cedar policy-based authorization](/docs/cedar-auth)** - AWS Cedar integration for fine-grained access control
- **[Observability](/docs/observability)** - OpenTelemetry tracing, metrics, and structured logging built-in
- **[Resilience patterns](/docs/resilience)** - Circuit breaker, retry logic, and bulkhead patterns included
- **[Zero-config defaults](/docs/configuration)** - XDG-compliant configuration with sensible production defaults
- **[Kubernetes-ready](/docs/health-checks)** - Automatic health/readiness probes for orchestration

It's opinionated and designed for teams that want consistent patterns across services.

{% callout type="note" title="Current Status" %}
acton-service is under active development (v{% $version.acton %}). Core features have stable APIs: HTTP/gRPC servers, type-enforced versioning, health checks, observability, and resilience middleware. Some advanced CLI features are in progress. The framework is built on mature libraries (axum, tonic, sqlx). Test thoroughly for your use case.
{% /callout %}

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

---

## Why acton-service?

### Common Challenges

Building production backends often involves solving recurring problems:

- **Dual Protocols**: Supporting both HTTP REST APIs and gRPC typically requires choosing one or running two separate servers
- **Observability**: Distributed tracing, metrics collection, and structured logging need integration across multiple libraries
- **[Resilience Patterns](/docs/resilience)**: Circuit breakers, retries, and bulkheads require careful implementation
- **Health Checks**: Orchestrators need health endpoints, but implementation approaches vary
- **API Evolution**: Breaking changes can occur when versioning is optional
- **Configuration**: Environment-based config often requires boilerplate for each service

### acton-service Approach

acton-service provides opinionated solutions to these challenges:

1. **[Dual HTTP + gRPC](/docs/dual-protocol)** - Run both protocols on the same port with automatic protocol detection, or use separate ports
2. **[Observability stack](/docs/observability)** - OpenTelemetry tracing, HTTP metrics, and structured JSON logging configured by default
3. **[Resilience patterns](/docs/resilience)** - Circuit breaker, exponential backoff retry, and bulkhead middleware included
4. **[Automatic health endpoints](/docs/health-checks)** - Kubernetes liveness and readiness probes with dependency monitoring
5. **[Type-enforced API versioning](/docs/api-versioning)** - Compiler prevents unversioned APIs through opaque types
6. **[Configuration system](/docs/configuration)** - XDG-compliant configuration with defaults and environment variable overrides
7. **[Middleware library](/docs/middleware)** - JWT auth, Cedar policy-based authorization, rate limiting, request tracking, compression, CORS, timeouts
8. **[Connection pool management](/docs/database)** - PostgreSQL, Redis, and NATS support with automatic retry and health checks

The type system enforces these patterns at compile time.

---

## Core Features

acton-service provides these capabilities:

- **[Type-Safe API Versioning](/docs/api-versioning)** - Compile-time enforcement with RFC 8594 deprecation headers
- **[Automatic Health Checks](/docs/health-checks)** - Kubernetes liveness/readiness probes with dependency monitoring
- **[Middleware Library](/docs/middleware)** - Authentication (JWT, Cedar policies), resilience (circuit breaker, retry, bulkhead), rate limiting, observability
- **[HTTP + gRPC Support](/docs/dual-protocol)** - Single-port multiplexing with automatic protocol detection
- **[Configuration System](/docs/configuration)** - XDG-compliant config with environment overrides

For detailed documentation on each feature, see the [API Versioning](/docs/api-versioning), [Health Checks](/docs/health-checks), [Resilience Patterns](/docs/resilience), and [Observability](/docs/observability) guides.

---

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
acton-service = { version = "{% $version.acton %}", features = ["http", "observability"] }
tokio = { version = "1", features = ["full"] }
```

Or use the CLI to scaffold a complete service:

```bash
cargo install acton-cli
acton service new my-api --yes
cd my-api && cargo run
```

See the [Installation Guide](/docs/installation) for detailed setup instructions and feature combinations.

---

## Next Steps

{% callout type="note" title="Get Started" %}
**New to acton-service?** Start with the [5-Minute Quickstart](/docs/quickstart) or follow the [Installation Guide](/docs/installation) to set up your first service.
{% /callout %}

- [Quickstart Guide](/docs/quickstart) - Get a service running in 5 minutes
- [Installation](/docs/installation) - Detailed installation and setup instructions
- [Framework Comparison](/docs/comparison) - How acton-service compares to Axum, Actix-Web, Rocket, and others
- [Examples]({% $github.repositoryUrl %}/tree/main/acton-service/examples) - Complete working examples for common patterns
