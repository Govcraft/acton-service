---
title: Observability
nextjs:
  metadata:
    title: Observability Stack
    description: OpenTelemetry tracing, metrics, and structured logging built-in
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


# Observability

acton-service includes a complete, production-ready observability stack built on OpenTelemetry standards. The three pillars of observability—distributed tracing, metrics collection, and structured logging—are configured out of the box with zero configuration required. Every request is automatically tracked, traced, and logged with correlation IDs for end-to-end visibility across your distributed services.

---

## Overview

The observability stack provides comprehensive visibility into your service's behavior with automatic instrumentation:

- **Distributed Tracing** - OpenTelemetry integration with OTLP exporter for distributed request tracing across services
- **Metrics Collection** - Automatic HTTP request metrics including count, duration histograms, active requests, and request/response sizes
- **Structured Logging** - JSON-formatted logs with correlation IDs, automatic sensitive data masking, and log level control

All observability features are enabled by default when you include the `observability` feature flag:

```toml
[dependencies]
acton-service = { version = "0.2", features = ["http", "observability"] }
```

---

## Distributed Tracing

acton-service automatically instruments all HTTP requests with OpenTelemetry distributed tracing. Every request creates a trace span that propagates through your service mesh, allowing you to track requests across multiple services and understand latency bottlenecks.

### Automatic OpenTelemetry Integration

The framework initializes OpenTelemetry tracing with an OTLP exporter by default. No manual instrumentation is required:

```rust
use acton_service::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/users", get(list_users))
        })
        .build_routes();

    // OpenTelemetry tracing automatically enabled
    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await
}
```

### Request ID Propagation

The request tracking middleware automatically generates and propagates correlation IDs across service boundaries using standard distributed tracing headers:

- **`x-request-id`** - Unique identifier for each incoming request
- **`x-trace-id`** - Trace identifier for the entire request flow across services
- **`x-span-id`** - Span identifier for the current service operation
- **`x-correlation-id`** - Correlation identifier for related requests

These headers are automatically:
- Generated if not present in incoming requests
- Propagated to downstream services
- Included in logs for correlation
- Returned in HTTP responses

```rust
// Request tracking is automatically enabled by ServiceBuilder
ServiceBuilder::new()
    .with_routes(routes)
    .build()
    .serve()
    .await?;
```

### Configuration

Configure the OpenTelemetry OTLP endpoint via environment variables:

```bash
# OTLP endpoint (default: http://localhost:4317)
export OTEL_EXPORTER_OTLP_ENDPOINT="http://jaeger:4317"

# Service name for traces
export OTEL_SERVICE_NAME="my-service"

# Optional: OTLP protocol (grpc or http)
export OTEL_EXPORTER_OTLP_PROTOCOL="grpc"
```

Or in `config.toml`:

```toml
[observability]
service_name = "my-service"
otlp_endpoint = "http://jaeger:4317"
```

---

## Metrics

acton-service automatically collects comprehensive HTTP request metrics using OpenTelemetry. All metrics are exported via the OTLP exporter and can be visualized in Prometheus, Grafana, or any OpenTelemetry-compatible metrics backend.

### Automatic HTTP Metrics

The following metrics are automatically collected for every HTTP request:

**Request Count**
- Total number of requests
- Labeled by HTTP method, route path, status code

**Request Duration Histograms**
- Request latency distribution
- Percentiles (p50, p95, p99) for latency analysis
- Labeled by HTTP method and route

**Active Requests**
- Current number of in-flight requests
- Useful for understanding service load

**Request and Response Sizes**
- Total bytes received in request bodies
- Total bytes sent in response bodies
- Helps identify bandwidth usage patterns

### Metrics Middleware

Metrics collection is automatic when using the observability middleware:

```rust
// Request tracking is automatically enabled by ServiceBuilder
ServiceBuilder::new()
    .with_routes(routes)
    .build()
    .serve()
    .await?;
// OpenTelemetry metrics are automatically collected
```

The metrics layer is automatically applied to all routes and includes:
- Request start/end timestamps
- HTTP method and path
- Response status codes
- Request duration calculation

---

## Structured Logging

All logs are emitted in structured JSON format with automatic field injection for correlation and debugging.

### JSON Format Logging

Logs are automatically formatted as JSON with consistent fields:

```json
{
  "timestamp": "2025-11-16T10:30:45.123Z",
  "level": "INFO",
  "target": "my_service::handlers",
  "message": "User created successfully",
  "request_id": "01HQWE2XKJY8W7S6G5D4F3E2A1",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "span_id": "00f067aa0ba902b7",
  "correlation_id": "user-signup-flow"
}
```

### Correlation ID Propagation

Correlation IDs from the request tracking middleware are automatically included in all log entries within the request context. This allows you to filter logs by request ID and trace the entire request flow:

```rust
use tracing::{info, error};

async fn create_user(Json(user): Json<User>) -> Result<Json<User>> {
    // Request ID automatically included in logs
    info!("Creating user: {}", user.email);

    // All logs in this request context include the same request_id
    let result = db.create_user(&user).await?;

    info!("User created successfully");
    Ok(Json(result))
}
```

### Sensitive Header Masking

The observability middleware automatically masks sensitive headers in logs to prevent credential leakage:

**Automatically Masked Headers:**
- `Authorization`
- `Cookie`
- `Set-Cookie`
- `X-API-Key`
- `X-Auth-Token`
- Any header containing "token", "secret", "password", "key" (case-insensitive)

Masked headers appear in logs as:
```text
Authorization: [REDACTED]
X-API-Key: [REDACTED]
```

### Log Level Control

Control logging verbosity via environment variables:

```bash
# Set global log level
export RUST_LOG=info

# Set per-module log levels
export RUST_LOG=my_service=debug,acton_service=info,sqlx=warn

# Disable all logs except errors
export RUST_LOG=error
```

Or in code:

```rust
use acton_service::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;

    // Initialize tracing with config
    init_tracing(&config)?;

    // Your service code...
}
```

---

## Configuration

### Environment Variables

Configure observability behavior using environment variables:

```bash
# OpenTelemetry OTLP endpoint
export OTEL_EXPORTER_OTLP_ENDPOINT="http://localhost:4317"

# Service name for traces and metrics
export OTEL_SERVICE_NAME="my-service"

# Log level (trace, debug, info, warn, error)
export RUST_LOG=info

# OTLP protocol (grpc or http)
export OTEL_EXPORTER_OTLP_PROTOCOL="grpc"
```

### Configuration File

Or configure via `~/.config/acton-service/my-service/config.toml`:

```toml
[observability]
# Service name for tracing and metrics
service_name = "my-service"

# OpenTelemetry OTLP endpoint
otlp_endpoint = "http://jaeger:4317"

# Log level (trace, debug, info, warn, error)
log_level = "info"

# Enable/disable specific observability features
tracing_enabled = true
metrics_enabled = true
logging_enabled = true
```

---

## Integration Examples

### Jaeger (Distributed Tracing)

Run Jaeger locally for trace visualization:

```bash
# Start Jaeger with OTLP support
docker run -d --name jaeger \
  -e COLLECTOR_OTLP_ENABLED=true \
  -p 16686:16686 \
  -p 4317:4317 \
  -p 4318:4318 \
  jaegertracing/all-in-one:latest

# Configure service to export to Jaeger
export OTEL_EXPORTER_OTLP_ENDPOINT="http://localhost:4317"
export OTEL_SERVICE_NAME="my-service"

# Run your service
cargo run

# View traces at http://localhost:16686
```

### Prometheus (Metrics)

Export metrics to Prometheus using an OpenTelemetry collector:

```yaml
# otel-collector-config.yaml
receivers:
  otlp:
    protocols:
      grpc:
        endpoint: 0.0.0.0:4317

exporters:
  prometheus:
    endpoint: "0.0.0.0:8889"
    namespace: "acton_service"

service:
  pipelines:
    metrics:
      receivers: [otlp]
      exporters: [prometheus]
```

```bash
# Start OpenTelemetry Collector
docker run -d --name otel-collector \
  -p 4317:4317 \
  -p 8889:8889 \
  -v $(pwd)/otel-collector-config.yaml:/etc/otel-collector-config.yaml \
  otel/opentelemetry-collector-contrib:latest \
  --config=/etc/otel-collector-config.yaml

# Configure Prometheus to scrape metrics
# prometheus.yml
scrape_configs:
  - job_name: 'acton-service'
    static_configs:
      - targets: ['localhost:8889']
```

### Grafana (Visualization)

Combine Jaeger and Prometheus in Grafana:

```bash
# Start Grafana
docker run -d --name grafana \
  -p 3000:3000 \
  grafana/grafana:latest

# Add Prometheus data source at http://localhost:3000
# Add Jaeger data source at http://localhost:16686
```

### Complete Docker Compose Stack

```yaml
# docker-compose.yml
version: '3.8'

services:
  jaeger:
    image: jaegertracing/all-in-one:latest
    environment:
      - COLLECTOR_OTLP_ENABLED=true
    ports:
      - "16686:16686"  # Jaeger UI
      - "4317:4317"    # OTLP gRPC
      - "4318:4318"    # OTLP HTTP

  otel-collector:
    image: otel/opentelemetry-collector-contrib:latest
    command: ["--config=/etc/otel-collector-config.yaml"]
    volumes:
      - ./otel-collector-config.yaml:/etc/otel-collector-config.yaml
    ports:
      - "4317:4317"    # OTLP gRPC
      - "8889:8889"    # Prometheus metrics

  prometheus:
    image: prom/prometheus:latest
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml
    ports:
      - "9090:9090"

  grafana:
    image: grafana/grafana:latest
    ports:
      - "3000:3000"
    depends_on:
      - prometheus
      - jaeger
```

```bash
# Start the observability stack
docker-compose up -d

# Run your service with observability
export OTEL_EXPORTER_OTLP_ENDPOINT="http://localhost:4317"
export OTEL_SERVICE_NAME="my-service"
cargo run

# Access dashboards:
# - Jaeger: http://localhost:16686
# - Prometheus: http://localhost:9090
# - Grafana: http://localhost:3000
```

---

## Next Steps

- See [Examples](/docs/examples) for complete observability setup with Jaeger and Prometheus
- Learn about [Middleware](/docs/middleware) customization and the request tracking middleware
- Read about [Configuration](/docs/configuration) for environment-based observability settings
- Explore [Health Checks](/docs/health-checks) for monitoring service dependencies
