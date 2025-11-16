# Acton-Service Examples

This directory contains organized examples demonstrating various features and capabilities of acton-service. Examples are categorized by feature to make it easier to find what you need.

## Getting Started

New to acton-service? Start with the [Basic Examples](#basic-examples).

## Categories

### 📚 [Basic Examples](./basic/)

Simple getting-started examples demonstrating core functionality:

- **[simple-api.rs](./basic/simple-api.rs)** - Zero-configuration versioned API with automatic health checks
- **[users-api.rs](./basic/users-api.rs)** - Multi-version API demonstrating version evolution and deprecation
- **[ping-pong.rs](./basic/ping-pong.rs)** - Simple request/response example

**Best for**: First-time users, understanding basic patterns

### 🔐 [Authorization](./authorization/)

Fine-grained access control using AWS Cedar policies:

- **[cedar-authz.rs](./authorization/cedar-authz.rs)** - JWT authentication + Cedar policy-based authorization
- Complete example with policies, test scripts, and documentation

**Best for**: Implementing role-based or attribute-based access control

**See**: [Authorization README](./authorization/README.md) for detailed setup and testing instructions

### 🔌 [gRPC Examples](./grpc/)

gRPC service integration:

- **[single-port.rs](./grpc/single-port.rs)** - HTTP REST + gRPC on a single port with automatic protocol detection

**Best for**: Building services that need both REST and gRPC interfaces

### 📨 [Event-Driven Architecture](./events/)

Event bus patterns and asynchronous communication:

- **[event-driven.rs](./events/event-driven.rs)** - HTTP API + gRPC service communicating via event bus

**Best for**: Building decoupled microservices with async event handling

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

All examples can be run using `cargo run --example <name>`:

```bash
# Basic examples
cargo run --manifest-path=acton-service/Cargo.toml --example simple-api
cargo run --manifest-path=acton-service/Cargo.toml --example users-api
cargo run --manifest-path=acton-service/Cargo.toml --example ping-pong

# Authorization (requires features)
cargo run --manifest-path=acton-service/Cargo.toml --example cedar-authz --features cedar-authz,cache

# gRPC (requires features)
cargo run --manifest-path=acton-service/Cargo.toml --example single-port --features grpc

# Events (requires features)
cargo run --manifest-path=acton-service/Cargo.toml --example event-driven --features grpc

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
| `grpc` | gRPC examples | tonic gRPC server support |
| `observability` | Observability examples | OpenTelemetry metrics and tracing |

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
