//! HTMX utility helpers.
//!
//! This module provides helper functions for common HTMX patterns.

use axum::{
    http::HeaderMap,
    response::{Html, IntoResponse, Response},
};

/// Check if the current request is an HTMX request based on headers.
///
/// Useful in middleware or where extractors aren't available.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::htmx::is_htmx_request;
///
/// fn check_request(headers: &HeaderMap) -> bool {
///     is_htmx_request(headers)
/// }
/// ```
#[must_use]
pub fn is_htmx_request(headers: &HeaderMap) -> bool {
    headers.contains_key("hx-request")
}

/// Check if the current request is an HTMX boosted request.
///
/// Boosted requests are made by HTMX when using `hx-boost="true"` on links
/// or forms. These requests should typically return full pages rather than
/// fragments.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::htmx::is_boosted_request;
///
/// fn check_boosted(headers: &HeaderMap) -> bool {
///     is_boosted_request(headers)
/// }
/// ```
#[must_use]
pub fn is_boosted_request(headers: &HeaderMap) -> bool {
    headers
        .get("hx-boosted")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s == "true")
}

/// Return different responses for HTMX vs full-page requests.
///
/// This is a helper for the common pattern of returning a fragment
/// for HTMX requests and a full page for regular requests.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::htmx::{HxRequest, fragment_or_full};
///
/// async fn items_list(HxRequest(is_htmx): HxRequest) -> impl IntoResponse {
///     let items_html = render_items();
///     fragment_or_full(
///         is_htmx,
///         items_html.clone(),
///         || render_full_page(&items_html),
///     )
/// }
/// ```
pub fn fragment_or_full<F>(is_htmx: bool, fragment: impl Into<String>, full_page_fn: F) -> Response
where
    F: FnOnce() -> String,
{
    if is_htmx {
        Html(fragment.into()).into_response()
    } else {
        Html(full_page_fn()).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn test_is_htmx_request() {
        let mut headers = HeaderMap::new();
        assert!(!is_htmx_request(&headers));

        headers.insert("hx-request", HeaderValue::from_static("true"));
        assert!(is_htmx_request(&headers));
    }

    #[test]
    fn test_is_boosted_request() {
        let mut headers = HeaderMap::new();
        assert!(!is_boosted_request(&headers));

        headers.insert("hx-boosted", HeaderValue::from_static("true"));
        assert!(is_boosted_request(&headers));

        headers.insert("hx-boosted", HeaderValue::from_static("false"));
        assert!(!is_boosted_request(&headers));
    }

    #[test]
    fn test_fragment_or_full() {
        // HTMX request should return fragment
        let response = fragment_or_full(true, "<p>Fragment</p>", || "<html><body><p>Full</p></body></html>".to_string());
        assert_eq!(response.status(), axum::http::StatusCode::OK);

        // Non-HTMX request should return full page
        let response = fragment_or_full(false, "<p>Fragment</p>", || "<html><body><p>Full</p></body></html>".to_string());
        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }
}
