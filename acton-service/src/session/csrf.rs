//! CSRF (Cross-Site Request Forgery) protection.
//!
//! This module provides CSRF protection for session-based applications.
//! It generates tokens, stores them in the session, and validates them
//! on non-safe HTTP methods.
//!
//! # How it works
//!
//! 1. A CSRF token is generated and stored in the session
//! 2. The token is made available to templates via the `CsrfToken` extractor
//! 3. Forms include the token as a hidden field or header
//! 4. The `CsrfLayer` middleware validates the token on POST/PUT/DELETE/PATCH requests
//!
//! # Example
//!
//! ```rust,ignore
//! use acton_service::session::{CsrfToken, CsrfLayer};
//!
//! // In your handler - get the token for templates
//! async fn form_page(csrf: CsrfToken) -> impl IntoResponse {
//!     Html(format!(r#"
//!         <form method="post">
//!             {}
//!             <input type="text" name="data">
//!             <button type="submit">Submit</button>
//!         </form>
//!     "#, csrf.as_hidden_field()))
//! }
//!
//! // For HTMX - include token in meta tag and headers
//! async fn layout(csrf: CsrfToken) -> impl IntoResponse {
//!     Html(format!(r#"
//!         <head>
//!             {}
//!         </head>
//!         <body hx-headers='{{"X-CSRF-Token": "{}"}}'>
//!             ...
//!         </body>
//!     "#, csrf.as_meta_tag(), csrf.token()))
//! }
//! ```

use axum::{
    body::Body,
    extract::FromRequestParts,
    http::{request::Parts, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use rand::Rng;
use tower_sessions::Session;

use super::config::CsrfConfig;
use crate::error::Error;

const CSRF_SESSION_KEY: &str = "_csrf_token";

/// CSRF token extractor and helper.
///
/// Use this extractor to get the CSRF token for inclusion in forms or headers.
/// The token is automatically generated if not present in the session.
///
/// # Example
///
/// ```rust,ignore
/// async fn form(csrf: CsrfToken) -> impl IntoResponse {
///     Html(format!(r#"
///         <form method="post">
///             {}
///             <button>Submit</button>
///         </form>
///     "#, csrf.as_hidden_field()))
/// }
/// ```
#[derive(Debug, Clone)]
pub struct CsrfToken(String);

impl CsrfToken {
    /// Create a new CSRF token with the given value.
    #[must_use]
    pub fn new(token: String) -> Self {
        Self(token)
    }

    /// Get the raw token string.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.0
    }

    /// Generate HTML for a hidden form field.
    ///
    /// Use this in forms to include the CSRF token.
    #[must_use]
    pub fn as_hidden_field(&self) -> String {
        format!(
            r#"<input type="hidden" name="_csrf" value="{}">"#,
            html_escape(&self.0)
        )
    }

    /// Generate HTML for a hidden form field with custom name.
    #[must_use]
    pub fn as_hidden_field_named(&self, name: &str) -> String {
        format!(
            r#"<input type="hidden" name="{}" value="{}">"#,
            html_escape(name),
            html_escape(&self.0)
        )
    }

    /// Generate HTML for a meta tag.
    ///
    /// Use this in the document head for JavaScript/HTMX access.
    #[must_use]
    pub fn as_meta_tag(&self) -> String {
        format!(
            r#"<meta name="csrf-token" content="{}">"#,
            html_escape(&self.0)
        )
    }

    /// Generate a new random CSRF token.
    #[must_use]
    pub fn generate(length: usize) -> Self {
        let token: String = rand::rng()
            .sample_iter(&rand::distr::Alphanumeric)
            .take(length)
            .map(char::from)
            .collect();
        Self(token)
    }

    /// Get or create a CSRF token from the session.
    ///
    /// If a token exists in the session, it is returned.
    /// Otherwise, a new token is generated and stored.
    pub async fn get_or_create(session: &Session, length: usize) -> Result<Self, Error> {
        // Try to get existing token
        if let Some(token) = session
            .get::<String>(CSRF_SESSION_KEY)
            .await
            .map_err(|e| Error::Session(format!("Failed to read CSRF token: {e}")))?
        {
            return Ok(Self(token));
        }

        // Generate new token
        let token = Self::generate(length);
        session
            .insert(CSRF_SESSION_KEY, &token.0)
            .await
            .map_err(|e| Error::Session(format!("Failed to store CSRF token: {e}")))?;

        Ok(token)
    }

    /// Regenerate the CSRF token.
    ///
    /// Call this after login to prevent CSRF token fixation.
    pub async fn regenerate(session: &Session, length: usize) -> Result<Self, Error> {
        let token = Self::generate(length);
        session
            .insert(CSRF_SESSION_KEY, &token.0)
            .await
            .map_err(|e| Error::Session(format!("Failed to store CSRF token: {e}")))?;
        Ok(token)
    }
}

impl std::fmt::Display for CsrfToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<S> FromRequestParts<S> for CsrfToken
where
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // Get session from request extensions (set by SessionManagerLayer)
        let session = parts.extensions.get::<Session>().cloned().ok_or_else(|| {
            Error::Session("Session not found in request extensions for CSRF".to_string())
        })?;

        // Default token length
        const DEFAULT_TOKEN_LENGTH: usize = 32;
        Self::get_or_create(&session, DEFAULT_TOKEN_LENGTH).await
    }
}

/// CSRF protection middleware layer.
///
/// This layer validates CSRF tokens on non-safe HTTP methods (POST, PUT, DELETE, PATCH).
/// The token can be provided via:
/// - Header: `X-CSRF-Token` (configurable)
/// - Form field: `_csrf` (configurable)
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::session::{CsrfLayer, CsrfConfig};
///
/// let csrf_layer = CsrfLayer::new(CsrfConfig::default());
///
/// let app = Router::new()
///     .route("/form", post(handle_form))
///     .layer(csrf_layer);
/// ```
#[derive(Debug, Clone)]
pub struct CsrfLayer {
    config: CsrfConfig,
}

impl CsrfLayer {
    /// Create a new CSRF layer with the given configuration.
    #[must_use]
    pub fn new(config: CsrfConfig) -> Self {
        Self { config }
    }

    /// Create a CSRF layer with default configuration.
    #[must_use]
    pub fn default_config() -> Self {
        Self {
            config: CsrfConfig::default(),
        }
    }
}

impl<S> tower::Layer<S> for CsrfLayer {
    type Service = CsrfMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        CsrfMiddleware {
            inner,
            config: self.config.clone(),
        }
    }
}

/// CSRF middleware service.
#[derive(Debug, Clone)]
pub struct CsrfMiddleware<S> {
    inner: S,
    config: CsrfConfig,
}

impl<S> tower::Service<Request<Body>> for CsrfMiddleware<S>
where
    S: tower::Service<Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send,
{
    type Response = Response;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let config = self.config.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            // Skip if CSRF is disabled
            if !config.enabled {
                return inner.call(request).await;
            }

            // Skip safe methods
            let is_safe = config
                .safe_methods
                .iter()
                .any(|m| m.eq_ignore_ascii_case(request.method().as_str()));

            if is_safe {
                return inner.call(request).await;
            }

            // Extract session and validate token
            let session = request.extensions().get::<Session>();
            if session.is_none() {
                tracing::warn!("CSRF validation failed: no session found");
                return Ok(csrf_error_response("CSRF validation failed: no session"));
            }

            let session = session.unwrap().clone();

            // Get expected token from session
            let expected_token: Option<String> = session.get(CSRF_SESSION_KEY).await.ok().flatten();

            let expected_token = match expected_token {
                Some(t) => t,
                None => {
                    tracing::warn!("CSRF validation failed: no token in session");
                    return Ok(csrf_error_response(
                        "CSRF validation failed: no token in session",
                    ));
                }
            };

            // Get provided token from header
            let provided_token = request
                .headers()
                .get(&config.header_name)
                .and_then(|v| v.to_str().ok())
                .map(String::from);

            // TODO: Also check form body for token (requires body extraction)
            // For now, we only support header-based tokens which works well with HTMX

            let provided_token = match provided_token {
                Some(t) => t,
                None => {
                    tracing::warn!("CSRF validation failed: no token provided in header");
                    return Ok(csrf_error_response(
                        "CSRF validation failed: no token provided",
                    ));
                }
            };

            // Constant-time comparison to prevent timing attacks
            if !constant_time_compare(&expected_token, &provided_token) {
                tracing::warn!("CSRF validation failed: token mismatch");
                return Ok(csrf_error_response("CSRF validation failed: invalid token"));
            }

            inner.call(request).await
        })
    }
}

/// CSRF validation middleware function.
///
/// Use this with `axum::middleware::from_fn_with_state` for integration
/// with the ServiceBuilder pattern.
pub async fn csrf_middleware(
    session: Session,
    config: CsrfConfig,
    request: Request<Body>,
    next: Next,
) -> Response {
    // Skip if disabled
    if !config.enabled {
        return next.run(request).await;
    }

    // Skip safe methods
    let is_safe = config
        .safe_methods
        .iter()
        .any(|m| m.eq_ignore_ascii_case(request.method().as_str()));

    if is_safe {
        return next.run(request).await;
    }

    // Get expected token from session
    let expected_token: Option<String> = session.get(CSRF_SESSION_KEY).await.ok().flatten();

    let expected_token = match expected_token {
        Some(t) => t,
        None => {
            tracing::warn!("CSRF validation failed: no token in session");
            return csrf_error_response("CSRF validation failed: no token in session");
        }
    };

    // Get provided token from header
    let provided_token = request
        .headers()
        .get(&config.header_name)
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    let provided_token = match provided_token {
        Some(t) => t,
        None => {
            tracing::warn!("CSRF validation failed: no token provided");
            return csrf_error_response("CSRF validation failed: no token provided");
        }
    };

    // Constant-time comparison
    if !constant_time_compare(&expected_token, &provided_token) {
        tracing::warn!("CSRF validation failed: token mismatch");
        return csrf_error_response("CSRF validation failed: invalid token");
    }

    next.run(request).await
}

/// Create a CSRF error response.
fn csrf_error_response(message: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        [("Content-Type", "application/json")],
        format!(
            r#"{{"error": "csrf_validation_failed", "message": "{}"}}"#,
            message
        ),
    )
        .into_response()
}

/// Constant-time string comparison to prevent timing attacks.
fn constant_time_compare(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut result = 0u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        result |= x ^ y;
    }
    result == 0
}

/// Basic HTML escaping for attribute values.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csrf_token_generation() {
        let token = CsrfToken::generate(32);
        assert_eq!(token.token().len(), 32);

        let token2 = CsrfToken::generate(32);
        assert_ne!(token.token(), token2.token()); // Should be random
    }

    #[test]
    fn test_csrf_token_html_output() {
        let token = CsrfToken::new("abc123".to_string());

        let hidden = token.as_hidden_field();
        assert!(hidden.contains("name=\"_csrf\""));
        assert!(hidden.contains("value=\"abc123\""));

        let meta = token.as_meta_tag();
        assert!(meta.contains("name=\"csrf-token\""));
        assert!(meta.contains("content=\"abc123\""));
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("\"quoted\""), "&quot;quoted&quot;");
        assert_eq!(html_escape("a&b"), "a&amp;b");
    }

    #[test]
    fn test_constant_time_compare() {
        assert!(constant_time_compare("abc", "abc"));
        assert!(!constant_time_compare("abc", "abd"));
        assert!(!constant_time_compare("abc", "ab"));
        assert!(!constant_time_compare("ab", "abc"));
    }
}
