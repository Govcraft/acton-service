---
title: Getting started
nextjs:
  metadata:
    title: acton-service - Production-grade Rust microservice framework
    description: Build microservices with enforced best practices, dual HTTP+gRPC support, and comprehensive observability out of the box.
---

**Production-grade Rust microservice framework for teams shipping to production**

Build microservices with enforced best practices, dual HTTP+gRPC support, and comprehensive observability out of the box.

---

## What is this?

Building production microservices requires solving the same problems repeatedly: API versioning, health checks, observability, resilience patterns, connection pooling, and configuration management. Most frameworks leave these as optional concerns or implementation details.

acton-service provides a **batteries-included, type-enforced framework** where production best practices are the default path:

- **[Type-enforced API versioning](/docs/api-versioning)** - Impossible to bypass, compiler-enforced versioning
- **[Dual HTTP + gRPC](/docs/dual-protocol)** - Run both protocols on the same port with automatic detection
- **[Cedar policy-based authorization](/docs/cedar-auth)** - AWS Cedar integration for fine-grained access control
- **[Production observability](/docs/observability)** - OpenTelemetry tracing, metrics, and structured logging built-in
- **[Resilience patterns](/docs/resilience)** - Circuit breaker, retry logic, and bulkhead patterns included
- **[Zero-config defaults](/docs/configuration)** - XDG-compliant configuration with sensible production defaults
- **[Kubernetes-ready](/docs/health-checks)** - Automatic health/readiness probes for orchestration

It's opinionated, comprehensive, and designed for teams where best practices can't be optional.

{% callout type="note" title="Current Status" %}
acton-service is under active development. Core features (HTTP/gRPC, versioning, health checks, observability, resilience) are production-ready. Some advanced features are in progress.
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

### The Problem

Building production microservices requires solving the same problems over and over:

- **Dual Protocols**: Modern deployments need both HTTP REST APIs and gRPC, but most frameworks make you choose one or run two separate servers
- **Observability**: Distributed tracing, metrics collection, and structured logging should be standard, not afterthoughts assembled from scattered libraries
- **[Resilience Patterns](/docs/resilience)**: Circuit breakers, retries, and bulkheads are critical for production but tedious to implement correctly
- **Health Checks**: Every orchestrator needs them, but every team implements them differently with varying quality
- **API Evolution**: Breaking changes slip through because versioning is optional and easily forgotten
- **Configuration**: Production deployments need environment-based config without requiring boilerplate for every service

### The Solution

acton-service provides a **comprehensive, opinionated framework** where production concerns are handled by default:

1. **[Dual HTTP + gRPC](/docs/dual-protocol)** - Run both protocols on the same port with automatic protocol detection, or use separate ports
2. **[Complete observability stack](/docs/observability)** - OpenTelemetry tracing, HTTP metrics, and structured JSON logging configured out of the box
3. **[Production resilience patterns](/docs/resilience)** - Circuit breaker, exponential backoff retry, and bulkhead middleware included
4. **[Automatic health endpoints](/docs/health-checks)** - Kubernetes-ready liveness and readiness probes with dependency monitoring
5. **[Type-enforced API versioning](/docs/api-versioning)** - The compiler prevents unversioned APIs; impossible to bypass
6. **[Zero-config defaults](/docs/configuration)** - XDG-compliant configuration with sensible defaults and environment variable overrides
7. **[Batteries-included middleware](/docs/middleware)** - JWT auth, Cedar policy-based authorization, rate limiting, request tracking, compression, CORS, timeouts
8. **[Connection pool management](/docs/database)** - PostgreSQL, Redis, and NATS support with automatic retry and health checks

Most importantly: **it's designed for teams**. The type system enforces best practices that individual contributors can't accidentally bypass.

---

## Core Features

acton-service provides comprehensive production-ready capabilities:

- **[Type-Safe API Versioning](/docs/api-versioning)** - Compile-time enforcement with RFC 8594 deprecation headers
- **[Automatic Health Checks](/docs/health-checks)** - Kubernetes liveness/readiness probes with dependency monitoring
- **[Batteries-Included Middleware](/docs/middleware)** - Authentication (JWT, Cedar policies), resilience (circuit breaker, retry, bulkhead), rate limiting, observability
- **[HTTP + gRPC Support](/docs/dual-protocol)** - Single-port multiplexing with automatic protocol detection
- **[Zero-Configuration Defaults](/docs/configuration)** - XDG-compliant config with environment overrides

For detailed documentation on each feature, see the [API Versioning](/docs/api-versioning), [Health Checks](/docs/health-checks), [Resilience Patterns](/docs/resilience), and [Observability](/docs/observability) guides.

---

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

See the [Installation Guide](/docs/installation) for detailed setup instructions and feature combinations.

---

## Next Steps

{% callout type="note" title="Get Started" %}
**New to acton-service?** Start with the [5-Minute Quickstart](/docs/quickstart) or follow the [Installation Guide](/docs/installation) to set up your first service.
{% /callout %}

- [Quickstart Guide](/docs/quickstart) - Get a service running in 5 minutes
- [Installation](/docs/installation) - Detailed installation and setup instructions
- [Framework Comparison](/docs/comparison) - How acton-service compares to Axum, Actix-Web, Rocket, and others
- [Examples](https://github.com/govcraft/acton-service/tree/main/acton-service/examples) - Complete working examples for common patterns
