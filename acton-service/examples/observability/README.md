# Observability Examples

Examples demonstrating metrics, tracing, and monitoring integration with acton-service.

## Examples

### test-metrics.rs

**Prometheus Metrics Integration**

Demonstrates:
- Prometheus metrics collection
- Custom metric definitions
- Automatic metric endpoints
- Performance monitoring

Run with:
```bash
cargo run --manifest-path=../../Cargo.toml --example test-metrics --features observability
```

View metrics:
```bash
curl http://localhost:8080/metrics
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

Metrics are automatically exposed at `/metrics` endpoint in Prometheus format.

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
