---
title: SurrealDB
nextjs:
  metadata:
    title: SurrealDB
    description: SurrealDB integration with runtime protocol selection, in-memory and server modes, retry logic, and health monitoring
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See [Database (PostgreSQL)](/docs/database) and [Turso (libsql)](/docs/turso) for the other primary backends.
{% /callout %}

---

Build services on SurrealDB, the multi-model database. One URL scheme selects the protocol at runtime - embed it in-process for tests, point at a server in production.

---

## Overview

acton-service provides SurrealDB integration with:

- **Runtime protocol selection** - the `any` engine picks the transport from the URL scheme
- **Automatic retry logic** - exponential backoff for connection failures
- **Actor-based management** - connection lifecycle handled by `SurrealDbAgent`
- **Root authentication** - optional username/password signin
- **Health monitoring** - reported through the readiness probe
- **Append-only audit storage** - SurrealDB is a supported audit backend

The client type is `SurrealClient`, an alias for `surrealdb::Surreal<surrealdb::engine::any::Any>`.

---

## Installation

Enable the `surrealdb` feature:

```toml
[dependencies]
{% $dep.surrealdbOnly %}
```

Or add it to an existing feature set:

```toml
[dependencies]
{% dep(["http", "surrealdb", "observability"]) %}
```

{% callout type="warning" title="One primary backend at a time" %}
The `database` (PostgreSQL), `turso` (libsql), and `surrealdb` features are **pairwise mutually exclusive** - enabling two of them is a compile error. Pick a single primary backend. The `clickhouse` feature is analytical and composes with any of them.
{% /callout %}

---

## Connection Modes

The URL scheme determines the transport - there is no separate mode setting:

| Scheme | Transport | Typical use |
|--------|-----------|-------------|
| `mem://` | In-process, in-memory | Tests, local development |
| `ws://` / `wss://` | WebSocket | Production servers |
| `http://` / `https://` | HTTP | Production servers, restricted networks |

```toml
# In-memory, no server required
[surrealdb]
url = "mem://"
```

```toml
# WebSocket against a running SurrealDB server
[surrealdb]
url = "ws://localhost:8000"
username = "root"
password = "root"
```

---

## Configuration

Full configuration options:

```toml
# config.toml
[surrealdb]
# Connection URL - the scheme selects the protocol (required)
url = "ws://localhost:8000"

# Namespace to use (default: "default")
namespace = "myapp"

# Database to use (default: "default")
database = "production"

# Root username for authentication (optional)
username = "root"

# Root password for authentication (optional)
password = "root"

# Maximum retry attempts for the connection (default: 5)
max_retries = 5

# Delay between retry attempts in seconds (default: 2)
retry_delay_secs = 2

# Whether the database is optional (default: false)
# If true, the service can start without a SurrealDB connection
optional = false

# Lazy initialization (default: true)
# If true, the connection is established in the background after startup
lazy_init = true
```

Authentication is only attempted when **both** `username` and `password` are set; the client signs in as a
root user, then selects the configured namespace and database. Retries use exponential backoff, doubling
`retry_delay_secs` on each attempt up to `max_retries`.

### Environment Variable Overrides

```bash
ACTON_SURREALDB_URL=ws://surreal.prod.example.com:8000
ACTON_SURREALDB_NAMESPACE=myapp
ACTON_SURREALDB_DATABASE=production

# Pull the password from a secrets manager in production
export ACTON_SURREALDB_PASSWORD=$(vault read -field=password secret/surrealdb)
```

---

## Basic Usage

Access SurrealDB through `AppState`. `state.surrealdb()` is **async** and returns
`Option<Arc<SurrealClient>>` - it is `None` when SurrealDB is not configured or not yet connected.

```rust
use acton_service::prelude::*;

#[derive(Debug, Serialize, Deserialize)]
struct User {
    name: String,
    email: String,
}

async fn list_users(
    State(state): State<AppState>,
) -> Result<Json<Vec<User>>> {
    let db = state
        .surrealdb()
        .await
        .ok_or_else(|| Error::Internal("SurrealDB not available".into()))?;

    let users: Vec<User> = db
        .query("SELECT name, email FROM users ORDER BY name")
        .await?
        .take(0)?;

    Ok(Json(users))
}
```

`surrealdb::Error` converts into the framework's `Error::Database(DatabaseError)` automatically, so a bare
`?` works inside handlers.

### Parameterized Queries

Bind parameters instead of interpolating them into SurrealQL:

```rust
async fn find_user(
    State(state): State<AppState>,
    Path(email): Path<String>,
) -> Result<Json<Option<User>>> {
    let db = state
        .surrealdb()
        .await
        .ok_or_else(|| Error::Internal("SurrealDB not available".into()))?;

    let user: Option<User> = db
        .query("SELECT name, email FROM users WHERE email = $email LIMIT 1")
        .bind(("email", email))
        .await?
        .take(0)?;

    Ok(Json(user))
}
```

### Creating Records

```rust
async fn create_user(
    State(state): State<AppState>,
    Json(input): Json<User>,
) -> Result<Json<User>> {
    let db = state
        .surrealdb()
        .await
        .ok_or_else(|| Error::Internal("SurrealDB not available".into()))?;

    let created: Option<User> = db
        .query("CREATE users CONTENT $data")
        .bind(("data", input))
        .await?
        .take(0)?;

    created
        .map(Json)
        .ok_or_else(|| Error::Internal("Insert returned no record".into()))
}
```

`take(n)` selects the result set of the nth statement, so a multi-statement `query()` can be unpacked one
statement at a time.

---

## Health Checks

SurrealDB health is reported through `SurrealDbHealth` in the pool health summary, which backs the `/ready`
endpoint. It carries the sanitized URL (credentials stripped), the namespace, the database, and whether the
client is connected.

```toml
[surrealdb]
optional = false  # Service is not ready until SurrealDB connects
```

```bash
curl http://localhost:8080/ready
# Returns 200 OK if SurrealDB is healthy
# Returns 503 Service Unavailable if it is down
```

Set `optional = true` to let the service stay ready while SurrealDB is unavailable - `state.surrealdb()`
simply returns `None`, and handlers decide how to degrade.

---

## Audit Storage

With the `audit` and `surrealdb` features enabled, SurrealDB is a supported append-only audit backend.
The `audit_events` table is defined with immutability enforced at the database level:

```sql
DEFINE TABLE IF NOT EXISTS audit_events SCHEMAFUL
    PERMISSIONS
        FOR select FULL
        FOR create FULL
        FOR update NONE
        FOR delete NONE;
```

Update and delete are denied by the database itself, so the hash-chained audit log cannot be rewritten
through the application. See [Audit Logging](/docs/audit) for the event model and configuration.

---

## Error Handling

Connection errors are categorized with troubleshooting guidance attached to the message:

- **Authentication errors** - bad credentials or a user without permission to sign in
- **Network errors** - the server is unreachable, DNS fails, or the connection is refused
- **Permission errors** - the user cannot access the requested namespace or database
- **Not found errors** - the namespace or database does not exist
- **Timeout errors** - the server is overloaded or unresponsive

URLs are sanitized before logging, so credentials embedded in a connection URL are never written to logs.

---

## Testing with `mem://`

The in-memory engine needs no server, which makes it a good fit for integration tests:

```toml
# config.test.toml
[surrealdb]
url = "mem://"
namespace = "test"
database = "test"
lazy_init = false  # Connect during startup so tests fail fast
```

{% callout type="note" title="No root user on mem://" %}
The embedded `mem://` engine has no default root user - leave `username` and `password` unset when using it. Provision users out of band on real (`ws://`, `http://`) deployments.
{% /callout %}

---

## Related Features

- **[Database (PostgreSQL)](/docs/database)** - PostgreSQL integration (mutually exclusive with `surrealdb`)
- **[Turso (libsql)](/docs/turso)** - Edge SQLite integration (mutually exclusive with `surrealdb`)
- **[ClickHouse (Analytics)](/docs/clickhouse)** - Analytical store that composes with any primary backend
- **[Audit Logging](/docs/audit)** - Append-only audit events with SurrealDB storage
- **[Health Checks](/docs/health-checks)** - Automatic database health monitoring
- **[Reactive Architecture](/docs/reactive-architecture)** - Actor-based connection management
