---
title: Resilience Patterns
nextjs:
  metadata:
    title: Resilience Patterns
    description: Build fault-tolerant services with circuit breakers, retry logic, and bulkhead patterns to prevent cascading failures and overload.
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


acton-service provides resilience patterns to protect your services from cascading failures, transient errors, and resource exhaustion.

{% callout type="warning" title="Resilience is opt-in and manually wired" %}
Unlike authentication, authorization, and rate limiting, resilience layers are **not** auto-applied by `ServiceBuilder`. You build a `ResilienceConfig`, ask it for a layer, and attach that layer to the tower service stack you want to protect.

Current surface (`resilience` feature):

- **Circuit breaker** — `ResilienceConfig::circuit_breaker_layer()`
- **Bulkhead** — `ResilienceConfig::bulkhead_layer()`
- **Retry** — configuration fields only. **No retry layer is constructed today**; see [Retry Logic](#retry-logic).
{% /callout %}

## Quick Start

`ResilienceConfig` is a configuration struct with a builder — it is **not** a `Layer`. Build it, then call a layer constructor:

```rust
use acton_service::middleware::ResilienceConfig;

let config = ResilienceConfig::new()
    .with_circuit_breaker(true)
    .with_circuit_breaker_threshold(0.5)   // 50% failure rate opens the circuit
    .with_bulkhead(true)
    .with_bulkhead_max_concurrent(100);    // max 100 concurrent calls

// Each constructor returns `None` when that pattern is disabled
if let Some(layer) = config.circuit_breaker_layer() {
    // Attach to your outbound tower client stack
}

if let Some(layer) = config.bulkhead_layer() {
    // Attach to your tower service stack
}
```

Both constructors return `Option`, so a disabled pattern costs you nothing at runtime.

### Protecting an Inbound Router

For an inbound axum router, use `apply_resilience` rather than attaching the layers by hand:

```rust
use acton_service::middleware::resilience::{apply_resilience, ResilienceConfig};
use axum::{routing::get, Router};

let app = Router::new().route("/", get(handler));
let app = apply_resilience(app, &ResilienceConfig::default());
```

This wires up three things that are easy to get wrong individually:

1. **A 5xx-aware failure classifier.** An inbound axum route is infallible — a handler that fails returns `Ok(Response)` with a 500 status, never `Err`. The default classifier counts only `Err` as a failure, so a hand-attached breaker would compile, look correct, and never open. `apply_resilience` classifies any 5xx response as a failure. A 4xx is the caller's fault and never trips the circuit.
2. **An error handler.** Both layers change the service error type, but `Router::layer` requires `Infallible`. Rejections are converted to `503 Service Unavailable` and logged.
3. **Ordering.** The bulkhead sits inside the breaker, so a concurrency rejection becomes a 503 that the breaker observes — sustained overload can open the circuit.

## Circuit Breaker

Circuit breakers prevent cascading failures by detecting unhealthy dependencies and failing fast instead of waiting for timeouts.

{% callout type="note" title="What are Cascading Failures?" %}
**Cascading failure** occurs when a failure in one service causes failures in dependent services, which spread through your system like falling dominos:

**Without circuit breaker:**
```
Database goes down (1 service)
  ↓
API Service keeps calling it, times out after 30s per request
  ↓
API Service thread pool exhausted waiting for database
  ↓
Frontend calls API, times out waiting for response
  ↓
Frontend becomes unresponsive to users
  ↓
Load balancer health checks fail
  ↓
ENTIRE SYSTEM DOWN (cascading failure)
```

**With circuit breaker:**
```
Database goes down (1 service)
  ↓
Circuit breaker detects failures, opens circuit
  ↓
API Service fails fast (returns 503 immediately, no waiting)
  ↓
API Service thread pool stays available
  ↓
Frontend gets fast 503 response, shows user-friendly error
  ↓
Rest of system continues working (failure contained)
```

Circuit breakers **contain** the blast radius of failures - one failing dependency doesn't bring down your entire system.
{% /callout %}

### How Circuit Breakers Work

Circuit breakers **automatically** monitor your service health and transition between states at runtime based on observed failures. You configure the thresholds and behavior, but state transitions happen automatically - no code changes or redeployment required.

**Closed (Normal Operation)**
- Requests pass through to downstream service
- Failures are counted and monitored
- **Automatically** transitions to Open if failure rate exceeds configured threshold

**Open (Failing Fast)**
- Requests fail immediately without calling downstream
- No load on failing service (allows recovery)
- **Automatically** transitions to Half-Open after configured wait duration

**Half-Open (Testing Recovery)**
- Limited requests pass through to test service health
- **Automatically** returns to Closed if test requests succeed
- **Automatically** returns to Open if test requests fail

**What You Configure:**
- When to open (failure threshold, minimum requests)
- How long to stay open (wait duration)

**What Happens Automatically:**
- State transitions based on observed failures
- Failure rate calculation
- Recovery testing

### Configuration

`ResilienceConfig` exposes builder methods for the most common knobs, and public fields for the rest:

```rust
use acton_service::middleware::ResilienceConfig;
use std::time::Duration;

let mut resilience = ResilienceConfig::new()
    .with_circuit_breaker(true)
    .with_circuit_breaker_threshold(0.5);   // Open at 50% failure rate (clamped to 0.0..=1.0)

// Fields without a builder method are public — set them directly
resilience.circuit_breaker_min_requests = 10;                          // Sliding window size
resilience.circuit_breaker_wait_duration = Duration::from_secs(60);    // How long to stay open
```

**Available builder methods:**

| Method | Argument | Effect |
| --- | --- | --- |
| `with_circuit_breaker(bool)` | enable flag | Turns the circuit breaker on or off |
| `with_circuit_breaker_threshold(f64)` | `0.0`–`1.0` | Failure rate that opens the circuit (clamped) |
| `with_bulkhead(bool)` | enable flag | Turns the bulkhead on or off |
| `with_bulkhead_max_concurrent(usize)` | concurrency cap | Maximum concurrent calls |

There is no `with_circuit_breaker_min_requests()`, `with_circuit_breaker_timeout_secs()`, or `with_circuit_breaker_half_open_requests()` method — set `circuit_breaker_min_requests` and `circuit_breaker_wait_duration` as fields.

### Building the Layer

`circuit_breaker_layer()` takes no type parameters and returns `None` when the circuit breaker is disabled:

```rust
use acton_service::middleware::resilience::ResilienceConfig;

let config = ResilienceConfig::default();

if let Some(layer) = config.circuit_breaker_layer() {
    // Apply the layer to your outbound service stack
}
```

This layer counts `Err` results as failures, which suits **outbound** tower stacks — the clients you use to call databases and downstream services, where a transport failure surfaces as `Err`.

For an **inbound** axum router, reach for `apply_resilience` instead. If you need the pieces separately, `http_circuit_breaker_layer()` returns a breaker that classifies 5xx responses as failures; you must still pair it with an error handler before calling `Router::layer`.

### Configuration Reference

**Failure Threshold** (`circuit_breaker_threshold`, default `0.5`)
- Fraction of failed requests that triggers the open state
- Range: 0.0 (never open) to 1.0 (open on total failure); values outside the range are clamped
- Recommended: 0.5 (50%) for most services

**Minimum Request Volume** (`circuit_breaker_min_requests`, default `10`)
- Sliding-window size — how many requests are considered before the failure rate is meaningful
- Prevents premature opening on low traffic
- Recommended: 10-20 requests

**Wait Duration** (`circuit_breaker_wait_duration`, default 30s)
- How long the circuit stays open before testing recovery
- Too short: doesn't allow the service to recover
- Too long: extends downtime unnecessarily
- Recommended: 30-60 seconds

### TOML Configuration

The `[middleware.resilience]` section is parsed into `config.middleware.resilience`:

```toml
# config.toml
[middleware.resilience]
circuit_breaker_enabled = true
circuit_breaker_threshold = 0.5      # Open at 50% failure rate
circuit_breaker_min_requests = 10    # Min requests before evaluation
circuit_breaker_wait_secs = 60       # How long to stay open
```

{% callout type="note" title="TOML values are not auto-applied" %}
`ServiceBuilder` does not construct resilience layers from this section. Read the values off the loaded `Config` and build the layers yourself. Keeping the settings in TOML still buys you environment-specific tuning without a recompile.
{% /callout %}

### When to Use Circuit Breakers

**Good For:**
- External API calls (third-party services)
- Database queries during outages
- Downstream microservice calls
- Any dependency that may fail temporarily

**Not Needed For:**
- In-memory operations
- Local file system access
- Synchronous CPU-bound work
- Operations with no external dependencies

### Monitoring Circuit Breaker State

```rust
use tracing::info;

// Circuit breaker emits events you can monitor
info!(
    circuit_state = ?state,
    failure_rate = failure_rate,
    "Circuit breaker state changed"
);
```

**Metrics to Track:**
- State transitions (closed → open → half-open)
- Failure rate over time
- Time spent in each state
- Request success rate in half-open state

## Retry Logic

{% callout type="warning" title="Retry is not server middleware" %}
There is no retry layer, and there will not be one at this position. Retrying means **replaying** a request, and an inbound `Request<Body>` wraps a stream that is consumed once — the underlying retry layer requires `Req: Clone`, which an inbound request cannot satisfy. Buffering every request body to make it replayable would be a memory-exhaustion risk on a public endpoint.

Retry belongs on **outbound** client stacks, where you control the request and can cheaply reconstruct it. Compose `tower-resilience-retry` (or `tower::retry`) into the clients you use to call databases and downstream services, and use the guidance below to decide *what* to retry.

The `retry_*` fields and TOML keys were removed in the release following this note; they never did anything.
{% /callout %}

### Backoff Strategy

When you implement retries yourself, exponential backoff with jitter is the pattern to reach for:

**Exponential Backoff:**
```text
Retry 1: 100ms
Retry 2: 200ms
Retry 3: 400ms
Retry 4: 800ms (with 2x multiplier)
```

**With Jitter (Recommended):**
```text
Retry 1: 80-120ms   (random ±20%)
Retry 2: 160-240ms
Retry 3: 320-480ms
```

Jitter prevents thundering herd when many clients retry simultaneously.

### Retryable vs Non-Retryable Errors

**Always Retry:**
- Network timeouts
- Connection refused
- Temporary DNS failures
- 503 Service Unavailable
- 429 Too Many Requests (with backoff)

**Never Retry:**
- 400 Bad Request (client error)
- 401 Unauthorized (auth issue)
- 403 Forbidden (permission issue)
- 404 Not Found (resource doesn't exist)
- 422 Unprocessable Entity (validation error)

**Configurable (Depends on Context):**
- 500 Internal Server Error (may be transient)
- Database deadlocks (usually transient)
- Read timeouts (may be temporary)

### Idempotency Requirement

{% callout type="warning" title="What is Idempotency?" %}
**Idempotent** means an operation produces the same result whether executed once or multiple times. Retrying an idempotent operation is safe - it won't cause duplicate side effects.

**Examples:**
- **Idempotent**: `DELETE /users/123` - deleting the same user twice has the same result (user is deleted)
- **NOT Idempotent**: `POST /orders` - creating an order twice creates two orders (duplicate!)

**Why it matters for retries:** If a request times out, you don't know if the server processed it before timing out. Retrying a non-idempotent operation risks duplicates.
{% /callout %}

**Only retry idempotent operations:**

```rust
// Safe to retry (idempotent)
GET  /api/v1/users/123        // Reading same data multiple times = safe
PUT  /api/v1/users/123        // Setting same value multiple times = safe
DELETE /api/v1/users/123      // Deleting same user multiple times = safe

// NOT safe to retry (non-idempotent)
POST /api/v1/orders      // Creates duplicate orders
POST /api/v1/payments    // Charges customer multiple times
PATCH /api/v1/counter    // Increments multiple times
```

**Making Non-Idempotent Operations Retryable:**

Use idempotency keys:

```rust
POST /api/v1/orders
Headers:
  Idempotency-Key: unique-request-id-12345

// Server deduplicates based on idempotency key
```

### Backoff Guidance by Workload

Starting points for the backoff you implement:

| Workload | Attempts | Base delay | Max delay |
| --- | --- | --- | --- |
| Fast-failing internal services | 3 | 50ms | 500ms |
| Slow external APIs | 5 | 1s | 30s |
| Database queries | 3 | 100ms | 5s (add jitter to prevent connection storms) |

## Bulkhead Pattern

Bulkheads limit concurrent requests to prevent resource exhaustion and isolate failures.

### How Bulkheads Work

Named after ship compartments that prevent total flooding:

1. Set maximum concurrent request limit
2. Requests beyond limit are queued or rejected
3. Prevents thread pool exhaustion
4. Isolates resource usage per operation

### Configuration

```rust
use acton_service::middleware::ResilienceConfig;
use std::time::Duration;

let mut resilience = ResilienceConfig::new()
    .with_bulkhead(true)
    .with_bulkhead_max_concurrent(100);           // Max 100 concurrent calls

// Public field — no builder method
resilience.bulkhead_max_wait = Duration::from_secs(5);   // Max wait for a slot

if let Some(layer) = resilience.bulkhead_layer() {
    // Apply the layer to your service
}
```

Unlike the circuit breaker, `bulkhead_layer()` is not generic — it returns `Option<BulkheadLayer>` directly, and `None` when the bulkhead is disabled.

There is no `with_bulkhead_queue_size()` or `with_bulkhead_wait_timeout_ms()` method. Concurrency and wait time are the two knobs; requests that cannot get a slot within `bulkhead_max_wait` are rejected.

### Configuration Reference

**Max Concurrent Calls** (`bulkhead_max_concurrent`, default `100`)
- Maximum calls processed simultaneously
- Based on service capacity and resources
- Typical values: 50-500 depending on workload

**Max Wait** (`bulkhead_max_wait`, default 5s)
- Maximum time a call waits for a free slot before being rejected
- Too short: unnecessary rejections
- Too long: poor user experience
- Recommended: 1-5 seconds for user-facing, 10-30s for background

Rejections and permits are logged by the layer: `on_call_permitted` emits a debug event with the current concurrency, `on_call_rejected` emits a warning with the configured maximum.

### When the Bulkhead Is Full

A rejected call surfaces as an **error from the bulkhead layer** — acton-service does not define a `BULKHEAD_FULL` error variant or map the rejection to an HTTP response for you. Because you attach the layer, you also decide how its rejection is rendered. `503 Service Unavailable` with a `Retry-After` header is the conventional choice:

```text
HTTP/1.1 503 Service Unavailable
Retry-After: 5
```

### Per-Operation Bulkheads

Isolate expensive operations from normal traffic by giving each its own `ResilienceConfig` and its own layer. Note that `ResilienceConfig` itself is never passed to `.layer()` — only the layer it produces:

```rust
use acton_service::middleware::ResilienceConfig;

// Expensive report generation: tight concurrency cap
let reports = ResilienceConfig::new()
    .with_bulkhead(true)
    .with_bulkhead_max_concurrent(5);

// Normal CRUD operations: generous cap
let crud = ResilienceConfig::new()
    .with_bulkhead(true)
    .with_bulkhead_max_concurrent(100);

let reports_layer = reports.bulkhead_layer();   // Option<BulkheadLayer>
let crud_layer = crud.bulkhead_layer();

// Wrap the tower services behind each operation with its own layer
```

Each `BulkheadLayer` owns an independent slot pool, so a saturated report queue cannot starve the CRUD handlers.

### When to Use Bulkheads

**Good For:**
- Protecting against traffic spikes
- Expensive database queries
- External API calls with rate limits
- CPU-intensive operations
- File upload/download endpoints

**Not Needed For:**
- Lightweight, fast operations (<10ms)
- Already rate-limited endpoints
- Health check endpoints
- Static file serving

## Combining Resilience Patterns

One `ResilienceConfig` can produce both layers. Build the config once, then stack the layers you get back:

```rust
use acton_service::middleware::ResilienceConfig;
use http::Request;
use std::time::Duration;

let mut resilience = ResilienceConfig::new()
    // Circuit breaker: detect and isolate failures
    .with_circuit_breaker(true)
    .with_circuit_breaker_threshold(0.5)
    // Bulkhead: prevent resource exhaustion
    .with_bulkhead(true)
    .with_bulkhead_max_concurrent(100);

resilience.circuit_breaker_min_requests = 10;
resilience.circuit_breaker_wait_duration = Duration::from_secs(60);
resilience.bulkhead_max_wait = Duration::from_secs(5);

let breaker = resilience.circuit_breaker_layer::<Request<()>, String>();
let bulkhead = resilience.bulkhead_layer();

// Stack `bulkhead` outside `breaker` so capacity is checked before the
// circuit-breaker state, then wrap your service with the result.
```

Retry is deliberately absent here — see [Retry Logic](#retry-logic) for why the retry settings do not produce a layer.

### Execution Order

Order your stack so that each layer protects the ones inside it:

1. **Bulkhead** - Check capacity first (reject early, cheaply)
2. **Circuit Breaker** - Fail fast if the circuit is open
3. **Service** - Execute the actual call

### Pattern Interaction Examples

**Scenario 1: Service Overload**
```text
1. Bulkhead rejects excess calls once max_concurrent is saturated
2. Accepted calls proceed through the circuit breaker
3. High failure rate trips the circuit breaker
4. Circuit opens, failing fast for recovery
```

**Scenario 2: Cascading Failure Prevention**
```text
1. Downstream service fails completely
2. Circuit breaker detects high failure rate
3. Circuit opens, stops sending requests
4. Bulkhead frees up capacity
5. Service remains responsive for other endpoints
```

## Configuration Templates

Every method below exists on `ResilienceConfig`, and every one reaches a layer.

### Conservative (Low-Risk Services)

```rust
ResilienceConfig::new()
    .with_circuit_breaker(true)
    .with_circuit_breaker_threshold(0.7)       // Higher threshold
    .with_bulkhead(true)
    .with_bulkhead_max_concurrent(200)         // Higher capacity
```

### Aggressive (High-Risk/Experimental)

```rust
ResilienceConfig::new()
    .with_circuit_breaker(true)
    .with_circuit_breaker_threshold(0.3)       // Lower threshold
    .with_bulkhead(true)
    .with_bulkhead_max_concurrent(50)          // Lower capacity
```

### Balanced (Production Default)

```rust
ResilienceConfig::new()
    .with_circuit_breaker(true)
    .with_circuit_breaker_threshold(0.5)       // Moderate threshold
    .with_bulkhead(true)
    .with_bulkhead_max_concurrent(100)         // Moderate capacity
```

## Troubleshooting

### Circuit Breaker Opens Frequently

**Symptom**: Circuit breaker constantly in open state

**Possible Causes:**
1. Downstream service genuinely unhealthy
2. Failure threshold too low
3. Request volume threshold too low
4. Timeout too short for recovery

**Solutions:**
- Monitor downstream service health
- Increase failure threshold (0.5 → 0.7)
- Increase minimum request volume
- Increase open state timeout duration

### Requests Failing with 503 Bulkhead Full

**Symptom**: High rate of 503 errors during traffic spikes

**Possible Causes:**
1. Bulkhead capacity too low
2. Slow request processing
3. Traffic spike exceeds service capacity

**Solutions:**
- Increase concurrent request limit
- Optimize slow handlers
- Add horizontal scaling
- Implement request prioritization

### Retries Causing Duplicate Operations

**Symptom**: Duplicate records or multiple charges

**Possible Causes:**
1. Retrying non-idempotent operations
2. Missing idempotency key implementation

**Solutions:**
- Only retry GET, PUT, DELETE (not POST)
- Implement idempotency keys for POST
- Use database unique constraints
- Track request IDs to deduplicate

## Monitoring and Observability

Track these metrics for resilience patterns:

**Circuit Breaker:**
- State transitions (closed/open/half-open)
- Failure rate percentage
- Time in each state
- Rejected request count

**Retries:**
- Retry attempts per request
- Successful retry rate
- Retry backoff distribution
- Permanently failed requests

**Bulkhead:**
- Current concurrent request count
- Queue depth over time
- Rejected request count
- Average wait time

**Implementation:**

```rust
use tracing::{info, warn};

info!(
    circuit_state = ?state,
    failure_rate = failure_rate,
    "Circuit breaker metrics"
);

warn!(
    retry_attempt = attempt,
    max_retries = max,
    backoff_ms = backoff,
    "Retry attempt"
);

info!(
    concurrent = current,
    max_concurrent = max,
    queue_depth = depth,
    "Bulkhead metrics"
);
```

## Next Steps

- [Configure Timeouts](/docs/configuration) - Set request timeouts
- [Monitor Metrics](/docs/observability) - Track resilience metrics
- [Rate Limiting](/docs/rate-limiting) - Combine with rate limits
- [Health Checks](/docs/health-checks) - Implement health monitoring
