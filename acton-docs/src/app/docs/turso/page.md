---
title: Turso (libsql)
nextjs:
  metadata:
    title: Turso (libsql)
    description: Turso/libsql database integration with local, remote, and embedded replica modes for edge-compatible SQLite
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See [Database (PostgreSQL)](/docs/database) for comparison with the PostgreSQL integration.
{% /callout %}

---

Build applications with Turso, the edge-friendly SQLite database. Supports local development, cloud deployment, and hybrid embedded replicas.

---

## Overview

acton-service provides production-ready Turso/libsql integration with:

- **Three connection modes** - Local, Remote, and EmbeddedReplica
- **Automatic retry logic** - Exponential backoff for connection failures
- **Actor-based management** - Connection lifecycle handled by `TursoDbAgent`
- **Encryption support** - AES-256-CBC encryption for local databases
- **Health monitoring** - Automatic health checks for readiness probes

{% callout type="note" title="What is Turso?" %}
[Turso](https://turso.tech) is a SQLite-compatible database built on libsql that runs at the edge. It offers the simplicity of SQLite with the scalability of cloud databases, making it ideal for edge deployments, mobile backends, and applications requiring low latency.
{% /callout %}

---

## Installation

Enable the Turso feature:

```toml
[dependencies]
{% $dep.tursoOnly %}
```

Or add to existing features:

```toml
[dependencies]
{% dep(["http", "turso", "observability"]) %}
```

{% callout type="warning" title="One primary backend at a time" %}
The `database` (PostgreSQL), `turso` (libsql), and `surrealdb` features are **pairwise mutually exclusive** - enabling more than one is a compile error. Pick a single primary backend. The `clickhouse` feature is analytical and composes with any of them. See [SurrealDB](/docs/surrealdb) and [Database (PostgreSQL)](/docs/database).
{% /callout %}

---

## Connection Modes

Turso supports three connection modes for different use cases:

### Local Mode

Pure SQLite - no network required. Ideal for development and testing.

```toml
# config.toml
[turso]
mode = "local"
path = "./data/app.db"
```

### Remote Mode

Connect directly to Turso cloud or a libsql-server instance.

```toml
# config.toml
[turso]
mode = "remote"
url = "libsql://your-database.turso.io"
auth_token = "your-auth-token"
```

### Embedded Replica Mode

Local SQLite that syncs with remote Turso. Best of both worlds - local speed with cloud durability.

```toml
# config.toml
[turso]
mode = "embedded_replica"
path = "./data/replica.db"
url = "libsql://your-database.turso.io"
auth_token = "your-auth-token"
sync_interval_secs = 60
read_your_writes = true
```

---

## Configuration

Full configuration options:

```toml
# config.toml
[turso]
# Connection mode: "local", "remote", or "embedded_replica"
mode = "local"

# Local database file path (required for local and embedded_replica)
path = "./data/app.db"

# Remote database URL (required for remote and embedded_replica)
# Format: libsql://your-db.turso.io or http://localhost:8080
url = "libsql://your-database.turso.io"

# Authentication token (required for remote and embedded_replica)
auth_token = "your-auth-token"

# Sync interval in seconds (embedded_replica only)
# Enables automatic background sync when set
sync_interval_secs = 60

# Encryption key for local database (optional, all modes)
# Uses AES-256-CBC encryption
encryption_key = "your-32-char-encryption-key-here"

# Read-your-writes consistency (embedded_replica only)
# When true, writes are visible locally before sync completes
read_your_writes = true

# Maximum retry attempts for connection (default: 5)
max_retries = 5

# Delay between retry attempts in seconds (default: 2)
retry_delay_secs = 2

# Whether database is optional (default: false)
# If true, service can start without database connection
optional = false

# Lazy initialization (default: true)
# If true, connection is established on first use
lazy_init = true
```

### Environment Variable Overrides

```bash
# Override connection settings
ACTON_TURSO_MODE=remote
ACTON_TURSO_URL=libsql://your-db.turso.io
ACTON_TURSO_AUTH_TOKEN=your-token

# For production, use secrets management
export ACTON_TURSO_AUTH_TOKEN=$(vault read -field=token secret/turso)
```

---

## Basic Usage

Access Turso through `AppState`:

```rust
use acton_service::prelude::*;

async fn get_users(
    State(state): State<AppState>,
) -> Result<Json<Vec<User>>> {
    // Get the Turso database connection
    let db = state.turso().await
        .ok_or_else(|| Error::Internal("Turso not available".into()))?;

    // Get a connection from the database
    let conn = db.connect()
        .map_err(|e| Error::Internal(format!("Connection failed: {}", e)))?;

    // Execute queries - libsql::Error converts into Error::Database automatically
    let mut rows = conn
        .query("SELECT id, name, email FROM users", ())
        .await?;

    let mut users = Vec::new();
    while let Some(row) = rows.next().await? {
        users.push(User {
            id: row.get(0)?,
            name: row.get(1)?,
            email: row.get(2)?,
        });
    }

    Ok(Json(users))
}
```

---

## Query Patterns

### Parameterized Queries

```rust
// Positional parameters
conn.execute(
    "INSERT INTO users (name, email) VALUES (?1, ?2)",
    ["Alice", "alice@example.com"],
).await?;

// Query with parameters
let mut rows = conn
    .query("SELECT * FROM users WHERE id = ?1", [user_id])
    .await?;
```

### Batch Operations

```rust
// Execute multiple statements
conn.execute_batch(r#"
    CREATE TABLE IF NOT EXISTS users (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL,
        email TEXT UNIQUE NOT NULL,
        created_at TEXT DEFAULT CURRENT_TIMESTAMP
    );
    CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);
"#).await?;
```

### Transactions

```rust
// Start a transaction
let tx = conn.transaction().await?;

// Perform operations
tx.execute("UPDATE accounts SET balance = balance - ?1 WHERE id = ?2", [amount, from_id]).await?;
tx.execute("UPDATE accounts SET balance = balance + ?1 WHERE id = ?2", [amount, to_id]).await?;

// Commit
tx.commit().await?;
```

---

## Embedded Replica Sync

When using `embedded_replica` mode, you can control synchronization:

```rust
// Force immediate sync with remote
db.sync().await?;

// Sync is also triggered automatically based on sync_interval_secs
```

### Read-Your-Writes

With `read_your_writes = true`, local writes are immediately visible:

```rust
// Insert locally
conn.execute("INSERT INTO messages (content) VALUES (?1)", ["Hello"]).await?;

// Immediately readable (before sync completes)
let mut rows = conn.query("SELECT * FROM messages WHERE content = 'Hello'", ()).await?;
assert!(rows.next().await?.is_some()); // Works!
```

---

## Encryption

Enable encryption for local databases:

```toml
[turso]
mode = "local"
path = "./data/encrypted.db"
encryption_key = "your-32-character-encryption-key"
```

The database uses AES-256-CBC encryption. The key should be exactly 32 characters.

{% callout type="warning" title="Key Management" %}
Store encryption keys securely using environment variables or a secrets manager. Never commit encryption keys to source control.
{% /callout %}

---

## Error Handling

Turso errors include helpful troubleshooting guidance:

```rust
match state.turso().await {
    Some(db) => {
        match db.connect() {
            Ok(conn) => {
                // Use connection
            }
            Err(e) => {
                // Error includes troubleshooting tips
                tracing::error!("Connection failed: {}", e);
            }
        }
    }
    None => {
        // Database not configured or optional and unavailable
    }
}
```

`libsql::Error` converts into the framework's `Error::Database(DatabaseError)` automatically, so a bare `?`
inside a handler is usually all you need. `DatabaseError` carries structured context (`operation`, `kind`,
`message`, `context`); construct one explicitly when you want to attach your own:

```rust
// DatabaseError, DatabaseErrorKind, and DatabaseOperation are all in the prelude
let db = state.turso().await
    .ok_or_else(|| Error::Database(DatabaseError::connection_failed("Turso not configured")))?;
```

Error categories with automatic detection:
- **Authentication errors** - Invalid or expired auth token
- **Network errors** - Connectivity issues to Turso cloud
- **Permission errors** - File/database permission issues
- **Not found errors** - Database doesn't exist
- **Timeout errors** - Connection or query timeout
- **Corruption errors** - Database file corruption

---

## Health Checks

Turso health is automatically monitored:

```toml
[turso]
optional = false  # Service not ready if Turso is unavailable
```

The `/ready` endpoint verifies Turso connectivity:

```bash
curl http://localhost:8080/ready
# Returns 200 OK if Turso is healthy
# Returns 503 Service Unavailable if Turso is down
```

---

## Migration from SQLite

Turso is SQLite-compatible, so existing SQLite databases work directly:

```toml
# Just point to your existing SQLite database
[turso]
mode = "local"
path = "./existing-sqlite.db"
```

### Moving to Cloud

1. Start with local mode during development
2. Create a Turso database: `turso db create myapp`
3. Switch to embedded_replica for gradual migration
4. Move to remote mode for full cloud operation

```toml
# Development
[turso]
mode = "local"
path = "./dev.db"

# Staging (hybrid)
[turso]
mode = "embedded_replica"
path = "./replica.db"
url = "libsql://myapp-staging.turso.io"
auth_token = "..."

# Production (cloud)
[turso]
mode = "remote"
url = "libsql://myapp.turso.io"
auth_token = "..."
```

---

## Comparison: Primary Backends

acton-service ships three primary database backends. They are **mutually exclusive** - enable exactly one
of `database`, `turso`, or `surrealdb`.

| Feature | Turso (`turso`) | PostgreSQL (`database`) | SurrealDB (`surrealdb`) |
|---------|-----------------|-------------------------|-------------------------|
| **Driver** | `libsql` | `sqlx` | `surrealdb` (`any` engine) |
| **Deployment** | Edge, embedded, cloud | Server-based | Embedded (`mem://`), server (`ws://`, `http://`) |
| **Latency** | Sub-millisecond (local) | Network dependent | In-process or network dependent |
| **Query language** | SQLite-compatible SQL | Full PostgreSQL SQL | SurrealQL |
| **Accessor** | `state.turso().await` | `state.db().await` | `state.surrealdb().await` |
| **Best for** | Edge apps, mobile backends | Complex queries, ACID | Multi-model / graph-shaped data |

**Use Turso when:**
- Building edge or mobile applications
- Need local-first with cloud sync
- Want SQLite simplicity with cloud durability
- Latency is critical

**Use PostgreSQL when:**
- Need advanced SQL features (CTEs, window functions)
- Have complex relational data
- Require strong ACID guarantees across distributed writes

**Use SurrealDB when:**
- Your data is document- or graph-shaped rather than strictly relational
- You want an embedded (`mem://`) store for tests and a server for production behind one URL scheme
- SurrealQL suits your access patterns better than SQL

{% callout type="note" title="ClickHouse composes" %}
The `clickhouse` feature is an *analytical* backend, not a primary one - it can be enabled alongside any of
the three above. See [ClickHouse (Analytics)](/docs/clickhouse).
{% /callout %}

---

## Complete Example

```rust
use acton_service::prelude::*;

#[derive(Debug, Serialize)]
struct Todo {
    id: i64,
    title: String,
    completed: bool,
}

async fn list_todos(
    State(state): State<AppState>,
) -> Result<Json<Vec<Todo>>> {
    let db = state.turso().await
        .ok_or_else(|| Error::Internal("Database not available".into()))?;
    let conn = db.connect()?;

    let mut rows = conn
        .query("SELECT id, title, completed FROM todos ORDER BY id", ())
        .await?;

    let mut todos = Vec::new();
    while let Some(row) = rows.next().await? {
        todos.push(Todo {
            id: row.get(0)?,
            title: row.get(1)?,
            completed: row.get::<i64>(2)? != 0,
        });
    }

    Ok(Json(todos))
}

async fn create_todo(
    State(state): State<AppState>,
    Json(input): Json<CreateTodo>,
) -> Result<Json<Todo>> {
    let db = state.turso().await
        .ok_or_else(|| Error::Internal("Database not available".into()))?;
    let conn = db.connect()?;

    conn.execute(
        "INSERT INTO todos (title, completed) VALUES (?1, 0)",
        [&input.title],
    ).await?;

    // Get the inserted row
    let mut rows = conn
        .query("SELECT id, title, completed FROM todos WHERE rowid = last_insert_rowid()", ())
        .await?;

    let row = rows
        .next()
        .await?
        .ok_or_else(|| Error::Internal("Insert failed".into()))?;

    Ok(Json(Todo {
        id: row.get(0)?,
        title: row.get(1)?,
        completed: false,
    }))
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;
    init_tracing(&config)?;

    // Initialize database schema
    let state = AppState::builder()
        .config(config.clone())
        .build()
        .await?;

    if let Some(db) = state.turso().await {
        let conn = db.connect()?;
        conn.execute_batch(r#"
            CREATE TABLE IF NOT EXISTS todos (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                completed INTEGER DEFAULT 0
            );
        "#).await?;
    }

    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router
                .route("/todos", get(list_todos).post(create_todo))
        })
        .build_routes();

    ServiceBuilder::new()
        .with_config(config)
        .with_routes(routes)
        .build()
        .serve()
        .await
}
```

---

## Troubleshooting

### Connection Refused (Remote Mode)

**Cause**: Invalid URL or network issues.

**Solution**: Verify URL format and network connectivity:

```bash
# Test connectivity
curl -I https://your-db.turso.io

# Verify URL format
# Correct: libsql://your-db.turso.io
# Wrong: https://your-db.turso.io
```

### Authentication Failed

**Cause**: Invalid or expired auth token.

**Solution**: Generate a new token:

```bash
turso db tokens create your-database
```

### Database File Locked

**Cause**: Multiple processes accessing local database.

**Solution**: Ensure only one process accesses the local file, or use remote mode for multi-process access.

### Sync Failures (Embedded Replica)

**Cause**: Network issues or auth token expired.

**Solution**: Check logs for specific error, verify token is valid:

```bash
# Check token expiration
turso db tokens list your-database
```

---

## Related Features

- **[Database (PostgreSQL)](/docs/database)** - PostgreSQL integration
- **[SurrealDB](/docs/surrealdb)** - Multi-model backend (mutually exclusive with `turso`)
- **[ClickHouse (Analytics)](/docs/clickhouse)** - Analytical store that composes with any primary backend
- **[Health Checks](/docs/health-checks)** - Automatic database health monitoring
- **[Configuration](/docs/configuration)** - Environment-based configuration
- **[Reactive Architecture](/docs/reactive-architecture)** - Actor-based connection management
