---
title: Troubleshooting
nextjs:
  metadata:
    title: Troubleshooting
    description: Common issues and solutions when working with acton-service
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


Find solutions to common problems when building services with acton-service.

## Compilation Errors

### Expected `VersionedRoutes`, found `Router`

**Full error**:
```text
error[E0308]: mismatched types
  --> src/main.rs:XX:XX
   |
   | ServiceBuilder::new().with_routes(app)
   |                                    ^^^ expected `VersionedRoutes`, found `Router`
```

**Cause**: You're trying to use a raw `Router` instead of `VersionedRoutes`.

**Solution**: Wrap your routes in `VersionedApiBuilder`:

```rust
// ❌ This won't compile
let app = Router::new().route("/hello", get(handler));
ServiceBuilder::new().with_routes(app)

// ✅ Do this instead
let routes = VersionedApiBuilder::new()
    .with_base_path("/api")
    .add_version(ApiVersion::V1, |router| {
        router.route("/hello", get(handler))
    })
    .build_routes();

ServiceBuilder::new().with_routes(routes)
```

### Cannot find type `AppState` in this scope

**Full error**:
```text
error[E0412]: cannot find type `AppState` in this scope
```

**Cause**: Missing import or wrong feature flags.

**Solution**:
```rust
// Add to your imports
use acton_service::prelude::*;

// Or explicitly
use acton_service::state::AppState;

// And ensure you have the right features in Cargo.toml
{% $dep.http %}
```

### Method `database` not found

**Full error**:
```text
error[E0599]: no method named `database` found for struct `AppState`
```

**Cause**: Missing `database` feature flag.

**Solution**: Add the feature to your `Cargo.toml`:
```toml
[dependencies]
{% $dep.database %}
```

### Could not find `tonic` in the list of imported crates

**Cause**: Missing `grpc` feature flag.

**Solution**: Add the feature:
```toml
[dependencies]
{% $dep.grpc %}
```

### Trait `IntoResponse` is not implemented

**Full error**:
```text
error[E0277]: the trait bound `MyType: IntoResponse` is not satisfied
```

**Cause**: Your handler is returning a type that doesn't implement `IntoResponse`.

**Solution**: Wrap it in `Json` or implement `IntoResponse`:

```rust
// ❌ This won't work
async fn handler() -> MyStruct {
    MyStruct { field: "value" }
}

// ✅ Wrap in Json
async fn handler() -> Json<MyStruct> {
    Json(MyStruct { field: "value" })
}

// ✅ Or use tuple for status code + json
async fn handler() -> (StatusCode, Json<MyStruct>) {
    (StatusCode::OK, Json(MyStruct { field: "value" }))
}
```

---

## Type System Issues

### Cannot infer type for type parameter `S`

**Cause**: Axum can't infer the state type for your routes.

**Solution**: Be explicit about state:

```rust
// ❌ Ambiguous
let app = Router::new()
    .route("/", get(handler));

// ✅ Explicit state type
let app: Router<AppState> = Router::new()
    .route("/", get(handler));
```

### Expected `fn` pointer, found closure

**Cause**: Handler function signature doesn't match Axum expectations.

**Solution**: Check your handler signature:

```rust
// ✅ Correct signatures
async fn simple_handler() -> &'static str { "ok" }
async fn json_handler() -> Json<MyType> { Json(value) }
async fn with_path(Path(id): Path<u64>) -> String { format!("ID: {}", id) }
async fn with_state(State(state): State<AppState>) -> String { "ok".to_string() }
```

---

## Runtime Errors

### Failed to bind to address 0.0.0.0:8080

**Cause**: Port 8080 is already in use.

**Solutions**:

1. **Change the port** (environment variable):
   ```bash
   ACTON_SERVICE_PORT=9090 cargo run
   ```

2. **Change the port** (config file):
   ```toml
   [service]
   port = 9090
   ```

3. **Kill the process** using port 8080:
   ```bash
   # Find the process
   lsof -i :8080
   # Kill it
   kill -9 <PID>
   ```

### Database connection failed

**Full error**:
```text
Error: Database connection failed: Connection refused
```

**Cause**: Database is not running or URL is incorrect.

**Solutions**:

1. **Check if database is running**:
   ```bash
   # PostgreSQL
   pg_isready
   # Or check process
   ps aux | grep postgres
   ```

2. **Verify connection URL**:
   ```toml
   [database]
   url = "postgres://user:password@localhost:5432/dbname"
   # Make sure: user, password, host, port, and dbname are correct
   ```

3. **Use optional flag during development**:
   ```toml
   [database]
   url = "postgres://localhost/mydb"
   optional = true  # Service starts even if DB is down
   ```

### JWT validation failed

**Cause**: Invalid JWT token or wrong secret.

**Solutions**:

1. **Check your secret**:
   ```rust
   // Make sure you're using the same secret for signing and validation
   let auth = JwtAuth::new("your-secret-key");
   ```

2. **Verify token format**:
   ```bash
   # Token should be: Bearer <token>
   curl -H "Authorization: Bearer eyJhbGc..." http://localhost:8080/api/v1/protected
   ```

3. **Check token expiration**:
   ```rust
   // Tokens might be expired
   // Check 'exp' claim in your JWT
   ```

---

## Configuration Issues

### Config file not found

**Cause**: Service can't find `config.toml`.

**Solution**: acton-service looks in these locations (in order):

1. `./config.toml` (current directory)
2. `~/.config/acton-service/{service_name}/config.toml`
3. `/etc/acton-service/{service_name}/config.toml`

```bash
# For development, create in current directory
cat > config.toml <<EOF
[service]
name = "my-service"
port = 8080
EOF

# For production
mkdir -p ~/.config/acton-service/my-service
cp config.toml ~/.config/acton-service/my-service/
```

### Environment variables not working

**Issue**: `ACTON_SERVICE_PORT=9090` doesn't change the port.

**Cause**: Environment variable naming or config priority.

**Solution**: Environment variables override config files:

```bash
# Correct format
ACTON_SERVICE_PORT=9090 cargo run

# Also works
export ACTON_SERVICE_PORT=9090
cargo run

# Check all ACTON_ variables
env | grep ACTON
```

---

## Database Issues

### Too many open connections

**Cause**: Database connection pool exhausted.

**Solution**: Configure pool size:

```toml
[database]
url = "postgres://localhost/mydb"
max_connections = 50  # Increase if needed (default: 10)
min_connections = 5   # Minimum pool size
```

### Connection pool timeout

**Cause**: All connections are busy, waiting for one to become available.

**Solutions**:

1. **Increase pool size**:
   ```toml
   [database]
   max_connections = 100
   ```

2. **Increase timeout**:
   ```toml
   [database]
   timeout_seconds = 30
   ```

3. **Check for connection leaks**:
   ```rust
   // Always use connections in short-lived scopes
   {
       let db = state.db().await.ok_or("Database not available")?;
       let result = sqlx::query!("SELECT * FROM users")
           .fetch_all(db)
           .await?;
       // Connection automatically returned to pool here
   }
   ```

### Prepared statement already exists

**Cause**: SQLx query name collision.

**Solution**: Use unique query names or use `query_as!` without naming:

```rust
// ❌ Potential collision
let users = sqlx::query!("SELECT * FROM users").fetch_all(db).await?;

// ✅ Use query_as with explicit naming
let users = sqlx::query_as!(User, "SELECT id, name FROM users")
    .fetch_all(db)
    .await?;
```

---

## Performance Issues

### Slow startup time

**Cause**: Large number of dependencies or debug build.

**Solutions**:

1. **Use release build**:
   ```bash
   cargo build --release
   ./target/release/my-service
   ```

2. **Reduce features**:
   ```toml
   # Only enable what you need
   {% $dep.http %}
   ```

3. **Use lazy initialization**:
   ```toml
   [database]
   lazy_init = true  # Don't connect until first use
   ```

### High memory usage

**Cause**: Too many connections or buffering.

**Solutions**:

1. **Reduce connection pools**:
   ```toml
   [database]
   max_connections = 10  # Reduce from default

   [cache]
   max_pool_size = 5
   ```

2. **Check for memory leaks**:
   ```bash
   # Use valgrind or heaptrack
   valgrind --leak-check=full ./target/debug/my-service
   ```

### Slow request processing

**Cause**: Database queries, blocking operations, or missing indices.

**Solutions**:

1. **Profile your code**:
   ```bash
   # Use cargo-flamegraph
   cargo install flamegraph
   cargo flamegraph --bin my-service
   ```

2. **Check database queries**:
   ```sql
   -- PostgreSQL: Enable query logging
   ALTER DATABASE mydb SET log_statement = 'all';

   -- Look for slow queries
   SELECT query, calls, total_time
   FROM pg_stat_statements
   ORDER BY total_time DESC
   LIMIT 10;
   ```

3. **Use async properly**:
   ```rust
   // ❌ Don't block the async runtime
   async fn handler() {
       std::thread::sleep(Duration::from_secs(1)); // Bad!
   }

   // ✅ Use async sleep
   async fn handler() {
       tokio::time::sleep(Duration::from_secs(1)).await; // Good!
   }
   ```

---

## Development Workflow

### Cannot find example `simple-api`

**Cause**: Running examples from wrong directory.

**Solution**: Run from repository root:

```bash
# From acton-service repository root
cargo run --example simple-api

# Not from acton-service/ subdirectory
```

### Tests fail with "database connection refused"

**Cause**: Test database not running.

**Solutions**:

1. **Use testcontainers** (recommended):
   ```rust
   #[tokio::test]
   async fn test_with_db() {
       // Automatically starts PostgreSQL in Docker
       let container = testcontainers::postgres::Postgres::default();
       // ... test code
   }
   ```

2. **Mock the database**:
   ```rust
   // Use a mock state for unit tests
   let state = AppState::default();
   ```

3. **Set up test database**:
   ```bash
   # Start a test database
   docker run -d -p 5433:5432 -e POSTGRES_PASSWORD=test postgres:16
   # Use different port for tests
   ACTON_DATABASE_URL=postgres://postgres:test@localhost:5433/test cargo test
   ```

### Hot reload not working

**Issue**: Want to auto-restart on code changes.

**Solution**: Use cargo-watch:

```bash
# Install cargo-watch
cargo install cargo-watch

# Auto-reload on changes
cargo watch -x run

# Auto-reload with custom command
cargo watch -x 'run --example simple-api'
```

---

## Still Stuck?

### Enable debug logging

```bash
RUST_LOG=debug cargo run
```

Or set in config:
```toml
[service]
log_level = "debug"
```

### Check the examples

The [examples directory](https://github.com/Govcraft/acton-service/tree/main/acton-service/examples) has working code for most features.

### Get help

- [GitHub Issues](https://github.com/Govcraft/acton-service/issues)
- [API Documentation](https://docs.rs/acton-service)
- Check [Tutorial](/docs/tutorial) for step-by-step guides

### Report a bug

If you've found a bug, please include:

1. Your `Cargo.toml` dependencies
2. Full error message
3. Minimal reproduction code
4. Rust version (`rustc --version`)
5. OS and version

```bash
# Quick diagnostic info
rustc --version
cargo --version
uname -a
```
