//! Resilience middleware for fault tolerance and reliability
//!
//! This module provides production-ready circuit breaker and bulkhead patterns
//! using tower-resilience to ensure service stability and graceful degradation.
//!
//! Retry is deliberately absent. Retrying means replaying a request, and an
//! inbound `Request<Body>` wraps a stream that is consumed once -- the retry
//! layer requires `Req: Clone`, which it cannot satisfy. Retry belongs on
//! outbound client stacks, which callers compose themselves.

use std::convert::Infallible;
use std::time::Duration;

use axum::http::StatusCode;
use axum::response::Response;
use axum::error_handling::HandleErrorLayer;
use axum::Router;

use tower::{BoxError, ServiceBuilder};

pub use tower_resilience_bulkhead::BulkheadLayer;
pub use tower_resilience_circuitbreaker::{
    CircuitBreakerConfigBuilder, CircuitBreakerLayer, CircuitState,
};
use tower_resilience_circuitbreaker::classifier::FnClassifier;

/// Failure classification used by [`ResilienceConfig::http_circuit_breaker_layer`].
///
/// A plain `fn` pointer (rather than a closure) keeps this type nameable in
/// public signatures.
pub type HttpFailureClassifier = FnClassifier<HttpClassifierFn>;

/// Signature of the inbound-HTTP failure classifier.
pub type HttpClassifierFn = fn(&Result<Response, Infallible>) -> bool;

/// Treat any 5xx response as a circuit-breaker failure.
///
/// Inbound axum routes are infallible, so a failing handler produces
/// `Ok(Response)` with a server-error status rather than `Err`.
fn is_server_error(result: &Result<Response, Infallible>) -> bool {
    result
        .as_ref()
        .is_ok_and(|response| response.status().is_server_error())
}

/// Configuration for resilience patterns
#[derive(Debug, Clone)]
pub struct ResilienceConfig {
    /// Enable circuit breaker
    pub circuit_breaker_enabled: bool,
    /// Failure threshold before circuit opens (0.0-1.0)
    pub circuit_breaker_threshold: f64,
    /// Minimum requests before calculating failure rate
    pub circuit_breaker_min_requests: u64,
    /// Duration to wait before attempting to close circuit
    pub circuit_breaker_wait_duration: Duration,

    /// Enable bulkhead (concurrency limiting)
    pub bulkhead_enabled: bool,
    /// Maximum concurrent requests
    pub bulkhead_max_concurrent: usize,
    /// Maximum wait time for request slot
    pub bulkhead_max_wait: Duration,
}

impl Default for ResilienceConfig {
    fn default() -> Self {
        Self {
            circuit_breaker_enabled: true,
            circuit_breaker_threshold: 0.5, // 50% failure rate
            circuit_breaker_min_requests: 10,
            circuit_breaker_wait_duration: Duration::from_secs(30),

            bulkhead_enabled: true,
            bulkhead_max_concurrent: 100,
            bulkhead_max_wait: Duration::from_secs(5),
        }
    }
}

/// Bridge the TOML-facing config to the layer-building config.
///
/// [`crate::config::ResilienceConfig`] is what `[middleware.resilience]`
/// deserializes into and stores its durations as integer seconds/milliseconds;
/// this type is what actually builds layers and uses [`Duration`].
impl From<&crate::config::ResilienceConfig> for ResilienceConfig {
    fn from(config: &crate::config::ResilienceConfig) -> Self {
        Self {
            circuit_breaker_enabled: config.circuit_breaker_enabled,
            circuit_breaker_threshold: config.circuit_breaker_threshold,
            circuit_breaker_min_requests: config.circuit_breaker_min_requests,
            circuit_breaker_wait_duration: config.circuit_breaker_wait_duration(),

            bulkhead_enabled: config.bulkhead_enabled,
            bulkhead_max_concurrent: config.bulkhead_max_concurrent,
            bulkhead_max_wait: config.bulkhead_max_wait(),
        }
    }
}

impl ResilienceConfig {
    /// Create a new resilience configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set circuit breaker enabled
    pub fn with_circuit_breaker(mut self, enabled: bool) -> Self {
        self.circuit_breaker_enabled = enabled;
        self
    }

    /// Set circuit breaker threshold
    pub fn with_circuit_breaker_threshold(mut self, threshold: f64) -> Self {
        self.circuit_breaker_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Set bulkhead enabled
    pub fn with_bulkhead(mut self, enabled: bool) -> Self {
        self.bulkhead_enabled = enabled;
        self
    }

    /// Set bulkhead maximum concurrent requests
    pub fn with_bulkhead_max_concurrent(mut self, max: usize) -> Self {
        self.bulkhead_max_concurrent = max;
        self
    }

    /// Create circuit breaker layer from configuration
    ///
    /// Returns a configured circuit breaker layer that can be applied to outbound
    /// (client) service stacks, where transport failures surface as `Err`.
    ///
    /// The layer counts `Err` results as failures and opens once the failure rate
    /// exceeds the configured threshold. For **inbound** axum stacks use
    /// [`Self::http_circuit_breaker_layer`] instead — an inbound `Route` is
    /// infallible, so a 5xx arrives as `Ok(Response)` and this layer would never
    /// trip. See [`apply_resilience`] for the full router-ready stack.
    ///
    /// # Example
    /// ```
    /// use acton_service::middleware::resilience::ResilienceConfig;
    ///
    /// let config = ResilienceConfig::default();
    /// if let Some(layer) = config.circuit_breaker_layer() {
    ///     // Apply layer to your outbound service stack
    ///     let _ = layer;
    /// }
    /// ```
    pub fn circuit_breaker_layer(&self) -> Option<CircuitBreakerLayer> {
        if !self.circuit_breaker_enabled {
            return None;
        }

        Some(
            self.circuit_breaker_builder()
                .on_state_transition(Self::log_state_transition)
                .build_with_handle()
                .0,
        )
    }

    /// Create a circuit breaker layer that classifies HTTP 5xx responses as failures
    ///
    /// Inbound axum routes are infallible: a handler returning 500 yields
    /// `Ok(Response)`, not `Err`. The default classifier only counts `Err` as a
    /// failure, so a breaker built by [`Self::circuit_breaker_layer`] would never
    /// open on an inbound router. This layer classifies any response with a
    /// server-error status as a failure instead.
    ///
    /// The resulting layer changes the service error type, so it must be paired
    /// with an error handler before attaching to a [`Router`]. Prefer
    /// [`apply_resilience`], which wires that up for you.
    ///
    /// [`Router`]: axum::Router
    pub fn http_circuit_breaker_layer(
        &self,
    ) -> Option<CircuitBreakerLayer<HttpFailureClassifier>> {
        if !self.circuit_breaker_enabled {
            return None;
        }

        Some(
            self.circuit_breaker_builder()
                .on_state_transition(Self::log_state_transition)
                .failure_classifier(is_server_error as HttpClassifierFn)
                .build_with_handle()
                .0,
        )
    }

    /// Shared circuit breaker builder settings, independent of failure classification.
    ///
    /// Callers must finish with `build_with_handle()` rather than `build()`.
    /// axum re-invokes `Layer::layer` on every request, and `build()` mints a
    /// fresh circuit on each application -- the failure window would reset
    /// between requests and the breaker could never open.
    fn circuit_breaker_builder(&self) -> CircuitBreakerConfigBuilder {
        CircuitBreakerLayer::builder()
            .name("acton-circuit-breaker")
            .failure_rate_threshold(self.circuit_breaker_threshold)
            .sliding_window_size(self.circuit_breaker_min_requests as usize)
            .wait_duration_in_open(self.circuit_breaker_wait_duration)
    }

    fn log_state_transition(from: CircuitState, to: CircuitState) {
        tracing::warn!(
            from = ?from,
            to = ?to,
            "Circuit breaker state transition"
        );
    }

    /// Create bulkhead layer from configuration
    ///
    /// The returned layer carries a **shared** semaphore. This matters: axum
    /// re-invokes `Layer::layer` on every request, and a layer built with the
    /// plain `build()` mints fresh state each time -- giving every request its
    /// own full set of permits, so the concurrency cap would silently never
    /// apply. See [`apply_resilience`].
    pub fn bulkhead_layer(&self) -> Option<BulkheadLayer> {
        if !self.bulkhead_enabled {
            return None;
        }

        Some(
            BulkheadLayer::builder()
                .name("acton-bulkhead")
                .max_concurrent_calls(self.bulkhead_max_concurrent)
                .max_wait_duration(self.bulkhead_max_wait)
                .on_call_permitted(|concurrent| {
                    tracing::debug!(
                        concurrent_requests = concurrent,
                        "Request permitted through bulkhead"
                    );
                })
                .on_call_rejected(|max| {
                    tracing::warn!(
                        max_concurrent = max,
                        "Request rejected by bulkhead - max concurrent limit reached"
                    );
                })
                .build_with_handle()
                .0,
        )
    }
}

/// Map a resilience rejection onto an HTTP status code.
///
/// The circuit breaker and bulkhead each introduce their own error type, so this
/// is generic over anything boxable. Both rejections mean "the service is
/// shedding load right now", which is a 503; the distinction between them is
/// carried in the log, not the status line.
async fn handle_resilience_error<E: Into<BoxError>>(error: E) -> (StatusCode, &'static str) {
    let error: BoxError = error.into();
    tracing::warn!(error = %error, "Request rejected by resilience middleware");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "Service temporarily unavailable",
    )
}

/// Apply resilience middleware (circuit breaker + bulkhead) to an axum router.
///
/// This is the supported way to use these patterns on **inbound** HTTP. It
/// wires three things that are easy to get wrong by hand:
///
/// 1. A 5xx-aware failure classifier, so the breaker actually trips. An inbound
///    route is infallible, so the default `Err`-only classifier would never open
///    the circuit.
/// 2. An error handler converting the layers' error types back to `Infallible`,
///    which axum's [`Router::layer`] requires.
/// 3. Ordering: the bulkhead is applied first so it sits *inside* the breaker.
///    A concurrency rejection therefore becomes a 503 that the breaker observes
///    as a server error, letting sustained overload open the circuit.
///
/// Layers disabled in the config are omitted entirely. If both are disabled the
/// router is returned untouched.
///
/// # Example
/// ```
/// use acton_service::middleware::resilience::{apply_resilience, ResilienceConfig};
/// use axum::{routing::get, Router};
///
/// let app = Router::new().route("/", get(|| async { "ok" }));
/// let app = apply_resilience(app, &ResilienceConfig::default());
/// ```
pub fn apply_resilience(app: Router, config: &ResilienceConfig) -> Router {
    // Applied first => innermost. A bulkhead rejection is converted to a 503
    // here, which the circuit breaker below then counts as a failure.
    let app = match config.bulkhead_layer() {
        Some(bulkhead) => app.layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(handle_resilience_error))
                .layer(bulkhead),
        ),
        None => app,
    };

    match config.http_circuit_breaker_layer() {
        Some(circuit_breaker) => app.layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(handle_resilience_error))
                .layer(circuit_breaker),
        ),
        None => app,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ResilienceConfig::default();
        assert!(config.circuit_breaker_enabled);
        assert!(config.bulkhead_enabled);
        assert_eq!(config.circuit_breaker_threshold, 0.5);
        assert_eq!(config.bulkhead_max_concurrent, 100);
    }

    #[test]
    fn test_builder_pattern() {
        let config = ResilienceConfig::new()
            .with_circuit_breaker(false)
            .with_bulkhead_max_concurrent(50);

        assert!(!config.circuit_breaker_enabled);
        assert_eq!(config.bulkhead_max_concurrent, 50);
    }

    #[test]
    fn test_threshold_clamping() {
        let config = ResilienceConfig::new().with_circuit_breaker_threshold(1.5);
        assert_eq!(config.circuit_breaker_threshold, 1.0);

        let config = ResilienceConfig::new().with_circuit_breaker_threshold(-0.5);
        assert_eq!(config.circuit_breaker_threshold, 0.0);
    }

    #[test]
    fn test_circuit_breaker_layer_creation() {
        let config = ResilienceConfig::new().with_circuit_breaker(true);
        assert!(config.circuit_breaker_layer().is_some());
        assert!(config.http_circuit_breaker_layer().is_some());

        let config = ResilienceConfig::new().with_circuit_breaker(false);
        assert!(config.circuit_breaker_layer().is_none());
        assert!(config.http_circuit_breaker_layer().is_none());
    }

    #[test]
    fn test_bulkhead_layer_creation() {
        let config = ResilienceConfig::new().with_bulkhead(true);
        assert!(config.bulkhead_layer().is_some());

        let config = ResilienceConfig::new().with_bulkhead(false);
        assert!(config.bulkhead_layer().is_none());
    }

    #[test]
    fn classifier_counts_5xx_as_failure_not_2xx() {
        let ok = Ok(Response::new(axum::body::Body::empty()));
        assert!(!is_server_error(&ok));

        let mut server_error = Response::new(axum::body::Body::empty());
        *server_error.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
        assert!(is_server_error(&Ok(server_error)));

        // A 4xx is the caller's fault, not the service's -- it must not trip.
        let mut bad_request = Response::new(axum::body::Body::empty());
        *bad_request.status_mut() = StatusCode::BAD_REQUEST;
        assert!(!is_server_error(&Ok(bad_request)));
    }
}
