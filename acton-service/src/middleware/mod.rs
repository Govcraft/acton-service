//! Middleware modules for authentication, rate limiting, and more

// Token abstraction layer (always available)
pub mod token;

// PASETO authentication (default)
pub mod paseto;

// Token revocation (requires cache feature)
#[cfg(feature = "cache")]
pub mod revocation;

// JWT authentication (requires jwt feature)
#[cfg(feature = "jwt")]
pub mod jwt;

pub mod rate_limit;
pub mod request_tracking;
pub mod route_matcher;

#[cfg(feature = "resilience")]
pub mod resilience;

#[cfg(feature = "otel-metrics")]
pub mod metrics;

#[cfg(feature = "governor")]
pub mod governor;

pub mod security_headers;

#[cfg(feature = "cedar-authz")]
pub mod cedar;

// Token abstraction exports (always available)
pub use token::{Claims, TokenValidator};

#[cfg(feature = "cache")]
pub use token::TokenRevocation;

// PASETO exports (default)
pub use paseto::PasetoAuth;

// Token revocation exports (requires cache)
#[cfg(feature = "cache")]
pub use revocation::RedisTokenRevocation;

// JWT exports (requires jwt feature)
#[cfg(feature = "jwt")]
pub use jwt::JwtAuth;

// Other middleware exports
pub use rate_limit::RateLimit;
pub use request_tracking::{
    request_id_layer, request_id_propagation_layer, sensitive_headers_layer, RequestTrackingConfig,
    PROPAGATE_HEADERS, SENSITIVE_HEADERS,
};
pub use route_matcher::{normalize_path, CompiledRoutePatterns};

#[cfg(feature = "resilience")]
pub use resilience::ResilienceConfig;

#[cfg(feature = "otel-metrics")]
pub use metrics::{metric_labels, metric_names, MetricsConfig};

#[cfg(feature = "governor")]
pub use governor::{GovernorConfig, GovernorRateLimit, RateLimitExceeded};

#[cfg(feature = "cedar-authz")]
pub use cedar::CedarAuthz;

#[cfg(all(feature = "cedar-authz", feature = "cache"))]
pub use cedar::{PolicyCache, RedisPolicyCache};

#[cfg(all(feature = "cedar-authz", feature = "grpc"))]
pub use cedar::{CedarAuthzLayer, CedarAuthzService};
