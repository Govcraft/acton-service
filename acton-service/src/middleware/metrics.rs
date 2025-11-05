//! Metrics middleware for HTTP observability
//!
//! Provides OpenTelemetry metrics integration for tracking
//! request counts, latencies, and status codes.

use std::time::Duration;

#[cfg(feature = "otel-metrics")]
use tower_otel_http_metrics::HTTPMetricsLayerBuilder;

/// Configuration for HTTP metrics
#[derive(Debug, Clone)]
pub struct MetricsConfig {
    /// Enable metrics collection
    pub enabled: bool,
    /// Service name for metrics
    pub service_name: String,
    /// Include request path in metrics
    pub include_path: bool,
    /// Include request method in metrics
    pub include_method: bool,
    /// Include status code in metrics
    pub include_status: bool,
    /// Histogram buckets for latency (in milliseconds)
    pub latency_buckets: Vec<f64>,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            service_name: "acton-service".to_string(),
            include_path: true,
            include_method: true,
            include_status: true,
            // Default buckets: 5ms, 10ms, 25ms, 50ms, 100ms, 250ms, 500ms, 1s, 2.5s, 5s, 10s
            latency_buckets: vec![
                5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 10000.0,
            ],
        }
    }
}

impl MetricsConfig {
    /// Create a new metrics configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set metrics enabled
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set service name
    pub fn with_service_name(mut self, name: impl Into<String>) -> Self {
        self.service_name = name.into();
        self
    }

    /// Set whether to include path in metrics
    pub fn with_include_path(mut self, include: bool) -> Self {
        self.include_path = include;
        self
    }

    /// Set whether to include method in metrics
    pub fn with_include_method(mut self, include: bool) -> Self {
        self.include_method = include;
        self
    }

    /// Set whether to include status code in metrics
    pub fn with_include_status(mut self, include: bool) -> Self {
        self.include_status = include;
        self
    }

    /// Set custom latency histogram buckets (in milliseconds)
    pub fn with_latency_buckets(mut self, buckets: Vec<f64>) -> Self {
        self.latency_buckets = buckets;
        self
    }

    /// Convert milliseconds to Duration for latency buckets
    pub fn latency_buckets_as_duration(&self) -> Vec<Duration> {
        self.latency_buckets
            .iter()
            .map(|&ms| Duration::from_millis(ms as u64))
            .collect()
    }
}

/// Standard metric names following OpenTelemetry conventions
pub mod metric_names {
    /// HTTP server request count
    pub const HTTP_SERVER_REQUEST_COUNT: &str = "http.server.request.count";
    /// HTTP server request duration
    pub const HTTP_SERVER_REQUEST_DURATION: &str = "http.server.request.duration";
    /// HTTP server active requests
    pub const HTTP_SERVER_ACTIVE_REQUESTS: &str = "http.server.active_requests";
    /// HTTP server request size
    pub const HTTP_SERVER_REQUEST_SIZE: &str = "http.server.request.size";
    /// HTTP server response size
    pub const HTTP_SERVER_RESPONSE_SIZE: &str = "http.server.response.size";
}

/// Standard metric labels following OpenTelemetry conventions
pub mod metric_labels {
    /// HTTP method (GET, POST, etc.)
    pub const HTTP_METHOD: &str = "http.method";
    /// HTTP route/path
    pub const HTTP_ROUTE: &str = "http.route";
    /// HTTP status code
    pub const HTTP_STATUS_CODE: &str = "http.status_code";
    /// Service name
    pub const SERVICE_NAME: &str = "service.name";
    /// Service version
    pub const SERVICE_VERSION: &str = "service.version";
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
///     .with_service_name("my-service");
///
/// let layer = create_metrics_layer(&config);
/// # /*
/// let app = ServiceBuilder::new()
///     .layer(layer)
///     .service(my_service);
/// # */
/// ```
#[cfg(feature = "otel-metrics")]
pub fn create_metrics_layer(
    config: &MetricsConfig,
) -> Option<tower_otel_http_metrics::HTTPMetricsLayer<
    tower_otel_http_metrics::NoOpExtractor,
    tower_otel_http_metrics::NoOpExtractor,
>> {
    if !config.enabled {
        tracing::info!("HTTP metrics disabled in configuration");
        return None;
    }

    // Get the global meter from the meter provider
    let meter = crate::observability::get_meter()?;

    // Build the metrics layer
    match HTTPMetricsLayerBuilder::builder()
        .with_meter(meter)
        .build()
    {
        Ok(layer) => {
            tracing::info!(
                service_name = %config.service_name,
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
#[cfg(not(feature = "otel-metrics"))]
pub fn create_metrics_layer(_config: &MetricsConfig) -> Option<()> {
    tracing::info!("HTTP metrics not available (otel-metrics feature disabled)");
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = MetricsConfig::default();
        assert!(config.enabled);
        assert!(config.include_path);
        assert!(config.include_method);
        assert!(config.include_status);
        assert_eq!(config.service_name, "acton-service");
    }

    #[test]
    fn test_builder_pattern() {
        let config = MetricsConfig::new()
            .with_enabled(true)
            .with_service_name("test-service")
            .with_include_path(false)
            .with_latency_buckets(vec![10.0, 50.0, 100.0]);

        assert!(config.enabled);
        assert_eq!(config.service_name, "test-service");
        assert!(!config.include_path);
        assert_eq!(config.latency_buckets, vec![10.0, 50.0, 100.0]);
    }

    #[test]
    fn test_latency_buckets_conversion() {
        let config = MetricsConfig::new()
            .with_latency_buckets(vec![10.0, 100.0, 1000.0]);

        let durations = config.latency_buckets_as_duration();
        assert_eq!(durations.len(), 3);
        assert_eq!(durations[0], Duration::from_millis(10));
        assert_eq!(durations[1], Duration::from_millis(100));
        assert_eq!(durations[2], Duration::from_millis(1000));
    }

    #[test]
    fn test_metric_names() {
        assert_eq!(metric_names::HTTP_SERVER_REQUEST_COUNT, "http.server.request.count");
        assert_eq!(metric_names::HTTP_SERVER_REQUEST_DURATION, "http.server.request.duration");
    }

    #[test]
    fn test_metric_labels() {
        assert_eq!(metric_labels::HTTP_METHOD, "http.method");
        assert_eq!(metric_labels::HTTP_STATUS_CODE, "http.status_code");
    }

    #[test]
    fn test_create_metrics_layer_disabled() {
        let config = MetricsConfig::new().with_enabled(false);
        let layer = create_metrics_layer(&config);
        assert!(layer.is_none(), "Should return None when metrics are disabled");
    }

    #[test]
    #[cfg(feature = "otel-metrics")]
    fn test_create_metrics_layer_without_meter_provider() {
        // Without initializing the meter provider, should return None
        let config = MetricsConfig::new().with_enabled(true);
        let layer = create_metrics_layer(&config);
        assert!(layer.is_none(), "Should return None when meter provider is not initialized");
    }

    #[test]
    fn test_metrics_config_custom_buckets() {
        let custom_buckets = vec![1.0, 5.0, 10.0];
        let config = MetricsConfig::new()
            .with_latency_buckets(custom_buckets.clone());

        assert_eq!(config.latency_buckets, custom_buckets);

        let durations = config.latency_buckets_as_duration();
        assert_eq!(durations.len(), 3);
        assert_eq!(durations[0], std::time::Duration::from_millis(1));
        assert_eq!(durations[1], std::time::Duration::from_millis(5));
        assert_eq!(durations[2], std::time::Duration::from_millis(10));
    }
}
