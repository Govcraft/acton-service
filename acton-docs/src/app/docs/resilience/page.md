---
title: Resilience Patterns
nextjs:
  metadata:
    title: Resilience Patterns
    description: Build fault-tolerant microservices with circuit breakers, retry logic, and bulkhead patterns to prevent cascading failures and overload.
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


acton-service provides resilience patterns to protect your services from cascading failures, transient errors, and resource exhaustion.

## Quick Start

```rust
use acton_service::middleware::ResilienceConfig;

ServiceBuilder::new()
    .with_routes(routes)
    .with_middleware(|router| {
        router.layer(ResilienceConfig::new()
            .with_circuit_breaker(true)
            .with_circuit_breaker_threshold(0.5)   // 50% failure threshold
            .with_retry(true)
            .with_retry_max_attempts(3)            // max 3 retries
            .with_bulkhead(true)
            .with_bulkhead_max_concurrent(100))    // max 100 concurrent requests
    })
    .build()
    .serve()
    .await?;
```

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
- How to test recovery (half-open request count)

**What Happens Automatically:**
- State transitions based on observed failures
- Failure rate calculation
- Recovery testing

### Configuration

Circuit breaker parameters can be configured **declaratively** via config files or environment variables (no recompilation needed), or programmatically in code.

**Option 1: Config File (Recommended for Production)**

```toml
# config.toml
[middleware.resilience]
circuit_breaker_enabled = true
circuit_breaker_threshold = 0.5      # Open at 50% failure rate
circuit_breaker_min_requests = 10    # Min requests before evaluation
circuit_breaker_wait_secs = 60       # How long to stay open
```

Configuration changes require service restart but not recompilation or redeployment.

**Option 2: Environment Variables**

```bash
ACTON_MIDDLEWARE_RESILIENCE_CIRCUIT_BREAKER_ENABLED=true
ACTON_MIDDLEWARE_RESILIENCE_CIRCUIT_BREAKER_THRESHOLD=0.5
ACTON_MIDDLEWARE_RESILIENCE_CIRCUIT_BREAKER_MIN_REQUESTS=10
ACTON_MIDDLEWARE_RESILIENCE_CIRCUIT_BREAKER_WAIT_SECS=60
```

**Option 3: Programmatic (Code)**

```rust
use acton_service::middleware::ResilienceConfig;

let resilience = ResilienceConfig::new()
    .with_circuit_breaker(true)
    .with_circuit_breaker_threshold(0.5)        // Open at 50% failure rate
    .with_circuit_breaker_min_requests(10)      // Min 10 requests before evaluation
    .with_circuit_breaker_timeout_secs(60)      // Open state duration
    .with_circuit_breaker_half_open_requests(3);// Test requests in half-open

ServiceBuilder::new()
    .with_routes(routes)
    .with_middleware(|router| router.layer(resilience))
    .build()
```

### Configuration Options

**Failure Threshold**
- Percentage of failed requests that triggers open state
- Range: 0.0 (never open) to 1.0 (open on any failure)
- Recommended: 0.5 (50%) for most services

**Minimum Request Volume**
- Minimum requests before evaluating failure rate
- Prevents premature opening on low traffic
- Recommended: 10-20 requests

**Timeout Duration**
- How long circuit stays open before testing recovery
- Too short: doesn't allow service to recover
- Too long: extends downtime unnecessarily
- Recommended: 30-60 seconds

**Half-Open Test Requests**
- Number of test requests in half-open state
- Too few: unreliable recovery detection
- Too many: may overwhelm recovering service
- Recommended: 3-5 requests

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

Retry logic handles transient failures by automatically retrying failed requests with exponential backoff.

### How Retries Work

1. Initial request fails with retryable error
2. Wait for backoff duration
3. Retry request
4. If fails, double backoff and retry again
5. Continue until max retries or success

### Configuration

```rust
use acton_service::middleware::ResilienceConfig;

let resilience = ResilienceConfig::new()
    .with_retry(true)
    .with_retry_max_attempts(3)               // Max 3 retry attempts
    .with_retry_initial_backoff_ms(100)       // Start with 100ms
    .with_retry_max_backoff_ms(10_000)        // Cap at 10 seconds
    .with_retry_backoff_multiplier(2.0)       // Double each retry
    .with_retry_jitter(true);                 // Add randomization
```

### Backoff Strategy

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

### Configuration Best Practices

**Fast-Failing Services:**
```rust
.with_retry(3)
.initial_backoff_ms(50)
.max_backoff_ms(500)
```

**Slow External APIs:**
```rust
.with_retry(5)
.initial_backoff_ms(1000)
.max_backoff_ms(30_000)
```

**Database Queries:**
```rust
.with_retry(3)
.initial_backoff_ms(100)
.max_backoff_ms(5_000)
.jitter(true)  // Prevent connection storms
```

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

let resilience = ResilienceConfig::new()
    .with_bulkhead(true)
    .with_bulkhead_max_concurrent(100)         // Max 100 concurrent requests
    .with_bulkhead_queue_size(50)              // Queue up to 50 waiting requests
    .with_bulkhead_wait_timeout_ms(5000);      // Wait max 5 seconds
```

### Configuration Options

**Max Concurrent Requests**
- Maximum requests processed simultaneously
- Based on service capacity and resources
- Typical values: 50-500 depending on workload

**Queue Size**
- Requests waiting for available slot
- 0 = reject immediately when full
- Too large = memory pressure during traffic spikes
- Recommended: 10-50% of concurrent limit

**Wait Timeout**
- Maximum time request waits in queue
- Too short: unnecessary rejections
- Too long: poor user experience
- Recommended: 1-5 seconds for user-facing, 10-30s for background

### Response When Bulkhead Full

```text
HTTP/1.1 503 Service Unavailable
Retry-After: 5

{
  "error": "Service temporarily overloaded",
  "code": "BULKHEAD_FULL",
  "status": 503,
  "retry_after": 5
}
```

### Per-Endpoint Bulkheads

Isolate expensive operations from normal traffic:

```rust
use acton_service::prelude::*;
use acton_service::middleware::ResilienceConfig;

let routes = VersionedApiBuilder::new()
    .with_base_path("/api")
    .add_version(ApiVersion::V1, |router| {
        router
            // Expensive report generation: limited concurrency
            .route("/reports/generate", post(generate_report)
                .layer(ResilienceConfig::new().with_bulkhead(true).with_bulkhead_max_concurrent(5)))

            // Normal CRUD operations: higher concurrency
            .route("/users", get(list_users)
                .layer(ResilienceConfig::new().with_bulkhead(true).with_bulkhead_max_concurrent(100)))
            .route("/documents", get(list_documents)
                .layer(ResilienceConfig::new().with_bulkhead(true).with_bulkhead_max_concurrent(100)))
    })
    .build_routes();

ServiceBuilder::new()
    .with_routes(routes)
    .build()
    .serve()
    .await?;
```

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

Layer multiple patterns for comprehensive protection:

```rust
use acton_service::middleware::ResilienceConfig;

let resilience = ResilienceConfig::new()
    // Circuit breaker: detect and isolate failures
    .with_circuit_breaker(true)
    .with_circuit_breaker_threshold(0.5)
    .with_circuit_breaker_min_requests(10)
    .with_circuit_breaker_timeout_secs(60)
    // Retry: handle transient failures
    .with_retry(true)
    .with_retry_max_attempts(3)
    .with_retry_initial_backoff_ms(100)
    .with_retry_max_backoff_ms(5_000)
    .with_retry_jitter(true)
    // Bulkhead: prevent resource exhaustion
    .with_bulkhead(true)
    .with_bulkhead_max_concurrent(100)
    .with_bulkhead_queue_size(50)
    .with_bulkhead_wait_timeout_ms(5000);

ServiceBuilder::new()
    .with_routes(routes)
    .with_middleware(|router| router.layer(resilience))
    .build()
```

### Execution Order

Resilience patterns execute in this order:

1. **Bulkhead** - Check capacity first
2. **Circuit Breaker** - Fail fast if open
3. **Retry** - Retry failed requests
4. **Request** - Execute actual handler

### Pattern Interaction Examples

**Scenario 1: Service Overload**
```text
1. Bulkhead rejects excess requests (503)
2. Accepted requests proceed through circuit breaker
3. High failure rate triggers circuit breaker
4. Circuit opens, failing fast for recovery
```

**Scenario 2: Transient Network Error**
```text
1. Request passes bulkhead (capacity available)
2. Circuit breaker is closed (service healthy)
3. Request fails with network timeout
4. Retry logic retries with backoff
5. Retry succeeds, request completes
```

**Scenario 3: Cascading Failure Prevention**
```text
1. Downstream service fails completely
2. Circuit breaker detects high failure rate
3. Circuit opens, stops sending requests
4. Bulkhead frees up capacity
5. Service remains responsive for other endpoints
```

## Configuration Templates

### Conservative (Low-Risk Services)

```rust
ResilienceConfig::new()
    .with_circuit_breaker(true)
    .with_circuit_breaker_threshold(0.7)       // Higher threshold
    .with_retry(true)
    .with_retry_max_attempts(5)                // More retries
    .with_bulkhead(true)
    .with_bulkhead_max_concurrent(200)         // Higher capacity
```

### Aggressive (High-Risk/Experimental)

```rust
ResilienceConfig::new()
    .with_circuit_breaker(true)
    .with_circuit_breaker_threshold(0.3)       // Lower threshold
    .with_retry(true)
    .with_retry_max_attempts(2)                // Fewer retries
    .with_bulkhead(true)
    .with_bulkhead_max_concurrent(50)          // Lower capacity
```

### Balanced (Production Default)

```rust
ResilienceConfig::new()
    .with_circuit_breaker(true)
    .with_circuit_breaker_threshold(0.5)       // Moderate threshold
    .with_retry(true)
    .with_retry_max_attempts(3)                // Standard retries
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
