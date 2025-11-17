---
title: Examples
nextjs:
  metadata:
    title: Examples
    description: Complete code examples demonstrating acton-service features and patterns
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


Browse complete, runnable examples showing how to build different types of services with acton-service. All examples are organized by category in the {% link href=githubUrl("/tree/main/acton-service/examples") %}examples/{% /link %} directory.

---

## Example Categories

Examples are organized by feature and complexity. **New to acton-service?** Start with [Basic Examples](#basic-examples).

### 📚 Basic Examples {#basic-examples}

**Directory**: {% link href=githubUrl("/tree/main/acton-service/examples/basic")}examples/basic/{% /link %}

Simple getting-started examples demonstrating core functionality:

#### **simple-api.rs** - Zero-Configuration Versioned API

The simplest possible service with automatic health checks.

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

Demonstrates:
- Automatic configuration loading
- Type-safe API versioning
- Auto-generated health endpoints (`/health`, `/ready`)
- Built-in tracing and logging

```bash
cargo run --manifest-path=acton-service/Cargo.toml --example simple-api
```

#### **users-api.rs** - Multi-Version API Evolution

Shows how to manage multiple API versions with deprecation headers.

Demonstrates:
- Multiple API versions (V1, V2, V3)
- Automatic deprecation warnings
- API evolution patterns
- Breaking change management

```bash
cargo run --manifest-path=acton-service/Cargo.toml --example users-api
```

#### **ping-pong.rs** - Simple Request/Response

Minimal service template for basic HTTP endpoints.

```bash
cargo run --manifest-path=acton-service/Cargo.toml --example ping-pong --features grpc
```

**Best for**: First-time users, understanding basic patterns

{% link href=githubUrl("/tree/main/acton-service/examples/basic/README.md")}📖 View Basic Examples README{% /link %}

---

### 🔐 Authorization {#authorization}

**Directory**: {% link href=githubUrl("/tree/main/acton-service/examples/authorization")}examples/authorization/{% /link %}

Fine-grained access control using AWS Cedar policies.

#### **cedar-authz.rs** - Policy-Based Authorization

Complete example with JWT authentication + Cedar authorization.

Demonstrates:
- Role-based access control (admin vs user)
- Resource ownership patterns
- JWT + Cedar integration
- Optional Redis caching for policy decisions

```bash
cargo run --manifest-path=acton-service/Cargo.toml --example cedar-authz --features cedar-authz,cache
```

Features auto-setup with:
- `policies.cedar` - Policy definitions
- `jwt-public.pem` - JWT validation key
- `config.toml` - Service configuration

**Best for**: Implementing RBAC or attribute-based access control

{% link href=githubUrl("/tree/main/acton-service/examples/authorization/README.md")}📖 View Authorization README{% /link %} for detailed setup, testing instructions, and policy explanations.

---

### 🔌 gRPC Examples {#grpc}

**Directory**: {% link href=githubUrl("/tree/main/acton-service/examples/grpc")}examples/grpc/{% /link %}

gRPC service integration patterns.

#### **single-port.rs** - HTTP + gRPC on One Port

Run both REST and gRPC on a single port with automatic protocol detection.

Demonstrates:
- Dual-protocol support on port 8080
- Automatic routing based on content-type
- gRPC (`application/grpc`) → tonic services
- All other requests → axum HTTP handlers

```bash
cargo run --manifest-path=acton-service/Cargo.toml --example single-port --features grpc
```

Test HTTP:
```bash
curl http://localhost:8080/api/v1/hello
```

Test gRPC:
```bash
grpcurl -plaintext -d '{"name": "world"}' localhost:8080 hello.HelloService/SayHello
```

**Best for**: Services needing both REST and gRPC interfaces

{% link href=githubUrl("/tree/main/acton-service/examples/grpc/README.md")}📖 View gRPC Examples README{% /link %}

---

### 📨 Event-Driven Architecture {#events}

**Directory**: {% link href=githubUrl("/tree/main/acton-service/examples/events")}examples/events/{% /link %}

Event bus patterns and asynchronous communication.

#### **event-driven.rs** - HTTP API + gRPC with Event Bus

Recommended architecture: HTTP publishes events, gRPC consumes them.

Demonstrates:
- HTTP REST API (port 8080) publishing events
- gRPC service (port 9090) consuming events
- Decoupled microservice communication
- Async event processing

```bash
cargo run --manifest-path=acton-service/Cargo.toml --example event-driven --features grpc
```

Architecture:
```
HTTP Client → REST API → Event Bus → gRPC Service → Business Logic
```

**Best for**: Decoupled microservices, async message processing

{% link href=githubUrl("/tree/main/acton-service/examples/events/README.md")}📖 View Events README{% /link %}

---

### 📊 Observability {#observability}

**Directory**: {% link href=githubUrl("/tree/main/acton-service/examples/observability")}examples/observability/{% /link %}

Metrics, tracing, and monitoring integration.

#### **test-metrics.rs** - Prometheus Metrics

Prometheus metrics collection and custom metric definitions.

```bash
cargo run --manifest-path=acton-service/Cargo.toml --example test-metrics --features otel-metrics
curl http://localhost:8080/metrics
```

#### **test-observability.rs** - OpenTelemetry Tracing

Distributed tracing setup with OpenTelemetry.

```bash
cargo run --manifest-path=acton-service/Cargo.toml --example test-observability --features observability
```

Demonstrates:
- OpenTelemetry initialization
- Span creation and propagation
- Integration with Jaeger/Zipkin
- Structured logging correlation

**Best for**: Production monitoring, debugging, performance analysis

{% link href=githubUrl("/tree/main/acton-service/examples/observability/README.md")}📖 View Observability README{% /link %}

---

### 📋 Templates {#templates}

**Directory**: {% link href=githubUrl("/tree/main/acton-service/examples/templates")}examples/templates/{% /link %}

Configuration and build templates for new projects.

- **config.toml.example** - Complete service configuration template
- **build.rs.example** - Build script for proto compilation

Use these as starting points for your own services:

```bash
cp examples/templates/config.toml.example config.toml
cp examples/templates/build.rs.example build.rs
```

**Best for**: Starting a new project, understanding all configuration options

{% link href=githubUrl("/tree/main/acton-service/examples/templates/README.md")}📖 View Templates README{% /link %}

---

## Running Examples

All examples run from the repository root with updated paths:

```bash
# Basic examples
cargo run --manifest-path=acton-service/Cargo.toml --example simple-api
cargo run --manifest-path=acton-service/Cargo.toml --example users-api
cargo run --manifest-path=acton-service/Cargo.toml --example ping-pong --features grpc

# Authorization (requires features)
cargo run --manifest-path=acton-service/Cargo.toml --example cedar-authz --features cedar-authz,cache

# gRPC (requires features)
cargo run --manifest-path=acton-service/Cargo.toml --example single-port --features grpc

# Events (requires features)
cargo run --manifest-path=acton-service/Cargo.toml --example event-driven --features grpc

# Observability (requires features)
cargo run --manifest-path=acton-service/Cargo.toml --example test-metrics --features otel-metrics
cargo run --manifest-path=acton-service/Cargo.toml --example test-observability --features observability
```

---

## Feature Flags for Examples

Some examples require specific feature flags:

| Feature | Required For | Description |
|---------|-------------|-------------|
| `cedar-authz` | Authorization examples | AWS Cedar policy authorization |
| `cache` | Cedar with caching | Redis caching for policy decisions |
| `grpc` | gRPC examples | tonic gRPC server support |
| `otel-metrics` | test-metrics | OpenTelemetry metrics collection |
| `observability` | test-observability | OpenTelemetry tracing |

---

## Learning Path

Recommended order for exploring acton-service:

1. **Start**: [simple-api.rs](#basic-examples) - Understand basic service setup
2. **Versioning**: [users-api.rs](#basic-examples) - Learn API version management
3. **Authorization**: [cedar-authz.rs](#authorization) - Add access control
4. **Advanced**: Explore [gRPC](#grpc), [events](#events), and [observability](#observability) as needed

---

## Example Structure

Each category includes:
- **README.md** - Detailed category documentation
- **Complete source code** - Runnable examples
- **Inline documentation** - Code comments explaining key concepts
- **Test commands** - Copy/paste curl/grpcurl commands

---

## Next Steps

- Review [Feature Flags](/docs/feature-flags) to understand which features to enable
- Check [Troubleshooting](/docs/troubleshooting) if you encounter issues
- Explore [API Reference](/docs/api-reference) for detailed documentation
