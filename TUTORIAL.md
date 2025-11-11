# Your First Service - Complete Tutorial

This tutorial walks you through building a production-ready User Management API from scratch.

**What you'll build**:
- RESTful API with versioned endpoints
- PostgreSQL database integration
- JWT authentication
- Health and readiness checks
- Comprehensive error handling
- OpenAPI documentation

**Time**: 30-45 minutes

## Prerequisites

- Rust 1.70+ installed
- PostgreSQL installed (or Docker)
- Basic understanding of REST APIs and async Rust

## Part 1: Project Setup (5 minutes)

### Create the project

```bash
cargo new user-api
cd user-api
```

### Add dependencies

Edit `Cargo.toml`:

```toml
[package]
name = "user-api"
version = "0.1.0"
edition = "2021"

[dependencies]
acton-service = { version = "0.3", features = [
    "http",
    "observability",
    "database"
] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
```

### Verify setup

```bash
cargo check
```

You should see:
```
    Checking user-api v0.1.0
    Finished dev [unoptimized + debuginfo]
```

## Part 2: Basic Service (5 minutes)

### Create the minimal service

Replace `src/main.rs`:

```rust
use acton_service::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Build versioned routes
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/status", get(status))
        })
        .build_routes();

    // Build and serve
    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await
}

async fn status() -> &'static str {
    "User API v1 - Running"
}
```

### Run it

```bash
cargo run
```

### Test it

In another terminal:

```bash
# Your endpoint
curl http://localhost:8080/api/v1/status
# Output: User API v1 - Running

# Automatic health check
curl http://localhost:8080/health
# Output: {"status":"healthy"}
```

🎉 **You now have a running microservice with health checks!**

## Part 3: Add User Model (5 minutes)

### Create the User type

Add to `src/main.rs` (before `main`):

```rust
#[derive(Debug, Serialize, Deserialize, Clone)]
struct User {
    id: u64,
    username: String,
    email: String,
    created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CreateUserRequest {
    username: String,
    email: String,
}
```

### Add handler functions

```rust
// List all users
async fn list_users() -> Json<Vec<User>> {
    Json(vec![
        User {
            id: 1,
            username: "alice".to_string(),
            email: "alice@example.com".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
        },
        User {
            id: 2,
            username: "bob".to_string(),
            email: "bob@example.com".to_string(),
            created_at: "2025-01-02T00:00:00Z".to_string(),
        },
    ])
}

// Get single user
async fn get_user(Path(id): Path<u64>) -> Result<Json<User>, StatusCode> {
    // For now, return mock data
    if id == 1 {
        Ok(Json(User {
            id: 1,
            username: "alice".to_string(),
            email: "alice@example.com".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
        }))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

// Create user
async fn create_user(
    Json(req): Json<CreateUserRequest>,
) -> (StatusCode, Json<User>) {
    // For now, return mock response
    (
        StatusCode::CREATED,
        Json(User {
            id: 3,
            username: req.username,
            email: req.email,
            created_at: "2025-01-10T00:00:00Z".to_string(),
        }),
    )
}
```

### Update routes

Replace the `add_version` section:

```rust
.add_version(ApiVersion::V1, |router| {
    router
        .route("/status", get(status))
        .route("/users", get(list_users).post(create_user))
        .route("/users/{id}", get(get_user))
})
```

### Test the new endpoints

```bash
# Restart your service (Ctrl+C then cargo run)

# List users
curl http://localhost:8080/api/v1/users

# Get specific user
curl http://localhost:8080/api/v1/users/1

# Create user
curl -X POST http://localhost:8080/api/v1/users \
  -H "Content-Type: application/json" \
  -d '{"username":"charlie","email":"charlie@example.com"}'
```

## Part 4: Add Database (10 minutes)

### Start PostgreSQL

Using Docker:

```bash
docker run -d \
  --name user-api-db \
  -e POSTGRES_PASSWORD=secret \
  -e POSTGRES_DB=userapi \
  -p 5432:5432 \
  postgres:16
```

Or use your local PostgreSQL installation.

### Create config file

Create `config.toml` in your project root:

```toml
[service]
name = "user-api"
port = 8080
log_level = "info"

[database]
url = "postgres://postgres:secret@localhost:5432/userapi"
max_connections = 10
optional = false
```

### Create database schema

Connect to your database:

```bash
psql -h localhost -U postgres -d userapi
# Password: secret
```

Create the users table:

```sql
CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    username VARCHAR(255) UNIQUE NOT NULL,
    email VARCHAR(255) UNIQUE NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Insert test data
INSERT INTO users (username, email) VALUES
    ('alice', 'alice@example.com'),
    ('bob', 'bob@example.com');

-- Verify
SELECT * FROM users;
```

### Update handlers to use database

Replace the handler functions:

```rust
// List all users (with database)
async fn list_users(State(state): State<AppState>) -> Result<Json<Vec<User>>> {
    let db = state.database()?;

    let users: Vec<User> = sqlx::query_as!(
        User,
        r#"
        SELECT
            id as "id!: u64",
            username,
            email,
            created_at::text as created_at
        FROM users
        ORDER BY id
        "#
    )
    .fetch_all(db)
    .await
    .map_err(|e| {
        error!("Database error: {}", e);
        Error::DatabaseError(e.to_string())
    })?;

    Ok(Json(users))
}

// Get single user (with database)
async fn get_user(
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<Json<User>> {
    let db = state.database()?;

    let user = sqlx::query_as!(
        User,
        r#"
        SELECT
            id as "id!: u64",
            username,
            email,
            created_at::text as created_at
        FROM users
        WHERE id = $1
        "#,
        id as i64
    )
    .fetch_optional(db)
    .await
    .map_err(|e| {
        error!("Database error: {}", e);
        Error::DatabaseError(e.to_string())
    })?
    .ok_or(Error::NotFound("User not found".to_string()))?;

    Ok(Json(user))
}

// Create user (with database)
async fn create_user(
    State(state): State<AppState>,
    Json(req): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<User>)> {
    let db = state.database()?;

    let user = sqlx::query_as!(
        User,
        r#"
        INSERT INTO users (username, email)
        VALUES ($1, $2)
        RETURNING
            id as "id!: u64",
            username,
            email,
            created_at::text as created_at
        "#,
        req.username,
        req.email
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        error!("Database error: {}", e);
        Error::DatabaseError(e.to_string())
    })?;

    Ok((StatusCode::CREATED, Json(user)))
}
```

### Test with real database

```bash
# Restart your service
cargo run

# List users (from database)
curl http://localhost:8080/api/v1/users

# Create new user (writes to database)
curl -X POST http://localhost:8080/api/v1/users \
  -H "Content-Type: application/json" \
  -d '{"username":"charlie","email":"charlie@example.com"}'

# Verify it was created
curl http://localhost:8080/api/v1/users
```

## Part 5: Add API Versioning (5 minutes)

### Create V2 with improved response format

Add a new response type:

```rust
#[derive(Debug, Serialize)]
struct UserListResponse {
    users: Vec<User>,
    total: usize,
    version: &'static str,
}
```

### Add V2 handlers

```rust
async fn list_users_v2(State(state): State<AppState>) -> Result<Json<UserListResponse>> {
    let db = state.database()?;

    let users: Vec<User> = sqlx::query_as!(
        User,
        r#"
        SELECT
            id as "id!: u64",
            username,
            email,
            created_at::text as created_at
        FROM users
        ORDER BY id
        "#
    )
    .fetch_all(db)
    .await
    .map_err(|e| Error::DatabaseError(e.to_string()))?;

    let total = users.len();

    Ok(Json(UserListResponse {
        users,
        total,
        version: "2.0",
    }))
}
```

### Add V2 to routes and deprecate V1

Update your routes in `main()`:

```rust
let routes = VersionedApiBuilder::new()
    .with_base_path("/api")
    // V1: Deprecated
    .add_version_deprecated(
        ApiVersion::V1,
        |router| {
            router
                .route("/status", get(status))
                .route("/users", get(list_users).post(create_user))
                .route("/users/{id}", get(get_user))
        },
        DeprecationInfo::new(ApiVersion::V1, ApiVersion::V2)
            .with_sunset_date("2026-12-31T23:59:59Z")
            .with_message("V1 is deprecated. Please migrate to V2 for improved response format."),
    )
    // V2: Current stable
    .add_version(ApiVersion::V2, |router| {
        router
            .route("/status", get(status))
            .route("/users", get(list_users_v2).post(create_user))
            .route("/users/{id}", get(get_user))
    })
    .build_routes();
```

### Test both versions

```bash
# V1 (deprecated) - check headers
curl -I http://localhost:8080/api/v1/users
# Look for: Deprecation, Sunset, Link headers

# V2 (current)
curl http://localhost:8080/api/v2/users
# Output: {"users":[...],"total":3,"version":"2.0"}
```

## Part 6: Add Authentication (5 minutes)

### Add JWT middleware

Update your `add_version` for V2 to include auth:

```rust
use acton_service::middleware::JwtAuth;

// In main(), before routes:
let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "dev-secret-key".to_string());
let auth_layer = JwtAuth::new(&jwt_secret);

// Update V2 routes to add auth
.add_version(ApiVersion::V2, |router| {
    router
        .route("/status", get(status))
        // Public endpoint
        .route("/users", get(list_users_v2))
        // Protected endpoints
        .route("/users", post(create_user))
        .route("/users/{id}", get(get_user))
        .layer(auth_layer)
})
```

### Generate a test JWT

For testing, use https://jwt.io/ or create a token:

```bash
# Example token (HS256, secret: "dev-secret-key")
export TOKEN="eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"
```

### Test authenticated endpoints

```bash
# Public endpoint (no auth needed)
curl http://localhost:8080/api/v2/users

# Protected endpoint without auth (should fail)
curl -X POST http://localhost:8080/api/v2/users \
  -H "Content-Type: application/json" \
  -d '{"username":"dave","email":"dave@example.com"}'
# Output: 401 Unauthorized

# Protected endpoint with auth (should succeed)
curl -X POST http://localhost:8080/api/v2/users \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{"username":"dave","email":"dave@example.com"}'
```

## Part 7: Better Error Handling (5 minutes)

### Use acton-service response types

Update `create_user` to use built-in response types:

```rust
use acton_service::responses::{Created, ValidationError};

async fn create_user(
    State(state): State<AppState>,
    Json(req): Json<CreateUserRequest>,
) -> Result<Created<User>> {
    // Validate input
    if req.username.is_empty() {
        return Err(Error::ValidationError("Username cannot be empty".to_string()));
    }
    if !req.email.contains('@') {
        return Err(Error::ValidationError("Invalid email format".to_string()));
    }

    let db = state.database()?;

    let user = sqlx::query_as!(
        User,
        r#"
        INSERT INTO users (username, email)
        VALUES ($1, $2)
        RETURNING
            id as "id!: u64",
            username,
            email,
            created_at::text as created_at
        "#,
        req.username,
        req.email
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        if e.to_string().contains("duplicate key") {
            Error::ConflictError("Username or email already exists".to_string())
        } else {
            Error::DatabaseError(e.to_string())
        }
    })?;

    Ok(Created(user))
}
```

### Test error handling

```bash
# Empty username
curl -X POST http://localhost:8080/api/v2/users \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{"username":"","email":"test@example.com"}'

# Invalid email
curl -X POST http://localhost:8080/api/v2/users \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{"username":"test","email":"invalidemail"}'

# Duplicate username
curl -X POST http://localhost:8080/api/v2/users \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{"username":"alice","email":"alice2@example.com"}'
```

## Part 8: Production Readiness (5 minutes)

### Add observability features

Update `Cargo.toml`:

```toml
acton-service = { version = "0.3", features = [
    "http",
    "observability",
    "database",
    "resilience",      # Add circuit breaker, retry
    "otel-metrics"     # Add metrics
] }
```

### Enable structured logging

```bash
RUST_LOG=info cargo run
```

You'll now see structured JSON logs for every request.

### Add production config

Create `~/.config/acton-service/user-api/config.toml`:

```toml
[service]
name = "user-api"
port = 8080
log_level = "info"

[database]
url = "postgres://postgres:secret@db.production.com:5432/userapi"
max_connections = 50
min_connections = 10
timeout_seconds = 30

[observability]
service_name = "user-api"
otlp_endpoint = "http://otel-collector:4317"
```

### Test health endpoints

```bash
# Liveness (is the process running?)
curl http://localhost:8080/health

# Readiness (are dependencies healthy?)
curl http://localhost:8080/ready
# If database is down, this returns 503
```

## What You've Built

Congratulations! You now have a production-ready microservice with:

✅ **Versioned REST API** (V1 and V2)
✅ **PostgreSQL database** with connection pooling
✅ **JWT authentication** on protected endpoints
✅ **Comprehensive error handling** with validation
✅ **Health and readiness checks** for Kubernetes
✅ **Structured logging** in JSON format
✅ **API deprecation** with proper headers
✅ **Type-safe routing** enforced at compile time

## Next Steps

### Add more features

1. **Update endpoint**: Add `PUT /users/{id}`
2. **Delete endpoint**: Add `DELETE /users/{id}`
3. **Pagination**: Add query params for list endpoint
4. **Filtering**: Add search by username or email
5. **Rate limiting**: Add `governor` feature

### Deploy to production

See the [README.md](./README.md) for Kubernetes deployment examples.

### Explore advanced features

- **gRPC support**: Add `grpc` feature for dual-protocol
- **Redis caching**: Add `cache` feature
- **Event streaming**: Add `events` feature for NATS
- **OpenAPI docs**: Add `openapi` feature

## Full Code Reference

See the complete working example at:
- [examples/users-api.rs](./acton-service/examples/users-api.rs)

## Need Help?

- [TROUBLESHOOTING.md](./TROUBLESHOOTING.md) - Common issues and solutions
- [FEATURE_FLAGS.md](./FEATURE_FLAGS.md) - Understanding feature flags
- [GitHub Issues](https://github.com/Govcraft/acton-service/issues)
