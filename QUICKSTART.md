# 5-Minute Quickstart

Get a production-ready microservice running in 5 minutes.

## Prerequisites

- Rust 1.70+ installed
- Basic familiarity with async Rust
- 5 minutes of your time

## Step 1: Create a New Project (30 seconds)

```bash
cargo new my-api
cd my-api
```

## Step 2: Add Dependencies (30 seconds)

Add acton-service to your `Cargo.toml`:

```toml
[dependencies]
acton-service = { version = "0.3", features = ["http", "observability"] }
tokio = { version = "1", features = ["full"] }
```

## Step 3: Write Your First Service (2 minutes)

Replace the contents of `src/main.rs`:

```rust
use acton_service::prelude::*;

// Your first handler
async fn hello() -> &'static str {
    "Hello from acton-service!"
}

#[tokio::main]
async fn main() -> Result<()> {
    // Build versioned routes - versioning is enforced!
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/hello", get(hello))
        })
        .build_routes();

    // Build and serve - config and tracing are automatic!
    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await
}
```

## Step 4: Run It! (30 seconds)

```bash
cargo run
```

You should see:

```
   Compiling my-api v0.1.0
    Finished dev [unoptimized + debuginfo] target(s)
     Running `target/debug/my-api`
```

## Step 5: Test It! (1 minute)

Open another terminal and test your endpoints:

```bash
# Your versioned API
curl http://localhost:8080/api/v1/hello
# Output: Hello from acton-service!

# Automatic health check (for Kubernetes)
curl http://localhost:8080/health
# Output: {"status":"healthy"}

# Automatic readiness check
curl http://localhost:8080/ready
# Output: {"status":"ready"}
```

## What Just Happened?

You just created a production-ready microservice with:

✅ **Type-enforced API versioning** - Impossible to create unversioned routes
✅ **Automatic health checks** - `/health` and `/ready` endpoints for Kubernetes
✅ **Automatic logging** - Structured JSON logs out of the box
✅ **Automatic tracing** - OpenTelemetry tracing ready to go
✅ **Graceful shutdown** - Proper signal handling (SIGTERM, SIGINT)
✅ **Zero configuration** - Works with sensible defaults

## What's Next?

### Add More Endpoints

```rust
async fn get_user(Path(id): Path<u64>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "id": id,
        "name": "Alice"
    }))
}

// Add to your router:
.add_version(ApiVersion::V1, |router| {
    router
        .route("/hello", get(hello))
        .route("/users/{id}", get(get_user))  // New!
})
```

### Add a Second Version

```rust
// V2 with improved response
async fn hello_v2() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "message": "Hello from V2!",
        "version": "2.0"
    }))
}

// Add V2 to your routes:
.add_version(ApiVersion::V1, |router| {
    router.route("/hello", get(hello))
})
.add_version(ApiVersion::V2, |router| {
    router.route("/hello", get(hello_v2))  // New version!
})
```

### Try the Examples

The repository includes comprehensive examples:

```bash
# Clone the repo
git clone https://github.com/Govcraft/acton-service
cd acton-service

# Run the simple API example
cargo run --example simple-api

# Run the users API example (shows deprecation)
cargo run --example users-api

# Run the dual HTTP+gRPC example
cargo run --example ping-pong --features grpc
```

## Common First Questions

**Q: Why can't I just use `Router::new()`?**
A: acton-service enforces versioning at compile time. All routes must be versioned to prevent breaking changes from slipping into production.

**Q: How do I add middleware?**
A: See [TUTORIAL.md](./TUTORIAL.md) for middleware examples.

**Q: How do I connect to a database?**
A: Add the `database` feature and see [TUTORIAL.md](./TUTORIAL.md#adding-a-database).

**Q: Can I disable versioning?**
A: No. If you need unversioned routes, use Axum directly. acton-service is opinionated about API evolution.

## Next Steps

- **[TUTORIAL.md](./TUTORIAL.md)** - Step-by-step guide to building a real service
- **[FEATURE_FLAGS.md](./FEATURE_FLAGS.md)** - Understand which features you need
- **[examples/](./acton-service/examples/)** - Complete working examples
- **[README.md](./README.md)** - Full feature documentation

## Need Help?

- [Troubleshooting Guide](./TROUBLESHOOTING.md)
- [GitHub Issues](https://github.com/Govcraft/acton-service/issues)
- [API Documentation](https://docs.rs/acton-service)

---

**You're now ready to build production microservices!** 🚀
