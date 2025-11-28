//! Request tracking middleware for distributed tracing
//!
//! Provides request ID generation, propagation, and header management
//! for distributed tracing across microservices.

use tower_http::{
    request_id::{PropagateRequestIdLayer, SetRequestIdLayer},
    sensitive_headers::SetSensitiveRequestHeadersLayer,
};

use crate::ids::MakeTypedRequestId;

/// Headers to propagate between services
pub const PROPAGATE_HEADERS: &[&str] = &[
    "x-request-id",
    "x-trace-id",
    "x-span-id",
    "x-correlation-id",
    "x-client-id",
];

/// Sensitive headers that should be masked in logs
pub const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "x-auth-token",
];

/// Configuration for request tracking
#[derive(Debug, Clone)]
pub struct RequestTrackingConfig {
    /// Enable request ID generation
    pub request_id_enabled: bool,
    /// Request ID header name
    pub request_id_header: String,
    /// Enable header propagation
    pub propagate_headers: bool,
    /// Enable sensitive header masking
    pub mask_sensitive_headers: bool,
}

impl Default for RequestTrackingConfig {
    fn default() -> Self {
        Self {
            request_id_enabled: true,
            request_id_header: "x-request-id".to_string(),
            propagate_headers: true,
            mask_sensitive_headers: true,
        }
    }
}

impl RequestTrackingConfig {
    /// Create a new request tracking configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set request ID enabled
    pub fn with_request_id(mut self, enabled: bool) -> Self {
        self.request_id_enabled = enabled;
        self
    }

    /// Set custom request ID header name
    pub fn with_request_id_header(mut self, header: impl Into<String>) -> Self {
        self.request_id_header = header.into();
        self
    }

    /// Set header propagation enabled
    pub fn with_header_propagation(mut self, enabled: bool) -> Self {
        self.propagate_headers = enabled;
        self
    }

    /// Set sensitive header masking enabled
    pub fn with_sensitive_header_masking(mut self, enabled: bool) -> Self {
        self.mask_sensitive_headers = enabled;
        self
    }
}

/// Create a request ID layer that generates type-safe request IDs.
///
/// Request IDs use the TypeID format with a "req" prefix and UUIDv7,
/// making them human-readable, type-safe, and time-sortable.
///
/// Example format: `req_01h455vb4pex5vsknk084sn02q`
pub fn request_id_layer() -> SetRequestIdLayer<MakeTypedRequestId> {
    SetRequestIdLayer::x_request_id(MakeTypedRequestId)
}

/// Create a request ID propagation layer
pub fn request_id_propagation_layer() -> PropagateRequestIdLayer {
    PropagateRequestIdLayer::x_request_id()
}

/// Create a sensitive headers layer
pub fn sensitive_headers_layer() -> SetSensitiveRequestHeadersLayer {
    let headers = SENSITIVE_HEADERS
        .iter()
        .map(|h| h.parse().expect("valid header name"))
        .collect::<Vec<_>>();

    SetSensitiveRequestHeadersLayer::new(headers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RequestTrackingConfig::default();
        assert!(config.request_id_enabled);
        assert!(config.propagate_headers);
        assert!(config.mask_sensitive_headers);
        assert_eq!(config.request_id_header, "x-request-id");
    }

    #[test]
    fn test_builder_pattern() {
        let config = RequestTrackingConfig::new()
            .with_request_id(false)
            .with_request_id_header("x-custom-id")
            .with_header_propagation(false);

        assert!(!config.request_id_enabled);
        assert_eq!(config.request_id_header, "x-custom-id");
        assert!(!config.propagate_headers);
    }

    #[test]
    fn test_propagate_headers_constant() {
        assert!(PROPAGATE_HEADERS.contains(&"x-request-id"));
        assert!(PROPAGATE_HEADERS.contains(&"x-trace-id"));
    }

    #[test]
    fn test_sensitive_headers_constant() {
        assert!(SENSITIVE_HEADERS.contains(&"authorization"));
        assert!(SENSITIVE_HEADERS.contains(&"x-api-key"));
    }
}
