---
title: Request IDs
nextjs:
  metadata:
    title: Request IDs
    description: Type-safe, time-sortable request identifiers using TypeID and UUIDv7 for distributed tracing and log correlation.
---

acton-service uses type-safe request identifiers based on the [TypeID specification](https://github.com/jetify-com/typeid) with UUIDv7 for time-sortability. These identifiers provide human-readable prefixes while maintaining the benefits of UUIDs.

{% callout type="note" title="Automatic Generation" %}
Request IDs are generated automatically for every HTTP request. You don't need to configure anything - they're enabled by default.
{% /callout %}

---

## Format

Request IDs follow the TypeID format: `{prefix}_{base32-encoded-uuid}`

```
req_01h455vb4pex5vsknk084sn02q
└─┬─┘└──────────┬──────────────┘
prefix    base32-encoded UUIDv7
```

### Components

| Part | Description | Example |
|------|-------------|---------|
| Prefix | Type identifier (`req`) | `req` |
| Separator | Underscore | `_` |
| Suffix | Base32-encoded UUIDv7 | `01h455vb4pex5vsknk084sn02q` |

### Why TypeID?

Traditional UUIDs have drawbacks for request tracing:

```
# Standard UUID - no type information, not sortable by time
550e8400-e29b-41d4-a716-446655440000

# TypeID with UUIDv7 - typed, time-sortable, human-readable
req_01h455vb4pex5vsknk084sn02q
```

**Benefits:**
- **Type safety**: The `req_` prefix clearly identifies request IDs
- **Time-sortable**: UUIDv7 includes a timestamp component
- **K-sortable**: IDs created later sort after earlier ones
- **Collision-resistant**: Same uniqueness guarantees as UUIDs
- **URL-safe**: Base32 encoding uses only alphanumeric characters

---

## UUIDv7: Time-Sortable Identifiers

Request IDs use UUIDv7 (RFC 9562) which embeds a Unix timestamp:

```
UUIDv7 structure:
┌────────────────────┬──────────┬────────────────────┐
│  48-bit timestamp  │  4-bit   │  12-bit + 62-bit   │
│  (milliseconds)    │  version │  random            │
└────────────────────┴──────────┴────────────────────┘
```

**Implications for request tracing:**

```rust
// IDs created at different times sort chronologically
let id1 = RequestId::new();  // 10:00:00.000
// ... time passes ...
let id2 = RequestId::new();  // 10:00:00.500

assert!(id1 < id2);  // Time-ordered!
```

This enables:
- **Log sorting**: Sort logs by request ID to get chronological order
- **Request sequencing**: Determine request order without timestamps
- **Time-based querying**: Find requests in a time range by ID prefix

---

## Using Request IDs

### In Request Handlers

Request IDs are automatically added to the request extensions:

```rust
use acton_service::ids::RequestId;
use axum::Extension;

async fn handler(
    Extension(request_id): Extension<RequestId>,
) -> impl IntoResponse {
    // Use the request ID for logging
    tracing::info!(
        request_id = %request_id,
        "Processing request"
    );

    // Include in response for client correlation
    Json(json!({
        "request_id": request_id.to_string(),
        "data": "..."
    }))
}
```

### In Response Headers

Request IDs are automatically included in response headers:

```http
HTTP/1.1 200 OK
x-request-id: req_01h455vb4pex5vsknk084sn02q
content-type: application/json

{"data": "..."}
```

### Creating Request IDs Manually

For testing or custom scenarios:

```rust
use acton_service::ids::RequestId;
use std::str::FromStr;

// Create a new request ID
let id = RequestId::new();
println!("{}", id);  // req_01h455vb4pex5vsknk084sn02q

// Parse an existing request ID
let parsed = RequestId::from_str("req_01h455vb4pex5vsknk084sn02q")?;
assert_eq!(parsed.prefix(), "req");

// Access the underlying string
let id_str: &str = id.as_str();

// Convert to owned String
let owned: String = id.into();
```

---

## Integration with tower-http

acton-service provides a `MakeTypedRequestId` implementation for tower-http:

```rust
use acton_service::ids::MakeTypedRequestId;
use tower_http::request_id::SetRequestIdLayer;

// This is configured automatically, but you can use it manually:
let layer = SetRequestIdLayer::new(
    http::header::HeaderName::from_static("x-request-id"),
    MakeTypedRequestId::default(),
);
```

---

## Log Correlation

Request IDs enable powerful log correlation:

### Structured Logging

```rust
use tracing::{info, instrument};

#[instrument(skip_all, fields(request_id = %request_id))]
async fn process_order(
    request_id: RequestId,
    order: Order,
) -> Result<(), Error> {
    info!("Processing order");

    // All logs in this function include request_id
    validate_order(&order)?;
    save_order(&order).await?;

    info!("Order processed successfully");
    Ok(())
}
```

### Log Output

```json
{
  "timestamp": "2024-01-15T10:30:45.123Z",
  "level": "INFO",
  "message": "Processing order",
  "request_id": "req_01h455vb4pex5vsknk084sn02q",
  "trace_id": "abc123"
}
{
  "timestamp": "2024-01-15T10:30:45.456Z",
  "level": "INFO",
  "message": "Order processed successfully",
  "request_id": "req_01h455vb4pex5vsknk084sn02q",
  "trace_id": "abc123"
}
```

### Querying Logs

```bash
# Find all logs for a specific request
grep "req_01h455vb4pex5vsknk084sn02q" /var/log/app.log

# Using jq for JSON logs
cat /var/log/app.log | jq 'select(.request_id == "req_01h455vb4pex5vsknk084sn02q")'

# Find requests in a time window (approximate, using ID sorting)
cat /var/log/app.log | jq 'select(.request_id >= "req_01h455v" and .request_id <= "req_01h455w")'
```

---

## Error Handling

### Parsing Errors

```rust
use acton_service::ids::{RequestId, RequestIdError};
use std::str::FromStr;

// Invalid format
let result = RequestId::from_str("invalid");
match result {
    Err(RequestIdError::Parse(_)) => {
        println!("Failed to parse as TypeID");
    }
    _ => {}
}

// Wrong prefix
let result = RequestId::from_str("user_01h455vb4pex5vsknk084sn02q");
match result {
    Err(RequestIdError::InvalidPrefix { expected, actual }) => {
        println!("Expected '{}', got '{}'", expected, actual);
        // Expected 'req', got 'user'
    }
    _ => {}
}
```

### Graceful Degradation

If an incoming request has an invalid `x-request-id` header, a new ID is generated:

```rust
// Client sends invalid header
// x-request-id: not-a-valid-id

// Server generates new valid ID
// x-request-id: req_01h455vb4pex5vsknk084sn02q
```

---

## Distributed Tracing Integration

Request IDs complement distributed tracing:

```
┌─────────────────────────────────────────────────────────────┐
│                     Request Flow                             │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Client ──────► API Gateway ──────► Auth Service             │
│                                          │                   │
│  Headers:                               │                   │
│    x-request-id: req_01h455...          │                   │
│    traceparent: 00-abc123...            ▼                   │
│                                    User Service              │
│                                          │                   │
│                                          ▼                   │
│                                    Database                  │
│                                                              │
└─────────────────────────────────────────────────────────────┘

Request ID: req_01h455vb4pex5vsknk084sn02q (stable across all services)
Trace ID:   abc123... (from OpenTelemetry/W3C Trace Context)
```

**Use Request IDs for:**
- User-facing error references ("Error: please reference ID req_01h455...")
- Log correlation within a single service
- Simple request tracking without full tracing infrastructure

**Use Trace IDs for:**
- Cross-service request flow visualization
- Performance analysis with Jaeger/Tempo
- Detailed span timing and hierarchy

---

## API Reference

### `RequestId`

```rust
pub struct RequestId { /* private */ }

impl RequestId {
    /// Prefix used for request IDs
    pub const PREFIX: &'static str = "req";

    /// Create a new request ID with UUIDv7
    pub fn new() -> Self;

    /// Get the ID as a string slice
    pub fn as_str(&self) -> &str;

    /// Get just the prefix portion
    pub fn prefix(&self) -> &str;

    /// Access the underlying MagicTypeId
    pub fn inner(&self) -> &MagicTypeId;

    /// Convert to the underlying MagicTypeId
    pub fn into_inner(self) -> MagicTypeId;
}

// Implements: Default, Clone, Debug, Display, FromStr,
//             PartialEq, Eq, PartialOrd, Ord, Hash,
//             AsRef<str>, From<RequestId> for String
```

### `RequestIdError`

```rust
pub enum RequestIdError {
    /// Failed to parse as TypeID
    Parse(MagicTypeIdError),

    /// Prefix was not "req"
    InvalidPrefix {
        expected: String,
        actual: String,
    },
}
```

### `MakeTypedRequestId`

```rust
/// tower-http MakeRequestId implementation
#[derive(Debug, Clone, Copy, Default)]
pub struct MakeTypedRequestId;

impl MakeRequestId for MakeTypedRequestId {
    fn make_request_id<B>(&mut self, request: &Request<B>) -> Option<RequestId>;
}
```

---

## Next Steps

- **[Observability](/docs/observability)** - Full distributed tracing setup
- **[Concepts](/docs/concepts)** - Understanding the three pillars of observability
- **[Troubleshooting](/docs/troubleshooting)** - Using request IDs for debugging
