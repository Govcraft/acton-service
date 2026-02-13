//! Lockout middleware for automatic enforcement
//!
//! Optional convenience middleware that automatically enforces login lockout
//! on routes. Extracts the identity from the JSON request body, checks lockout
//! status, and records failures/successes based on the response status code.

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header::HeaderValue, HeaderName, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use super::service::LoginLockout;
use crate::error::{Error, ErrorResponse};

/// Middleware state for automatic lockout enforcement
///
/// Wraps a [`LoginLockout`] service and a JSON field name to extract
/// the identity from the request body. Apply to login routes using
/// `axum::middleware::from_fn_with_state`.
///
/// # Behavior
///
/// 1. Buffers the request body and extracts the identity from the specified JSON field
/// 2. If the content is not JSON, the request passes through without enforcement
/// 3. If the identity is locked, returns HTTP 423 with `Retry-After` header
/// 4. Forwards the request to the inner handler
/// 5. If the response is 401, records a failure and applies progressive delay
/// 6. If the response is 2xx, records a success (clears lockout state)
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::lockout::{LoginLockout, LockoutMiddleware};
///
/// let lockout = LoginLockout::new(config, redis_pool);
/// let mw = LockoutMiddleware::new(lockout, "email");
///
/// let app = Router::new()
///     .route("/login", post(login_handler))
///     .route_layer(axum::middleware::from_fn_with_state(
///         mw,
///         LockoutMiddleware::middleware,
///     ));
/// ```
#[derive(Clone)]
pub struct LockoutMiddleware {
    lockout: LoginLockout,
    identity_field: String,
}

impl LockoutMiddleware {
    /// Create a new lockout middleware
    ///
    /// `identity_field` is the JSON field name to extract from the request
    /// body (e.g., `"email"`, `"username"`).
    pub fn new(lockout: LoginLockout, identity_field: &str) -> Self {
        Self {
            lockout,
            identity_field: identity_field.to_string(),
        }
    }

    /// Middleware function for axum
    ///
    /// Use with `axum::middleware::from_fn_with_state`.
    pub async fn middleware(
        State(mw): State<Self>,
        request: Request<Body>,
        next: Next,
    ) -> Result<Response, Error> {
        // Only process JSON content types
        let is_json = request
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|ct| ct.contains("application/json"))
            .unwrap_or(false);

        if !is_json {
            return Ok(next.run(request).await);
        }

        // Buffer the body to extract identity
        let (parts, body) = request.into_parts();
        let bytes = axum::body::to_bytes(body, 1024 * 1024) // 1MB limit
            .await
            .map_err(|e| Error::BadRequest(format!("Failed to read request body: {}", e)))?;

        // Try to extract identity from JSON
        let identity = serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|v| v.get(&mw.identity_field).cloned())
            .and_then(|v| v.as_str().map(|s| s.to_string()));

        let identity = match identity {
            Some(id) => id,
            None => {
                // Can't extract identity — pass through without enforcement
                let request = Request::from_parts(parts, Body::from(bytes));
                return Ok(next.run(request).await);
            }
        };

        // Check lockout status
        let status = mw.lockout.check(&identity).await?;
        if status.locked {
            let retry_after = status.lockout_remaining_secs;
            let error_response = ErrorResponse::with_code(
                StatusCode::LOCKED,
                "ACCOUNT_LOCKED",
                format!("Account locked. Try again in {} seconds", retry_after),
            );
            let mut response = (StatusCode::LOCKED, axum::Json(error_response)).into_response();
            if let Ok(value) = HeaderValue::from_str(&retry_after.to_string()) {
                response
                    .headers_mut()
                    .insert(HeaderName::from_static("retry-after"), value);
            }
            return Ok(response);
        }

        // Reconstruct request with buffered body and forward
        let request = Request::from_parts(parts, Body::from(bytes));
        let response = next.run(request).await;

        // Record outcome based on response status
        let response_status = response.status();
        if response_status == StatusCode::UNAUTHORIZED {
            let failure_status = mw.lockout.record_failure(&identity).await?;
            if failure_status.delay_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(failure_status.delay_ms)).await;
            }
        } else if response_status.is_success() {
            mw.lockout.record_success(&identity).await?;
        }

        Ok(response)
    }
}
