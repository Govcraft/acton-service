# Observability Examples

Examples demonstrating metrics, tracing, and monitoring integration with acton-service.

## Examples

### test-prometheus-metrics.rs

**Pull-based Prometheus `/metrics` endpoint**

Demonstrates the `prometheus-metrics` feature:
- A `/metrics` endpoint in Prometheus text-exposition format
- The OpenTelemetry HTTP metrics tower layer feeding the same meter provider
- Batteries-included wiring: `ServiceBuilder` initializes the meter provider,
  applies the metrics layer, and mounts `/metrics` automatically

Run with:
```bash
cargo run --manifest-path=../../Cargo.toml --example test-prometheus-metrics --features prometheus-metrics
```

Generate traffic and scrape metrics:
```bash
curl http://localhost:8080/api/v1/hello
curl http://localhost:8080/metrics
```

### test-metrics.rs

**HTTP metrics tower layer (manual wiring)**

Demonstrates constructing the OpenTelemetry HTTP metrics layer directly and
applying it to a hand-built router. Metrics are collected into a meter provider;
for OTLP push export enable a collector via the `[otlp]` config and the
`otel-metrics` feature.

Run with:
```bash
cargo run --manifest-path=../../Cargo.toml --example test-metrics --features otel-metrics
```

### test-observability.rs

**OpenTelemetry Tracing Setup**

Demonstrates:
- OpenTelemetry initialization
- Distributed tracing configuration
- Span creation and propagation
- Integration with observability backends (Jaeger, Zipkin, etc.)

Run with:
```bash
cargo run --manifest-path=../../Cargo.toml --example test-observability --features observability
```

## Prerequisites

Requires the `observability` feature flag:
```bash
--features observability
```

## Configuration

### Metrics

With the `prometheus-metrics` feature, metrics are automatically exposed at the
`/metrics` endpoint in Prometheus text-exposition format (mounted by
`ServiceBuilder` alongside `/health` and `/ready`).

Common metrics include:
- Request counts
- Response times
- Error rates
- Custom business metrics

### Tracing

OpenTelemetry can export traces to various backends. Configure via environment variables:

```bash
# Jaeger
export OTEL_EXPORTER_JAEGER_ENDPOINT=http://localhost:14268/api/traces

# OTLP (generic)
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317

# Zipkin
export OTEL_EXPORTER_ZIPKIN_ENDPOINT=http://localhost:9411/api/v2/spans
```

## Testing

### View Prometheus Metrics

```bash
curl http://localhost:8080/metrics
```

Example output:
```
# HELP http_requests_total Total HTTP requests
# TYPE http_requests_total counter
http_requests_total{method="GET",path="/health"} 42

# HELP http_request_duration_seconds HTTP request duration
# TYPE http_request_duration_seconds histogram
http_request_duration_seconds_bucket{le="0.005"} 100
```

### View Traces

1. Start Jaeger:
```bash
docker run -d -p 16686:16686 -p 14268:14268 jaegertracing/all-in-one:latest
```

2. Run your service with tracing enabled

3. Open Jaeger UI: http://localhost:16686

## Production Setup

For production observability:

1. **Metrics**: Use Prometheus + Grafana
   - Scrape `/metrics` endpoint periodically
   - Create dashboards in Grafana
   - Set up alerting rules

2. **Tracing**: Use Jaeger or similar
   - Deploy distributed tracing backend
   - Configure sampling rates
   - Set up trace retention policies

3. **Logging**: Use structured JSON logging
   - Ship logs to centralized system (ELK, Loki)
   - Correlate logs with traces using trace IDs
   - Set appropriate log levels per environment

## Observability Best Practices

1. **Instrument everything**: HTTP handlers, database calls, external APIs
2. **Use correlation IDs**: Track requests across services
3. **Set sampling rates**: Balance detail vs. performance
4. **Monitor key metrics**: Latency, error rate, throughput (RED method)
5. **Alert on SLOs**: Define and monitor Service Level Objectives

## Next Steps

- Integrate with your monitoring stack (Prometheus, Grafana, Jaeger)
- Add custom metrics for business logic
- Implement distributed tracing across multiple services
- Set up alerting based on metrics
- See the main acton-service documentation for advanced observability patterns
