---
title: Database (PostgreSQL)
nextjs:
  metadata:
    title: Database (PostgreSQL)
    description: PostgreSQL integration with SQLx, connection pooling, health checks, and automatic dependency monitoring for production services
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

SQLx provides compile-time checked queries using the `query!` and `query_as!` macros:

```rust
// Compile-time verified query - type mismatches caught at build time
async fn get_user_by_id(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
) -> Result<Json<User>> {
    let db = state.db().await.ok_or_else(|| Error::Internal("Database unavailable".to_string()))?;

    let user = sqlx::query_as!(
        User,
        "SELECT id, name, email FROM users WHERE id = $1",
        user_id
    )
    .fetch_one(db)
    .await?;

    Ok(Json(user))
}
```

To enable compile-time verification, set the `DATABASE_URL` environment variable:

```bash
export DATABASE_URL=postgres://localhost/mydb
cargo build
```

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
