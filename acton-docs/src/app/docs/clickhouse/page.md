---
title: ClickHouse (Analytics)
nextjs:
  metadata:
    title: ClickHouse (Analytics)
    description: ClickHouse analytical database integration for event analytics, time-series data, audit storage, and high-volume append-only workloads
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---

Integrate ClickHouse as a complementary analytical database for event analytics, time-series data, and high-volume append-only workloads.

---

## Overview

acton-service provides ClickHouse integration through the `clickhouse` crate with automatic client management, health checks, and a framework-level `AnalyticsWriter` trait for structured data ingestion. ClickHouse connections are managed automatically through the `AppState` with zero configuration required for development.

{% callout type="note" title="Composable, Not Exclusive" %}
Unlike the primary database backends (PostgreSQL, Turso, SurrealDB) which are **mutually exclusive**, the `clickhouse` feature is **composable** — you can use it alongside any primary database. This mirrors how `cache` (Redis) and `events` (NATS) work. ClickHouse serves as your analytical/OLAP store while your primary database handles OLTP workloads.
{% /callout %}

{% callout type="note" title="Agent-Managed Client" %}
The ClickHouse client is managed internally by a **ClickHousePoolAgent** that handles connection lifecycle, health monitoring, and graceful shutdown. You interact with the client via `state.clickhouse()` — the agent works transparently behind the scenes. See [Reactive Architecture](/docs/reactive-architecture) for implementation details.
{% /callout %}

## Installation

Enable the clickhouse feature:

```toml
[dependencies]
{% $dep.clickhouse %}
```

Combine with your primary database:

```toml
[dependencies]
{% $dep.clickhouseDatabase %}
```

## Configuration

ClickHouse configuration follows XDG standards with environment variable overrides:

```toml
# ~/.config/acton-service/my-service/config.toml
[clickhouse]
url = "http://localhost:8123"
database = "default"
username = "default"
password = ""
optional = false  # Readiness fails if ClickHouse is unavailable
```

### Environment Variable Override

```bash
ACTON_CLICKHOUSE_URL=http://localhost:8123 cargo run
```

### Connection Settings

The framework manages a lightweight HTTP client with sensible defaults:

- **url**: ClickHouse HTTP interface URL (default port: 8123)
- **database**: Target database name (default: `"default"`)
- **username**: Authentication username (optional)
- **password**: Authentication password (optional)
- **max_retries**: Connection retry attempts with exponential backoff (default: 5)
- **retry_delay_secs**: Base delay between retries in seconds (default: 2)
- **optional**: Whether the service can start without ClickHouse (default: false)
- **lazy_init**: Initialize connection in background on startup (default: true)

{% callout type="note" title="No Connection Pool Needed" %}
Unlike PostgreSQL or Redis, ClickHouse uses an HTTP-based protocol. The `clickhouse::Client` handles request multiplexing internally and is cheaply clonable — no connection pool is needed.
{% /callout %}

## Basic Usage

Access the ClickHouse client through `AppState` in your handlers:

```rust
use acton_service::prelude::*;
use clickhouse::Row;

#[derive(Row, Serialize, Deserialize)]
struct PageView {
    timestamp: i64,
    user_id: String,
    path: String,
    duration_ms: u64,
}

async fn record_page_view(
    State(state): State<AppState>,
    Json(view): Json<PageView>,
) -> Result<Json<()>> {
    let client = state.clickhouse().await
        .ok_or_else(|| Error::Internal("ClickHouse unavailable".to_string()))?;

    let mut insert = client.insert("page_views").await?;
    insert.write(&view).await?;
    insert.end().await?;

    Ok(Json(()))
}

#[tokio::main]
async fn main() -> Result<()> {
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/page-views", post(record_page_view))
        })
        .build_routes();

    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await
}
```

## AnalyticsWriter Trait

The framework provides an `AnalyticsWriter<T>` trait that gives you a standard pattern for writing analytical data to ClickHouse tables. This is the analytics equivalent of the `Repository` trait for CRUD operations.

```rust
use acton_service::prelude::*;
use clickhouse::Row;

#[derive(Row, Serialize)]
struct OrderEvent {
    timestamp: i64,
    order_id: String,
    customer_id: String,
    total_cents: i64,
    item_count: u32,
}

struct OrderAnalytics {
    client: clickhouse::Client,
}

impl AnalyticsWriter<OrderEvent> for OrderAnalytics {
    fn client(&self) -> &clickhouse::Client {
        &self.client
    }

    fn table_name(&self) -> &str {
        "order_events"
    }
}
```

### Writing Single Events

```rust
async fn on_order_placed(analytics: &OrderAnalytics, order: &Order) -> Result<()> {
    let event = OrderEvent {
        timestamp: chrono::Utc::now().timestamp_millis(),
        order_id: order.id.to_string(),
        customer_id: order.customer_id.to_string(),
        total_cents: order.total_cents,
        item_count: order.items.len() as u32,
    };

    analytics.write_one(event).await
}
```

### Batch Writes

For high-throughput scenarios, batch writes are significantly more efficient:

```rust
async fn flush_events(analytics: &OrderAnalytics, events: Vec<OrderEvent>) -> Result<()> {
    // Batches are sent as a single HTTP request to ClickHouse
    analytics.write_batch(events).await
}
```

## Querying Data

Use the ClickHouse client directly for analytical queries:

```rust
use clickhouse::Row;

#[derive(Row, Deserialize)]
struct DailySummary {
    date: String,
    total_orders: u64,
    total_revenue_cents: i64,
}

async fn daily_revenue(
    State(state): State<AppState>,
) -> Result<Json<Vec<DailySummary>>> {
    let client = state.clickhouse().await
        .ok_or_else(|| Error::Internal("ClickHouse unavailable".to_string()))?;

    let summaries = client
        .query(
            "SELECT
                toDate(fromUnixTimestamp64Milli(timestamp)) AS date,
                count() AS total_orders,
                sum(total_cents) AS total_revenue_cents
            FROM order_events
            WHERE timestamp >= subtractDays(now64(3), 30)
            GROUP BY date
            ORDER BY date DESC"
        )
        .fetch_all::<DailySummary>()
        .await?;

    Ok(Json(summaries))
}
```

## Audit Storage Backend

ClickHouse is an excellent backend for [Audit Logging](/docs/audit) — its append-only MergeTree engine naturally enforces immutability, and columnar storage compresses audit data efficiently.

Enable both features:

```toml
[dependencies]
{% $dep.clickhouseAudit %}
```

Initialize the audit storage:

```rust
use acton_service::audit::storage::clickhouse_impl::ClickHouseAuditStorage;

let client = state.clickhouse().await
    .ok_or_else(|| Error::Internal("ClickHouse unavailable".to_string()))?;
let storage = ClickHouseAuditStorage::new(client);
storage.initialize().await?;  // Creates audit_events table
```

The ClickHouse audit backend creates a `audit_events` table with:
- **MergeTree engine** with `ORDER BY (timestamp, sequence)` for fast time-range queries
- **Monthly partitioning** via `PARTITION BY toYYYYMM(timestamp)` for efficient cleanup
- **No immutability rules needed** — ClickHouse MergeTree tables do not support standard UPDATE/DELETE operations

## Health Checks

ClickHouse health is automatically monitored by the `/ready` endpoint:

```toml
[clickhouse]
optional = false  # Service not ready if ClickHouse is down
```

The readiness probe executes `SELECT 1` to verify connectivity:

```bash
curl http://localhost:8080/ready
# Returns 200 OK if ClickHouse is healthy
# Returns 503 Service Unavailable if ClickHouse is down
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

## Table Design Patterns

### Time-Series Events

```sql
CREATE TABLE events (
    timestamp DateTime64(3, 'UTC'),
    event_type String,
    user_id String,
    payload String
) ENGINE = MergeTree()
ORDER BY (event_type, timestamp)
PARTITION BY toYYYYMM(timestamp)
```

### Pre-Aggregated Materialized Views

```sql
CREATE MATERIALIZED VIEW hourly_counts
ENGINE = SummingMergeTree()
ORDER BY (event_type, hour)
AS SELECT
    event_type,
    toStartOfHour(timestamp) AS hour,
    count() AS event_count
FROM events
GROUP BY event_type, hour
```

## Error Handling

ClickHouse errors map to HTTP 500 with an `ANALYTICS_ERROR` code. Internal details are not exposed to clients:

```rust
async fn query_analytics(
    State(state): State<AppState>,
) -> Result<Json<Vec<Summary>>> {
    let client = state.clickhouse().await
        .ok_or_else(|| Error::Internal("ClickHouse unavailable".to_string()))?;

    // ClickHouse errors automatically convert to Error::ClickHouse
    // and map to 500 ANALYTICS_ERROR in the response
    let results = client
        .query("SELECT ?fields FROM summary_table")
        .fetch_all::<Summary>()
        .await?;

    Ok(Json(results))
}
```

## Best Practices

### Use Batch Inserts for High Throughput

```rust
// ✅ Good - batch insert (one HTTP request)
analytics.write_batch(events).await?;

// ❌ Bad - individual inserts (N HTTP requests)
for event in events {
    analytics.write_one(event).await?;
}
```

### Design Tables for Query Patterns

ClickHouse performance depends heavily on `ORDER BY` matching your query filters:

```sql
-- ✅ Good - ORDER BY matches WHERE clause
CREATE TABLE events (...) ORDER BY (user_id, timestamp)
-- Efficient: WHERE user_id = 'abc' AND timestamp > ...

-- ❌ Bad - ORDER BY doesn't match queries
CREATE TABLE events (...) ORDER BY (timestamp)
-- Inefficient: WHERE user_id = 'abc' (full scan)
```

### Use Monthly Partitioning for Retention

```sql
-- Efficient data lifecycle management
ALTER TABLE events DROP PARTITION '202401';  -- Drop entire month instantly
```

### Mark ClickHouse as Optional When Appropriate

For services where analytics is supplementary:

```toml
[clickhouse]
optional = true  # Service remains ready even if ClickHouse is down
```

```rust
// Gracefully skip analytics when unavailable
if let Some(client) = state.clickhouse().await {
    let _ = record_analytics(&client, &event).await;
}
```

## Production Deployment

### Environment Configuration

```bash
export ACTON_CLICKHOUSE_URL=http://ch.prod.internal:8123
export ACTON_CLICKHOUSE_DATABASE=analytics
export ACTON_CLICKHOUSE_USERNAME=service_account
export ACTON_CLICKHOUSE_PASSWORD=secret
```

### Kubernetes Secret Integration

```yaml
env:
  - name: ACTON_CLICKHOUSE_URL
    valueFrom:
      secretKeyRef:
        name: clickhouse-credentials
        key: url
  - name: ACTON_CLICKHOUSE_PASSWORD
    valueFrom:
      secretKeyRef:
        name: clickhouse-credentials
        key: password
```

### Architecture Pattern

A typical production setup uses ClickHouse alongside a primary database:

```text
┌─────────────┐     OLTP      ┌──────────────┐
│  Service     │──────────────>│  PostgreSQL   │
│  (acton)     │               │  (primary)    │
│              │     OLAP      ├──────────────┤
│              │──────────────>│  ClickHouse   │
│              │               │  (analytics)  │
└─────────────┘               └──────────────┘
```

## Related Features

- **[Database (PostgreSQL)](/docs/database)** - Primary OLTP database for transactional workloads
- **[Turso (libsql)](/docs/turso)** - Lightweight embedded/edge database alternative
- **[Audit Logging](/docs/audit)** - Immutable audit trails with ClickHouse as a storage backend
- **[Health Checks](/docs/health-checks)** - Automatic ClickHouse health monitoring
- **[Configuration](/docs/configuration)** - Environment and file-based configuration
- **[Events (NATS)](/docs/events)** - Pair with NATS for event-driven analytics pipelines
