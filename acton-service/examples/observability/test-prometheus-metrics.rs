//! Prometheus Metrics Example - Pull-based `/metrics` endpoint
//!
//! This example demonstrates the `prometheus-metrics` feature:
//! - A pull-based `/metrics` endpoint in Prometheus text-exposition format
//! - The OpenTelemetry HTTP metrics tower layer (via
//!   `opentelemetry-instrumentation-tower`) feeding the same meter provider
//! - Zero manual wiring: `ServiceBuilder` initializes the meter provider,
//!   applies the metrics layer, and mounts `/metrics` automatically
//!
//! Run with:
//!   cargo run --example test-prometheus-metrics --features prometheus-metrics
//!
//! Then, in another terminal, generate some traffic and scrape metrics:
//!   curl http://localhost:8080/api/v1/hello
//!   curl http://localhost:8080/metrics
//!
//! The `/metrics` output includes HTTP server metrics (request counts and
//! latencies) plus the per-version `api.version.requests` counter.

use acton_service::prelude::*;

#[derive(Serialize)]
struct Message {
    message: String,
}

async fn hello() -> Json<Message> {
    Json(Message {
        message: "Hello - this request is counted in /metrics!".to_string(),
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    // HTTP metrics are enabled via the [middleware.metrics] config section.
    // With default config they are on; here we set it explicitly so the example
    // is self-contained regardless of any config.toml on disk.
    let mut config = Config::<()>::default();
    config.service.name = "prometheus-metrics-example".to_string();
    config.middleware.metrics = Some(acton_service::config::MetricsConfig {
        enabled: true,
        include_path: true,
        include_method: true,
        include_status: true,
        latency_buckets_ms: vec![5.0, 25.0, 100.0, 500.0, 1000.0],
    });

    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| router.route("/hello", get(hello)))
        .build_routes();

    // ServiceBuilder automatically:
    // - initializes the Prometheus meter provider (pull reader + registry)
    // - applies the OpenTelemetry HTTP metrics tower layer
    // - mounts GET /metrics alongside /health and /ready
    ServiceBuilder::new()
        .with_config(config)
        .with_routes(routes)
        .build()
        .serve()
        .await?;

    Ok(())
}
