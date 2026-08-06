//! Metrics middleware for HTTP observability
//!
//! Provides OpenTelemetry metrics integration for tracking
//! request counts, latencies, and status codes.

#[cfg(feature = "_metrics")]
use opentelemetry_instrumentation_tower::HTTPMetricsLayerBuilder;

/// Configuration for HTTP metrics.
///
/// This is [`crate::config::MetricsConfig`] itself, not a copy of it. There used
/// to be two structs of this name -- one parsed from `[middleware.metrics]`, one
/// consumed here -- with a hand-written conversion between them. Every field
/// that conversion carried across was then dropped on the floor by the layer
/// builder, and nothing could tell you so, because the two types agreeing is not
/// the same as the layer honouring them. One type has no gap to fall into.
pub use crate::config::MetricsConfig;

/// The instruments the HTTP metrics layer actually creates.
///
/// These are read back from a live scrape by
/// `tests/metrics_config_reaches_the_scrape.rs`, because a constant naming a
/// metric that is never emitted is worse than no constant: it is a dashboard
/// query that returns nothing, with nothing to say why. There is deliberately
/// no request-count instrument here -- the count is the duration histogram's
/// `_count`, and naming one implied an instrument that never existed.
pub mod metric_names {
    /// Duration of HTTP server requests, in seconds.
    pub const HTTP_SERVER_REQUEST_DURATION: &str = "http.server.request.duration";
    /// Number of in-flight HTTP server requests.
    pub const HTTP_SERVER_ACTIVE_REQUESTS: &str = "http.server.active_requests";
    /// Size of HTTP server request bodies, in bytes.
    pub const HTTP_SERVER_REQUEST_BODY_SIZE: &str = "http.server.request.body.size";
    /// Size of HTTP server response bodies, in bytes.
    pub const HTTP_SERVER_RESPONSE_BODY_SIZE: &str = "http.server.response.body.size";
}

/// The attributes the HTTP metrics layer actually records.
///
/// Post-1.0 semantic-convention names. The pre-1.0 spellings (`http.method`,
/// `http.status_code`) were published here and are not what any instrument
/// emits.
pub mod metric_labels {
    /// HTTP method (GET, POST, ...).
    pub const HTTP_REQUEST_METHOD: &str = "http.request.method";
    /// Matched route template, not the request URI -- this is what bounds
    /// series count to the route table.
    pub const HTTP_ROUTE: &str = "http.route";
    /// HTTP response status code.
    pub const HTTP_RESPONSE_STATUS_CODE: &str = "http.response.status_code";
    /// Network protocol name (`http`).
    pub const NETWORK_PROTOCOL_NAME: &str = "network.protocol.name";
    /// Network protocol version (`1.1`, `2`).
    pub const NETWORK_PROTOCOL_VERSION: &str = "network.protocol.version";
    /// URL scheme (`http`, `https`).
    pub const URL_SCHEME: &str = "url.scheme";
    /// Service name, carried on the resource rather than per measurement, and
    /// set from `[service] name`.
    pub const SERVICE_NAME: &str = "service.name";
}

/// Create the HTTP metrics layer
///
/// This function creates a Tower layer that automatically collects OpenTelemetry
/// metrics for HTTP requests, including:
/// - Request count
/// - Request duration (latency)
/// - Active requests
/// - Request/response sizes
///
/// # Arguments
/// * `config` - Metrics configuration
///
/// # Returns
/// * `Some(layer)` if metrics are enabled and meter provider is available
/// * `None` if metrics are disabled or meter provider is not initialized
///
/// # Example
/// ```rust,no_run
/// use acton_service::middleware::metrics::{MetricsConfig, create_metrics_layer};
/// use tower::ServiceBuilder;
///
/// let config = MetricsConfig::new()
///     .with_latency_buckets_ms(vec![10.0, 100.0, 1000.0]);
///
/// let layer = create_metrics_layer(&config);
/// # /*
/// let app = ServiceBuilder::new()
///     .layer(layer)
///     .service(my_service);
/// # */
/// ```
#[cfg(feature = "_metrics")]
pub fn create_metrics_layer(
    config: &MetricsConfig,
) -> Option<
    opentelemetry_instrumentation_tower::HTTPMetricsLayer<
        opentelemetry_instrumentation_tower::NoOpExtractor,
        opentelemetry_instrumentation_tower::NoOpExtractor,
    >,
> {
    if !config.enabled {
        tracing::info!("HTTP metrics disabled in configuration");
        return None;
    }

    // Get the global meter from the meter provider
    let meter = crate::observability::get_meter()?;

    let mut builder = HTTPMetricsLayerBuilder::builder().with_meter(meter);

    // The instrument is declared in seconds; the key is in milliseconds because
    // that is the scale an operator thinks about a request in.
    let boundaries = config.latency_buckets_seconds();
    match &boundaries {
        Some(seconds) => builder = builder.with_request_duration_bounds(seconds.clone()),
        None => tracing::warn!(
            latency_buckets_ms = ?config.latency_buckets_ms,
            "[middleware.metrics] latency_buckets_ms is empty or not strictly increasing \
             through finite, positive values; using the instrumentation library's default \
             boundaries instead"
        ),
    }

    // Build the metrics layer
    match builder.build() {
        Ok(layer) => {
            tracing::info!(
                latency_buckets_seconds = ?boundaries,
                "HTTP metrics layer initialized"
            );
            Some(layer)
        }
        Err(e) => {
            tracing::error!(
                error = %e,
                "Failed to build HTTP metrics layer"
            );
            None
        }
    }
}

/// Create the HTTP metrics layer (no-op when feature is disabled)
#[cfg(not(feature = "_metrics"))]
pub fn create_metrics_layer(_config: &MetricsConfig) -> Option<()> {
    tracing::info!("HTTP metrics not available (otel-metrics/prometheus-metrics feature disabled)");
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_config_is_enabled_with_seconds_scaled_buckets() {
        let config = MetricsConfig::default();

        assert!(config.enabled);
        let seconds = config
            .latency_buckets_seconds()
            .expect("the default boundaries must be usable");
        assert_eq!(seconds.first().copied(), Some(0.001));
        assert_eq!(seconds.last().copied(), Some(10.0));
    }

    #[test]
    fn the_builder_sets_what_it_says_it_sets() {
        let config = MetricsConfig::new()
            .with_enabled(true)
            .with_latency_buckets_ms(vec![10.0, 50.0, 100.0]);

        assert!(config.enabled);
        assert_eq!(config.latency_buckets_ms, vec![10.0, 50.0, 100.0]);
    }

    #[test]
    fn milliseconds_become_seconds_without_losing_fractions() {
        // `2.5` ms is 2.5 ms, not the 2 ms an integer conversion would give.
        let config = MetricsConfig::new().with_latency_buckets_ms(vec![2.5, 100.0, 1000.0]);

        assert_eq!(
            config.latency_buckets_seconds(),
            Some(vec![0.0025, 0.1, 1.0])
        );
    }

    #[test]
    fn boundaries_that_no_histogram_can_use_are_refused() {
        // Each of these would be accepted by serde and rejected by the SDK, or
        // worse, accepted by both and silently useless.
        for buckets in [
            vec![],
            vec![100.0, 50.0],
            vec![10.0, 10.0],
            vec![0.0, 10.0],
            vec![-1.0, 10.0],
            vec![10.0, f64::INFINITY],
            vec![10.0, f64::NAN],
        ] {
            let config = MetricsConfig::new().with_latency_buckets_ms(buckets.clone());
            assert_eq!(
                config.latency_buckets_seconds(),
                None,
                "{buckets:?} must not reach the instrument"
            );
        }
    }

    #[test]
    fn test_create_metrics_layer_disabled() {
        let config = MetricsConfig::new().with_enabled(false);
        let layer = create_metrics_layer(&config);
        assert!(
            layer.is_none(),
            "Should return None when metrics are disabled"
        );
    }

    #[test]
    #[cfg(feature = "_metrics")]
    fn test_create_metrics_layer_without_meter_provider() {
        // Without initializing the meter provider, should return None
        let config = MetricsConfig::new().with_enabled(true);
        let layer = create_metrics_layer(&config);
        assert!(
            layer.is_none(),
            "Should return None when meter provider is not initialized"
        );
    }
}
