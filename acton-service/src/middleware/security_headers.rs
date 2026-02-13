//! Security headers middleware
//!
//! Applies standard HTTP security headers (HSTS, X-Content-Type-Options, etc.)
//! using `tower_http::set_header::SetResponseHeaderLayer`.

use axum::http::HeaderValue;
use axum::Router;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::config::SecurityHeadersConfig;

/// Apply security headers to the router based on configuration.
///
/// `tls_enabled` controls whether HSTS is sent -- HSTS over plain HTTP
/// is meaningless and potentially confusing.
pub fn apply_security_headers(
    mut app: Router,
    config: &SecurityHeadersConfig,
    tls_enabled: bool,
) -> Router {
    if !config.enabled {
        return app;
    }

    // Strict-Transport-Security (only when TLS is active)
    if tls_enabled && config.hsts {
        let mut value = format!("max-age={}", config.hsts_max_age_secs);
        if config.hsts_include_subdomains {
            value.push_str("; includeSubDomains");
        }
        if config.hsts_preload {
            value.push_str("; preload");
        }
        if let Ok(hv) = HeaderValue::from_str(&value) {
            // HSTS uses overriding mode -- framework-set value takes precedence
            app = app.layer(SetResponseHeaderLayer::overriding(
                http::header::STRICT_TRANSPORT_SECURITY,
                hv,
            ));
        }
    }

    // X-Content-Type-Options: nosniff
    if config.x_content_type_options {
        app = app.layer(SetResponseHeaderLayer::if_not_present(
            http::header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ));
    }

    // X-Frame-Options
    if !config.x_frame_options.is_empty() {
        if let Ok(hv) = HeaderValue::from_str(&config.x_frame_options) {
            app = app.layer(SetResponseHeaderLayer::if_not_present(
                http::header::X_FRAME_OPTIONS,
                hv,
            ));
        }
    }

    // X-XSS-Protection: 0 (modern recommendation: disable the browser XSS filter)
    if config.x_xss_protection {
        app = app.layer(SetResponseHeaderLayer::if_not_present(
            http::header::X_XSS_PROTECTION,
            HeaderValue::from_static("0"),
        ));
    }

    // Referrer-Policy
    if !config.referrer_policy.is_empty() {
        if let Ok(hv) = HeaderValue::from_str(&config.referrer_policy) {
            app = app.layer(SetResponseHeaderLayer::if_not_present(
                http::header::REFERRER_POLICY,
                hv,
            ));
        }
    }

    // Permissions-Policy (optional)
    if let Some(ref policy) = config.permissions_policy {
        if let Ok(hv) = HeaderValue::from_str(policy) {
            app = app.layer(SetResponseHeaderLayer::if_not_present(
                http::header::HeaderName::from_static("permissions-policy"),
                hv,
            ));
        }
    }

    app
}
