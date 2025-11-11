//! Middleware modules for authentication, rate limiting, and more

pub mod jwt;
pub mod rate_limit;
pub mod request_tracking;

#[cfg(feature = "resilience")]
pub mod resilience;

#[cfg(feature = "otel-metrics")]
pub mod metrics;

#[cfg(feature = "governor")]
pub mod governor;

pub use jwt::{Claims, JwtAuth};

#[cfg(feature = "cache")]
pub use jwt::{JwtRevocation, RedisJwtRevocation};
pub use rate_limit::RateLimit;
pub use request_tracking::{
    request_id_layer, request_id_propagation_layer, sensitive_headers_layer,
    RequestTrackingConfig, PROPAGATE_HEADERS, SENSITIVE_HEADERS,
};

#[cfg(feature = "resilience")]
pub use resilience::ResilienceConfig;

#[cfg(feature = "otel-metrics")]
pub use metrics::{MetricsConfig, metric_labels, metric_names};

#[cfg(feature = "governor")]
pub use governor::{GovernorConfig, RateLimitExceeded};
