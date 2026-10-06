# Acton-Service Examples

This directory contains organized examples demonstrating various features and capabilities of acton-service. Examples are categorized by feature to make it easier to find what you need.

## Getting Started

New to acton-service? Start with the [Basic Examples](#basic-examples).

## Categories

### 📚 [Basic Examples](./basic/)

Simple getting-started examples demonstrating core functionality:

- **[simple-api.rs](./basic/simple-api.rs)** - Zero-configuration versioned API with automatic health checks
- **[users-api.rs](./basic/users-api.rs)** - Multi-version API demonstrating version evolution and deprecation
- **[ping-pong.rs](../../acton-service-integration-tests/examples/basic/ping-pong.rs)** - Simple request/response example

**Best for**: First-time users, understanding basic patterns

### 🤖 Agent Examples

Reactive agent-based components:

- **[background-worker.rs](./background-worker.rs)** - Managed background task execution with tracking, cancellation, and graceful shutdown

**Best for**: Understanding the reactive agent system, background task management

### 🔐 [Authorization](./authorization/)

Fine-grained access control using AWS Cedar policies:

- **[cedar-authz.rs](./authorization/cedar-authz.rs)** - JWT authentication + Cedar policy-based authorization
- Complete example with policies, test scripts, and documentation

**Best for**: Implementing role-based or attribute-based access control

**See**: [Authorization README](./authorization/README.md) for detailed setup and testing instructions

### 🔌 [gRPC Examples](./grpc/)

gRPC service integration:

- **[single-port.rs](../../acton-service-integration-tests/examples/grpc/single-port.rs)** - HTTP REST + gRPC on a single port with automatic protocol detection

**Best for**: Building services that need both REST and gRPC interfaces

### 📨 [Event-Driven Architecture](./events/)

Event bus patterns and asynchronous communication:

- **[event-driven.rs](../../acton-service-integration-tests/examples/events/event-driven.rs)** - HTTP API + gRPC service communicating via event bus

**Best for**: Building decoupled services with async event handling

### 📊 [Observability](./observability/)

Metrics, tracing, and monitoring:

- **[test-metrics.rs](./observability/test-metrics.rs)** - Prometheus metrics integration
- **[test-observability.rs](./observability/test-observability.rs)** - OpenTelemetry tracing setup

**Best for**: Production observability, debugging, and monitoring

### 📋 [Templates](./templates/)

Configuration and build templates for your own projects:

- **[config.toml.example](./templates/config.toml.example)** - Service configuration template
- **[build.rs.example](./templates/build.rs.example)** - Build script for proto compilation

**Best for**: Starting a new project or understanding configuration options

## Running Examples

Run facade examples with `cargo run --example <name>`. The gRPC examples live in
the private integration harness so their protobuf build does not slow ordinary
framework checks; select that package explicitly as shown below:

```bash
# Basic examples
cargo run --manifest-path=acton-service/Cargo.toml --example simple-api
cargo run --manifest-path=acton-service/Cargo.toml --example users-api
cargo run -p acton-service-integration-tests --example ping-pong --features grpc

# Agent examples
cargo run --manifest-path=acton-service/Cargo.toml --example background-worker

# Authorization (requires features)
cargo run --manifest-path=acton-service/Cargo.toml --example cedar-authz --features cedar-authz,cache

# gRPC (requires features)
cargo run -p acton-service-integration-tests --example single-port --features grpc

# Events (requires features)
cargo run -p acton-service-integration-tests --example event-driven --features grpc

# Observability (requires features)
cargo run --manifest-path=acton-service/Cargo.toml --example test-metrics --features observability
cargo run --manifest-path=acton-service/Cargo.toml --example test-observability --features observability
```

## Feature Flags

Some examples require specific feature flags:

| Feature | Required For | Description |
|---------|-------------|-------------|
| `cedar-authz` | Authorization examples | AWS Cedar policy-based authorization |
| `cache` | Cedar with caching | Redis caching for policy decisions |
| `grpc` | gRPC examples, ping-pong | tonic gRPC server support |
| `otel-metrics` | test-metrics example | OpenTelemetry metrics collection |
| `observability` | test-observability example | OpenTelemetry tracing setup |

## Learning Path

Recommended order for learning acton-service:

1. **Start**: [simple-api.rs](./basic/simple-api.rs) - Understand basic service setup
2. **Versioning**: [users-api.rs](./basic/users-api.rs) - Learn API version management
3. **Authorization**: [cedar-authz.rs](./authorization/cedar-authz.rs) - Add access control
4. **Advanced**: Explore gRPC, events, and observability as needed

## Need Help?

- Check the README in each category directory for detailed information
- See the main [acton-service README](../../README.md) for general documentation
- Each example file contains inline documentation explaining what it demonstrates

## Contributing

When adding new examples:

1. Place them in the appropriate category directory
2. Include comprehensive inline documentation
3. Update this README with a brief description
4. Update the category README if adding to an existing category
5. Consider adding a dedicated README for complex multi-file examples
