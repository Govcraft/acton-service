---
title: Installation
nextjs:
  metadata:
    title: Installation
    description: Install acton-service and set up your first microservice
---

Get acton-service installed and ready for development in minutes. Choose between adding it to an existing project or scaffolding a complete service with the CLI.

---

## Prerequisites

Before installing acton-service, ensure you have:

- **Rust 1.84 or later** - acton-service requires recent Rust features for type-enforced versioning
- **Cargo** - Rust's package manager (comes with Rust)

Verify your installation:

```bash
rustc --version  # Should show 1.84.0 or later
cargo --version
```

---

## Method 1: Add to Existing Project

Add acton-service to your `Cargo.toml` with the features you need:

```toml
[dependencies]
acton-service = { version = "0.2", features = ["http", "observability"] }
tokio = { version = "1", features = ["full"] }
```

### Common Feature Combinations

**Minimal HTTP Service**
```toml
acton-service = { version = "0.2", features = ["http"] }
```

**HTTP with Database**
```toml
acton-service = { version = "0.2", features = ["http", "database", "observability"] }
```

**Full-Featured Service**
```toml
acton-service = { version = "0.2", features = [
    "http",          # Axum HTTP framework
    "grpc",          # Tonic gRPC support
    "database",      # PostgreSQL via SQLx
    "cache",         # Redis connection pooling
    "events",        # NATS JetStream
    "observability", # Structured logging
    "governor",      # Advanced rate limiting
    "cedar-authz",   # AWS Cedar policy-based authorization
] }
```

**Everything Enabled**
```toml
acton-service = { version = "0.2", features = ["full"] }
```

See the [Feature Flags guide](/docs/feature-flags) for detailed information on choosing features.

---

## Method 2: Scaffold New Service with CLI

The `acton` CLI generates production-ready services with configured features and best practices:

### Install the CLI

```bash
cargo install acton-cli
```

### Create a New Service

**Quick Start** (minimal service):
```bash
acton service new my-api --yes
cd my-api && cargo run
```

**Full-Featured Service** (with database, cache, and events):
```bash
acton service new user-service \
    --http \
    --database postgres \
    --cache redis \
    --events nats \
    --observability

cd user-service && cargo run
```

The CLI generates:
- Complete project structure with `src/main.rs`
- Pre-configured `Cargo.toml` with selected features
- Health check endpoints (`/health`, `/ready`)
- OpenTelemetry observability setup
- XDG-compliant configuration scaffolding

---

## Verify Installation

After installing, verify everything works by creating a minimal service:

```rust
// src/main.rs
use acton_service::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/hello", get(|| async { "Hello, acton-service!" }))
        })
        .build_routes();

    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await
}
```

Run the service:

```bash
cargo run
```

Test the endpoints:

```bash
# Test your API
curl http://localhost:8080/api/v1/hello
# Output: Hello, acton-service!

# Verify health checks
curl http://localhost:8080/health
# Output: {"status":"healthy"}

curl http://localhost:8080/ready
# Output: {"status":"ready"}
```

If all three commands return successful responses, your installation is working correctly.

---

## Next Steps

Now that you have acton-service installed:

- **[Quickstart Guide](/docs/quickstart)** - Build your first versioned API in 5 minutes
- **[Tutorial](/docs/tutorial)** - Complete step-by-step guide to building a production service
- **[Examples](/docs/examples)** - See common patterns and production features in action
- **[Feature Flags](/docs/feature-flags)** - Decision tree for choosing the right features
- **[Configuration](/docs/configuration)** - Set up environment and file-based configuration

---

## Troubleshooting

### Rust Version Too Old

If you see compilation errors about missing features, ensure Rust 1.84 or later:

```bash
rustup update stable
rustc --version
```

### Feature Flag Errors

If you see errors about missing features, verify your `Cargo.toml` includes the required flags:

```toml
# ❌ Missing features
acton-service = "0.2"

# ✅ With required features
acton-service = { version = "0.2", features = ["http"] }
```

### CLI Installation Issues

If `cargo install acton-cli` fails, try:

```bash
# Update cargo
cargo install --locked acton-cli

# Or install from git
cargo install --git https://github.com/your-org/acton-cli
```

For more help, see the [Troubleshooting guide](/docs/troubleshooting) or [open an issue](https://github.com/your-org/acton-service/issues).
