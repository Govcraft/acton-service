//! Integration coverage for `[middleware.metrics]` reaching the instruments.
//!
//! The defect these cover was reported downstream: an operator set
//! `latency_buckets_ms` to their SLO thresholds, scraped `/metrics`, and got the
//! default boundaries back. Nothing failed. Nothing warned. The config parsed,
//! `--check-config` reported it, and the histogram ignored it.
//!
//! It survived because every unit test in reach was true of a config *value*,
//! and the loss happened between the value and the instrument -- twice over,
//! once where the layer builder dropped it and once where a meter-provider view
//! would have overruled it. The only assertion that could have caught it is the
//! one made here: drive a real request through the real layer and read what the
//! registry actually holds.
//!
//! Routers are driven with `ServiceExt::oneshot` rather than a bound server,
//! following `governor_integration.rs` and `resilience_router.rs`.

#![cfg(feature = "prometheus-metrics")]

use acton_service::config::{Config, MetricsConfig};
use acton_service::middleware::metrics::{create_metrics_layer, metric_labels, metric_names};
use acton_service::observability::{init_meter_provider, PROMETHEUS_REGISTRY};
use axum::{body::Body, http::Request, routing::post, Router};
use tower::ServiceExt;

/// The boundaries an operator would plausibly choose: a 250ms SLO, with
/// resolution either side of it. None of these appear in any default set, so a
/// passing assertion cannot be a coincidence.
const CONFIGURED_MS: &[f64] = &[50.0, 150.0, 250.0, 400.0, 750.0];

/// `le` upper bounds Prometheus reports for the HTTP request-duration histogram.
fn scraped_duration_bounds() -> Vec<f64> {
    PROMETHEUS_REGISTRY
        .get()
        .expect("init_meter_provider installs the registry")
        .gather()
        .iter()
        .filter(|family| family.name().contains("http_server_request_duration"))
        .flat_map(|family| family.get_metric().to_vec())
        .flat_map(|metric| metric.get_histogram().get_bucket().to_vec())
        .map(|bucket| bucket.upper_bound())
        .collect()
}

/// Configured milliseconds, in the seconds the instrument is declared in.
fn configured_bounds_in_seconds() -> Vec<f64> {
    CONFIGURED_MS.iter().map(|ms| ms / 1000.0).collect()
}

// One test per process under nextest, which is what lets this touch the
// process-wide meter provider at all: `METER_PROVIDER` is a `OnceCell` and the
// first writer wins for the life of the process.
#[tokio::test]
async fn configured_latency_buckets_reach_the_scrape() {
    let metrics = MetricsConfig::new().with_latency_buckets_ms(CONFIGURED_MS.to_vec());

    let mut config = Config::<()>::default();
    config.service.name = "metrics-config-e2e".to_string();
    config.middleware.metrics = Some(metrics.clone());

    init_meter_provider(&config).expect("meter provider initializes");
    let layer = create_metrics_layer(&metrics).expect("the layer builds once a meter exists");

    // A POST carrying Content-Length, so the request-body-size instrument
    // records too: the layer reads that header, and an empty GET leaves the
    // instrument with no measurements and therefore absent from the scrape.
    let app = Router::new()
        .route("/probe", post(|| async { "ok" }))
        .layer(layer);

    app.oneshot(
        Request::builder()
            .method("POST")
            .uri("/probe")
            .header("content-length", "2")
            .body(Body::from("hi"))
            .expect("request builds"),
    )
    .await
    .expect("router is infallible");

    let bounds = scraped_duration_bounds();

    assert!(
        !bounds.is_empty(),
        "the request must have been recorded; got no duration histogram at all"
    );
    assert_eq!(
        bounds,
        configured_bounds_in_seconds(),
        "the scrape must show the configured boundaries, not a default set"
    );

    // The same scrape settles what this crate publishes as metric and attribute
    // names. They were wrong -- a request-count instrument that does not exist,
    // and pre-1.0 attribute spellings -- and nothing here could tell, because
    // every assertion on them compared a constant to its own literal.
    let scraped = scraped_family_names();

    for name in [
        metric_names::HTTP_SERVER_REQUEST_DURATION,
        metric_names::HTTP_SERVER_ACTIVE_REQUESTS,
        metric_names::HTTP_SERVER_REQUEST_BODY_SIZE,
        metric_names::HTTP_SERVER_RESPONSE_BODY_SIZE,
    ] {
        let prometheus_name = name.replace('.', "_");
        assert!(
            scraped
                .iter()
                .any(|family| family.starts_with(&prometheus_name)),
            "{name} names no instrument this layer creates; scraped: {scraped:?}"
        );
    }

    for label in [
        metric_labels::HTTP_REQUEST_METHOD,
        metric_labels::HTTP_ROUTE,
        metric_labels::HTTP_RESPONSE_STATUS_CODE,
        metric_labels::NETWORK_PROTOCOL_NAME,
        metric_labels::NETWORK_PROTOCOL_VERSION,
        metric_labels::URL_SCHEME,
    ] {
        let prometheus_label = label.replace('.', "_");
        assert!(
            scraped_labels().contains(&prometheus_label),
            "{label} names no attribute this layer records; scraped: {:?}",
            scraped_labels()
        );
    }
}

/// Metric family names in the registry, as Prometheus spells them.
fn scraped_family_names() -> Vec<String> {
    PROMETHEUS_REGISTRY
        .get()
        .expect("init_meter_provider installs the registry")
        .gather()
        .iter()
        .map(|family| family.name().to_string())
        .collect()
}

/// Every attribute name attached to any measurement in the registry.
fn scraped_labels() -> Vec<String> {
    PROMETHEUS_REGISTRY
        .get()
        .expect("init_meter_provider installs the registry")
        .gather()
        .iter()
        .flat_map(|family| family.get_metric().to_vec())
        .flat_map(|metric| metric.get_label().to_vec())
        .map(|label| label.name().to_string())
        .collect()
}
