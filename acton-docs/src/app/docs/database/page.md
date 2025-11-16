---
title: Database (PostgreSQL)
nextjs:
  metadata:
    title: Database (PostgreSQL)
    description: PostgreSQL integration with SQLx, connection pooling, health checks, and automatic dependency monitoring for production services
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


Integrate PostgreSQL databases with compile-time checked queries, automatic connection pooling, and built-in health monitoring.

## Overview

acton-service provides production-ready PostgreSQL integration through SQLx with automatic connection pool management, health checks, and compile-time query verification. Database connections are managed automatically through the `AppState` with zero configuration required for development.

## Installation

Enable the database feature:

```toml
[dependencies]
acton-service = { version = "0.2", features = ["database", "http", "observability"] }
```

## Configuration

Database configuration follows XDG standards with environment variable overrides:

```toml
# ~/.config/acton-service/my-service/config.toml
[database]
url = "postgres://username:password@localhost/mydb"
max_connections = 50
optional = false  # Readiness fails if database is unavailable
```

### Environment Variable Override

```bash
ACTON_DATABASE_URL=postgres://localhost/mydb cargo run
```

### Connection Pool Settings

The framework uses SQLx's connection pool with sensible production defaults:

- **max_connections**: Maximum number of connections in the pool (default: 50)
- **min_connections**: Minimum idle connections maintained (default: 10)
- **connect_timeout**: Maximum time to wait for connection (default: 30s)
- **idle_timeout**: Time before idle connections are closed (default: 10m)

## Basic Usage

Access the database through `AppState` in your handlers:

```rust
use acton_service::prelude::*;

#[derive(Serialize)]
struct User {
    id: i64,
    name: String,
    email: String,
}

async fn list_users(State(state): State<AppState>) -> Result<Json<Vec<User>>> {
    let db = state.db().await.ok_or_else(|| Error::Internal("Database unavailable".to_string()))?;

    let users = sqlx::query_as!(
        User,
        "SELECT id, name, email FROM users ORDER BY name"
    )
    .fetch_all(db)
    .await?;

    Ok(Json(users))
}

#[tokio::main]
async fn main() -> Result<()> {
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/users", get(list_users))
        })
        .build_routes();

    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await
}
```

## Compile-Time Query Verification

{% callout type="note" title="SQLx's Killer Feature" %}
SQLx can verify your SQL queries against your actual database schema **during compilation**, catching SQL errors before runtime. This requires additional setup but is highly recommended for production services.
{% /callout %}

### How It Works

When you use `query!` or `query_as!` macros, SQLx connects to your database during `cargo build` and:
1. Parses your SQL query
2. Executes `DESCRIBE` to get column types from your schema
3. Generates type-safe Rust code matching your database schema
4. Catches mismatches (wrong table name, missing column, type errors) at compile time

**Example compile-time error:**
```rust
sqlx::query_as!(User, "SELECT id, nmae FROM users")
//                              ^^^^ typo caught at compile time!
// error: no such column: nmae
```

### Setup for Local Development

**Step 1: Ensure PostgreSQL is running**
```bash
# macOS (Homebrew)
brew services start postgresql@16

# Linux (systemd)
sudo systemctl start postgresql

# Docker
docker run -d -p 5432:5432 -e POSTGRES_PASSWORD=postgres postgres:16
```

**Step 2: Create your development database**
```bash
createdb myapp_dev

# Or with psql
psql -U postgres -c "CREATE DATABASE myapp_dev;"
```

**Step 3: Set DATABASE_URL environment variable**

```bash
# Add to ~/.bashrc or ~/.zshrc for permanent effect
export DATABASE_URL="postgres://username:password@localhost/myapp_dev"

# Or use .env file (requires dotenv)
echo "DATABASE_URL=postgres://localhost/myapp_dev" > .env
```

**Step 4: Run migrations (if using)**

```bash
sqlx migrate run
```

**Step 5: Build with compile-time verification**

```bash
cargo build
# SQLx connects to DATABASE_URL and verifies all queries
```

### Offline Mode (For CI/CD)

For environments without database access (CI runners, laptops without PostgreSQL), use **offline mode**:

**Step 1: Prepare offline data (requires database connection)**

```bash
# Connect to dev database and generate offline metadata
cargo sqlx prepare

# Creates sqlx-data.json with query metadata
# Commit this file to version control
```

**Step 2: Build without database**

```bash
# Set offline mode
export SQLX_OFFLINE=true

# Build succeeds without DATABASE_URL
cargo build
```

**Step 3: Keep offline data up-to-date**

Regenerate `sqlx-data.json` whenever queries or schema change:

```bash
# After modifying queries or running migrations
cargo sqlx prepare

# Commit the updated file
git add sqlx-data.json
git commit -m "chore: update SQLx offline data"
```

### CI/CD Configuration

**GitHub Actions:**
```yaml
- name: Build with SQLx offline mode
  env:
    SQLX_OFFLINE: true
  run: cargo build --release
```

**GitLab CI:**
```yaml
build:
  script:
    - export SQLX_OFFLINE=true
    - cargo build --release
```

### When Compile-Time Verification Fails

**Problem:** Build fails because DATABASE_URL is unavailable or points to wrong database.

**Solutions:**

1. **Use offline mode** (recommended for CI/CD)
   ```bash
   export SQLX_OFFLINE=true
   cargo build
   ```

2. **Point to development database**
   ```bash
   export DATABASE_URL="postgres://localhost/dev_db"
   cargo build
   ```

3. **Skip verification temporarily** (NOT recommended)
   ```rust
   // Use query() instead of query!() - no compile-time checks
   sqlx::query("SELECT * FROM users")  // Runtime errors possible!
   ```

### Dynamic Queries (When NOT to Use Macros)

Compile-time macros require static SQL strings. For dynamic queries, use runtime query builders:

```rust
// ❌ Won't compile - query must be static string literal
let table = "users";
sqlx::query!(format!("SELECT * FROM {}", table))  // ERROR

// ✅ Use runtime query() for dynamic SQL
let table = "users";
let query = format!("SELECT id, name FROM {}", table);
sqlx::query(&query)
    .fetch_all(&pool)
    .await?
```

### Best Practices

**DO:**
- ✅ Use `query!`/`query_as!` for static queries (99% of cases)
- ✅ Keep `sqlx-data.json` in version control
- ✅ Regenerate offline data after schema migrations
- ✅ Use `SQLX_OFFLINE=true` in CI/CD

**DON'T:**
- ❌ Skip compile-time checks unless absolutely necessary
- ❌ Forget to update offline data after migrations
- ❌ Commit sensitive credentials in DATABASE_URL (use localhost in docs)

## Health Checks

Database health is automatically monitored by the `/ready` endpoint:

```toml
[database]
optional = false  # Service not ready if database is down
```

The readiness probe executes a simple query to verify connectivity:

```bash
curl http://localhost:8080/ready
# Returns 200 OK if database is healthy
# Returns 503 Service Unavailable if database is down
```

### Kubernetes Integration

```yaml
readinessProbe:
  httpGet:
    path: /ready
    port: 8080
  initialDelaySeconds: 5
  periodSeconds: 5
  failureThreshold: 3
```

## Transactions

Handle database transactions using SQLx transaction support:

```rust
async fn create_user_with_profile(
    State(state): State<AppState>,
    Json(request): Json<CreateUserRequest>,
) -> Result<Json<User>> {
    let db = state.db().await.ok_or_else(|| Error::Internal("Database unavailable".to_string()))?;

    let mut tx = db.begin().await?;

    // Insert user
    let user = sqlx::query_as!(
        User,
        "INSERT INTO users (name, email) VALUES ($1, $2) RETURNING id, name, email",
        request.name,
        request.email
    )
    .fetch_one(&mut *tx)
    .await?;

    // Insert profile
    sqlx::query!(
        "INSERT INTO profiles (user_id, bio) VALUES ($1, $2)",
        user.id,
        request.bio
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(user))
}
```

## Migrations

SQLx supports embedded migrations for deployment automation:

```rust
use sqlx::migrate;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;
    let state = AppState::builder()
        .config(config.clone())
        .build()
        .await?;

    // Run migrations on startup
    let db = state.db().await.ok_or_else(|| Error::Internal("Database unavailable".to_string()))?;
    migrate!("./migrations")
        .run(db)
        .await?;

    // Start service
    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await
}
```

Create migrations using the SQLx CLI:

```bash
cargo install sqlx-cli --no-default-features --features postgres
sqlx migrate add create_users_table
```

## Error Handling

Database errors are automatically mapped to appropriate HTTP status codes:

```rust
use acton_service::prelude::*;

async fn get_user(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<User>> {
    let db = state.db().await.ok_or_else(|| Error::Internal("Database unavailable".to_string()))?;

    let user = sqlx::query_as!(User, "SELECT * FROM users WHERE id = $1", id)
        .fetch_optional(db)
        .await?
        .ok_or_else(|| {
            Error::NotFound(format!("User {} not found", id))
        })?;

    Ok(Json(user))
}
```

## Connection Pool Monitoring

Monitor connection pool health through application metrics:

```rust
use acton_service::prelude::*;

async fn pool_stats(State(state): State<AppState>) -> Result<Json<PoolStats>> {
    let db = state.db().await.ok_or_else(|| Error::Internal("Database unavailable".to_string()))?;

    let stats = PoolStats {
        size: db.size(),
        idle: db.num_idle(),
    };

    Ok(Json(stats))
}
```

## Best Practices

### Always Use Prepared Statements

```rust
// ✅ Good - parameterized query
sqlx::query!("SELECT * FROM users WHERE id = $1", user_id)
    .fetch_one(db)
    .await?;

// ❌ Bad - SQL injection risk
sqlx::query(&format!("SELECT * FROM users WHERE id = {}", user_id))
    .fetch_one(db)
    .await?;
```

### Configure Connection Limits

Set appropriate connection pool sizes based on your deployment:

```toml
[database]
max_connections = 50  # Adjust based on your database server capacity
```

### Handle Optional Database

For services that can degrade gracefully:

```toml
[database]
optional = true  # Service remains ready even if database is down
```

```rust
async fn cached_data(State(state): State<AppState>) -> Result<Json<Data>> {
    // Try database first, fall back to cache
    if let Some(db) = state.db().await {
        if let Ok(data) = fetch_from_db(db).await {
            return Ok(Json(data));
        }
    }

    // Fallback to cache
    let cache = state.redis().await.ok_or_else(|| Error::Internal("Cache unavailable".to_string()))?;
    let data = fetch_from_cache(cache).await?;
    Ok(Json(data))
}
```

### Use Transactions for Multi-Step Operations

Always use transactions when multiple queries must succeed or fail together:

```rust
let mut tx = db.begin().await?;
// Multiple queries...
tx.commit().await?;
```

## Production Deployment

### Environment Configuration

```bash
# Production environment
export ACTON_DATABASE_URL=postgres://user:pass@db.prod.example.com/mydb
export ACTON_DATABASE_MAX_CONNECTIONS=100
```

### Kubernetes Secret Integration

```yaml
env:
  - name: ACTON_DATABASE_URL
    valueFrom:
      secretKeyRef:
        name: db-credentials
        key: url
```

### Connection Pooling Strategy

For services running multiple replicas, calculate per-instance pool size:

```text
Max DB Connections = 200
Service Replicas = 4
Max Connections per Instance = 200 / 4 = 50
```

```toml
[database]
max_connections = 50
```

## Related Features

- **[Health Checks](/docs/health-checks)** - Automatic database health monitoring
- **[Cache (Redis)](/docs/cache)** - Complement database with Redis caching
- **[Configuration](/docs/configuration)** - Environment and file-based configuration
