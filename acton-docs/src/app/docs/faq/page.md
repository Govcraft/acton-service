---
title: FAQ
nextjs:
  metadata:
    title: Frequently Asked Questions
    description: Common questions about acton-service features, design decisions, and production readiness
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


Common questions about acton-service features, design decisions, and production readiness.

---

## How does this compare to using Axum or Tonic directly?

acton-service is built on top of Axum (HTTP) and Tonic (gRPC) but adds production-ready features as defaults: type-enforced versioning, automatic health checks, observability stack, resilience patterns, and connection pool management.

If you need maximum flexibility, use the underlying libraries directly. If you want production best practices enforced by the type system, use acton-service.

---

## Can I run both HTTP and gRPC on the same port?

Yes! This is a core feature. acton-service provides automatic protocol detection allowing HTTP and gRPC to share a single port, or you can configure separate ports if preferred.

See the [`ping-pong` example](https://github.com/Govcraft/acton-service/blob/main/acton-service/examples/basic/ping-pong.rs) for a complete demonstration.

---

## Does this work with existing Axum middleware?

Yes. All Tower middleware works unchanged. Use `.layer()` with any tower middleware.

The framework includes comprehensive middleware for:
- JWT authentication
- Rate limiting
- Resilience patterns (circuit breaker, retry, bulkhead)
- Request tracking and correlation IDs
- Metrics collection

---

## Why enforce versioning so strictly?

API versioning is critical in production but easy to skip when deadlines loom. Making it impossible to bypass via the type system ensures consistent team practices and prevents breaking changes from slipping through.

The type system forces you to think about API evolution from day one, not as an afterthought when you need to make breaking changes.

---

## Can I use this without the enforced versioning?

No. If you need unversioned routes, use Axum directly. acton-service is opinionated about API evolution and production best practices.

The entire design philosophy is built around preventing common production issues through compile-time guarantees.

---

## What's the current status?

**Version 0.2.x** - Core features have stable APIs:
- HTTP/gRPC servers with protocol detection
- Type-enforced API versioning
- Health checks (liveness/readiness)
- Observability (OpenTelemetry tracing/metrics)
- Resilience patterns (circuit breaker, retry, bulkhead)
- Middleware stack (auth, rate limiting, correlation IDs)
- Connection pooling (PostgreSQL, Redis, NATS)

The framework is built on established libraries (axum, tonic, sqlx).

Some advanced CLI features are in progress. Review the [roadmap](https://github.com/Govcraft/acton-service#roadmap) and test thoroughly for your use case before deploying.

---

## What's the performance overhead?

The framework provides type-safe abstractions over high-performance libraries (tokio, axum, tonic).

Type-enforced versioning uses compile-time checks with zero runtime cost. Optional middleware (circuit breakers, metrics, authentication) adds overhead proportional to the features enabled. Performance characteristics depend primarily on:

- The underlying libraries (axum for HTTP, tonic for gRPC, sqlx for database)
- Which middleware features you enable
- Your application logic and workload patterns
- Infrastructure configuration (connection pool sizes, cache settings)

For performance-critical applications, benchmark your specific use case with your workload.

---

## How do I handle database migrations?

acton-service doesn't enforce a specific migration strategy. Popular approaches:

**SQLx migrations** (recommended):
```bash
# Create migration
sqlx migrate add create_users_table

# Run migrations
sqlx migrate run
```

**Diesel migrations**:
```bash
diesel migration generate create_users_table
diesel migration run
```

**External tools**: Flyway, Liquibase, or custom scripts.

The framework focuses on connection management, not schema evolution.

---

## Can I use a different database besides PostgreSQL?

Currently, acton-service focuses on PostgreSQL via SQLx. The architecture supports other databases, but they're not included yet.

Planned support:
- MySQL/MariaDB
- SQLite
- MongoDB (via community plugin)

For now, if you need other databases, use Axum directly or contribute database support to the project.

---

## How do I deploy this to production?

acton-service produces a standard Rust binary. Deploy like any other application:

**Docker**:
```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/my-service /usr/local/bin/
CMD ["my-service"]
```

**Kubernetes**: Standard Deployment with:
- Liveness probe: `GET /health`
- Readiness probe: `GET /ready`

**Systemd**: Install binary and create service file.

See the [deployment guide](/docs/deployment) for detailed instructions.

---

## How do I test services built with acton-service?

**Unit tests**: Test handlers in isolation using mock `AppState`.

**Integration tests**: Use `testcontainers` for real database/cache:
```rust
#[tokio::test]
async fn test_create_user() {
    let container = testcontainers::postgres::Postgres::default();
    // ... test with real database
}
```

**End-to-end tests**: Use `reqwest` or `hyper` to call actual endpoints:
```rust
#[tokio::test]
async fn test_api_endpoint() {
    let response = reqwest::get("http://localhost:8080/api/v1/users")
        .await?;
    assert_eq!(response.status(), 200);
}
```

---

## What's the minimum Rust version required?

acton-service requires **Rust 1.75 or later**.

The framework uses modern Rust features:
- Async/await
- `impl Trait` in return position
- Generic associated types (GATs)

Update Rust with: `rustup update stable`

---

## How do I contribute?

Contributions are welcome! Check the [contribution guide](https://github.com/Govcraft/acton-service/blob/main/CONTRIBUTING.md).

Good first issues:
- Documentation improvements
- Example applications
- Feature flag refinements
- Database adapter support

---

## Where can I get help?

- [GitHub Discussions](https://github.com/Govcraft/acton-service/discussions) - Ask questions
- [GitHub Issues](https://github.com/Govcraft/acton-service/issues) - Report bugs
- [API Documentation](https://docs.rs/acton-service) - Detailed API reference
- [Examples](/docs/examples) - Working code samples
- [Troubleshooting](/docs/troubleshooting) - Common issues and solutions

---

## Is there a community or Discord?

Not yet! The project is still growing. For now, use:
- GitHub Discussions for questions
- GitHub Issues for bugs
- Pull requests for contributions

If there's demand, we'll create a Discord or community forum.

---

## What license is this under?

acton-service is licensed under **MIT OR Apache-2.0**.

You can choose either license for your use case. Most Rust projects use this dual license.

---

## Can I use this for commercial projects?

**Yes!** The MIT and Apache-2.0 licenses allow commercial use without restrictions.

You don't need to:
- Open source your application
- Pay licensing fees
- Attribute in your UI (though it's appreciated!)

Just include the license file as required by the terms.
