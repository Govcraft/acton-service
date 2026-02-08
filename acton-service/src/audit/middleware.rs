//! Audit middleware for HTTP request logging
//!
//! Provides a Tower layer that automatically captures HTTP request/response
//! details as audit events. Supports per-route annotation and route filtering.

use axum::{
    body::Body,
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use std::time::Instant;

use super::event::{AuditEvent, AuditEventKind, AuditSeverity, AuditSource};
use super::logger::AuditLogger;

/// Per-route audit annotation
///
/// Apply to specific routes to override the default event kind.
///
/// # Example
///
/// ```rust,ignore
/// Router::new()
///     .route("/admin/users/:id", delete(handler))
///     .layer(Extension(AuditRoute::new("user.delete")))
/// ```
#[derive(Clone, Debug)]
pub struct AuditRoute {
    /// Custom event name for this route
    pub name: String,
}

impl AuditRoute {
    /// Create a new audit route annotation
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

/// Create an audit route annotation layer for use with specific routes
///
/// This adds an `AuditRoute` extension to matching requests, which the global
/// audit middleware picks up to emit a custom-named audit event.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::audit::middleware::audit_layer;
///
/// Router::new()
///     .route("/admin/users/:id", delete(handler).layer(audit_layer("user.delete")))
/// ```
pub fn audit_layer(name: &str) -> axum::Extension<AuditRoute> {
    axum::Extension(AuditRoute::new(name))
}

/// Audit middleware function
///
/// This middleware captures HTTP request/response details as audit events.
/// It checks the audit configuration to decide whether to audit a request:
///
/// 1. If the route has an `AuditRoute` extension, always audit with that name
/// 2. If `audit_all_requests` is true, audit everything (except excluded routes)
/// 3. If the request path matches an `audited_routes` pattern, audit it
///
/// This is applied as a global middleware by `ServiceBuilder::build()`.
pub async fn audit_middleware(
    State(logger): State<AuditLogger>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let method = request.method().to_string();
    let path = request.uri().path().to_string();

    // Extract audit route annotation if present
    let audit_route = request.extensions().get::<AuditRoute>().cloned();

    // Decide whether to audit this request based on config
    let should_audit = if audit_route.is_some() {
        // Per-route annotation always triggers auditing
        true
    } else {
        let config = logger.config();

        // Check exclusions first
        if path_matches_patterns(&path, &config.excluded_routes) {
            false
        } else if config.audit_all_requests {
            true
        } else {
            path_matches_patterns(&path, &config.audited_routes)
        }
    };

    if !should_audit {
        return next.run(request).await;
    }

    // Extract source information from request
    let source = AuditSource {
        ip: request
            .headers()
            .get("x-forwarded-for")
            .or_else(|| request.headers().get("x-real-ip"))
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(',').next().unwrap_or(s).trim().to_string()),
        user_agent: request
            .headers()
            .get("user-agent")
            .and_then(|v| v.to_str().ok())
            .map(String::from),
        subject: request
            .extensions()
            .get::<crate::middleware::Claims>()
            .map(|c| c.sub.clone()),
        request_id: request
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(String::from),
    };

    let start = Instant::now();
    let response = next.run(request).await;
    let duration_ms = start.elapsed().as_millis() as u64;

    let status_code = response.status().as_u16();

    // Determine the event kind
    let kind = if let Some(ref route) = audit_route {
        AuditEventKind::Custom(route.name.clone())
    } else {
        AuditEventKind::HttpRequest
    };

    // Determine severity based on status code
    let severity = if status_code >= 500 {
        AuditSeverity::Error
    } else if status_code >= 400 {
        AuditSeverity::Warning
    } else {
        AuditSeverity::Informational
    };

    let event = AuditEvent::new(kind, severity, logger.service_name().to_string())
        .with_source(source)
        .with_http(method, path, Some(status_code), Some(duration_ms));

    logger.log(event).await;

    response
}

/// Check if a path matches any of the given glob patterns
///
/// Supports simple wildcard matching:
/// - `*` matches any single path segment
/// - `**` or trailing `/*` matches any remaining segments
pub fn path_matches_patterns(path: &str, patterns: &[String]) -> bool {
    for pattern in patterns {
        if path_matches_glob(path, pattern) {
            return true;
        }
    }
    false
}

/// Simple glob matching for route patterns
fn path_matches_glob(path: &str, pattern: &str) -> bool {
    // Exact match
    if path == pattern {
        return true;
    }

    // Trailing wildcard: "/api/v1/admin/*" matches "/api/v1/admin/anything"
    if let Some(prefix) = pattern.strip_suffix("/*") {
        return path.starts_with(prefix) && path.len() > prefix.len();
    }
    if let Some(prefix) = pattern.strip_suffix("/**") {
        return path.starts_with(prefix);
    }

    // Simple star matching within segments
    if pattern.contains('*') {
        let parts: Vec<&str> = pattern.split('*').collect();
        if parts.len() == 2 {
            return path.starts_with(parts[0]) && path.ends_with(parts[1]);
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_matches_exact() {
        assert!(path_matches_glob("/api/v1/users", "/api/v1/users"));
        assert!(!path_matches_glob("/api/v1/users", "/api/v1/posts"));
    }

    #[test]
    fn test_path_matches_trailing_wildcard() {
        assert!(path_matches_glob(
            "/api/v1/admin/users",
            "/api/v1/admin/*"
        ));
        assert!(path_matches_glob(
            "/api/v1/admin/settings",
            "/api/v1/admin/*"
        ));
        assert!(!path_matches_glob("/api/v1/users", "/api/v1/admin/*"));
    }

    #[test]
    fn test_path_matches_double_wildcard() {
        assert!(path_matches_glob(
            "/api/v1/admin/users/123",
            "/api/v1/admin/**"
        ));
        assert!(path_matches_glob("/api/v1/admin", "/api/v1/admin/**"));
    }

    #[test]
    fn test_path_matches_patterns_list() {
        let patterns = vec![
            "/api/v1/admin/*".to_string(),
            "/api/v1/users/*/delete".to_string(),
        ];

        assert!(path_matches_patterns("/api/v1/admin/settings", &patterns));
        assert!(path_matches_patterns(
            "/api/v1/users/123/delete",
            &patterns
        ));
        assert!(!path_matches_patterns("/api/v1/posts", &patterns));
    }
}
