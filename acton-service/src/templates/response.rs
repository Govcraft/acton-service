//! Template response helpers for full pages and HTMX fragments.

use askama::Template;
use axum::{
    http::{HeaderName, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
};

/// Render mode for templates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderMode {
    /// Full page render (with layout).
    #[default]
    FullPage,
    /// Fragment render (no layout, for HTMX).
    Fragment,
}

/// Wrapper for rendering templates as HTML responses.
///
/// Provides additional control over rendering, including:
/// - Fragment rendering for HTMX (out-of-band updates)
/// - Custom status codes
/// - Additional headers
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::templates::{HtmlTemplate, RenderMode};
/// use acton_service::htmx::HxRequest;
///
/// async fn users_list(HxRequest(is_htmx): HxRequest) -> impl IntoResponse {
///     let template = UsersListTemplate { users };
///
///     if is_htmx {
///         HtmlTemplate::fragment(template)
///     } else {
///         HtmlTemplate::page(template)
///     }
/// }
/// ```
pub struct HtmlTemplate<T: Template> {
    template: T,
    status: StatusCode,
    mode: RenderMode,
    headers: Vec<(HeaderName, String)>,
}

impl<T: Template> HtmlTemplate<T> {
    /// Create a new HTML template response.
    #[must_use]
    pub fn new(template: T) -> Self {
        Self {
            template,
            status: StatusCode::OK,
            mode: RenderMode::FullPage,
            headers: Vec::new(),
        }
    }

    /// Create a full page response.
    #[must_use]
    pub fn page(template: T) -> Self {
        Self::new(template)
    }

    /// Create a fragment response (for HTMX).
    ///
    /// Fragments are rendered without the layout wrapper,
    /// suitable for HTMX partial updates.
    #[must_use]
    pub fn fragment(template: T) -> Self {
        Self {
            template,
            status: StatusCode::OK,
            mode: RenderMode::Fragment,
            headers: Vec::new(),
        }
    }

    /// Set the HTTP status code.
    #[must_use]
    pub fn with_status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    /// Add a custom header.
    #[must_use]
    pub fn with_header(mut self, name: HeaderName, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }

    /// Add HX-Trigger header for HTMX events.
    #[must_use]
    pub fn with_hx_trigger(self, event: impl Into<String>) -> Self {
        self.with_header(HeaderName::from_static("hx-trigger"), event)
    }

    /// Add HX-Redirect header.
    #[must_use]
    pub fn with_hx_redirect(self, url: impl Into<String>) -> Self {
        self.with_header(HeaderName::from_static("hx-redirect"), url)
    }

    /// Add HX-Refresh header to trigger full page refresh.
    #[must_use]
    pub fn with_hx_refresh(self) -> Self {
        self.with_header(HeaderName::from_static("hx-refresh"), "true")
    }

    /// Add HX-Push-Url header.
    #[must_use]
    pub fn with_hx_push_url(self, url: impl Into<String>) -> Self {
        self.with_header(HeaderName::from_static("hx-push-url"), url)
    }

    /// Add HX-Replace-Url header.
    #[must_use]
    pub fn with_hx_replace_url(self, url: impl Into<String>) -> Self {
        self.with_header(HeaderName::from_static("hx-replace-url"), url)
    }

    /// Add HX-Retarget header.
    #[must_use]
    pub fn with_hx_retarget(self, selector: impl Into<String>) -> Self {
        self.with_header(HeaderName::from_static("hx-retarget"), selector)
    }

    /// Add HX-Reswap header.
    #[must_use]
    pub fn with_hx_reswap(self, swap: impl Into<String>) -> Self {
        self.with_header(HeaderName::from_static("hx-reswap"), swap)
    }

    /// Get the render mode.
    #[must_use]
    pub fn mode(&self) -> RenderMode {
        self.mode
    }
}

impl<T: Template> IntoResponse for HtmlTemplate<T> {
    fn into_response(self) -> Response {
        match self.template.render() {
            Ok(html) => {
                let mut response = (self.status, Html(html)).into_response();

                // Add custom headers
                for (name, value) in self.headers {
                    if let Ok(value) = HeaderValue::from_str(&value) {
                        response.headers_mut().insert(name, value);
                    }
                }

                response
            }
            Err(err) => {
                tracing::error!("Template rendering error: {}", err);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Html(format!("<!-- Template error: {} -->", err)),
                )
                    .into_response()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use askama::Template;

    #[derive(Template)]
    #[template(source = "<p>Hello, {{ name }}!</p>", ext = "html")]
    struct TestTemplate {
        name: String,
    }

    #[test]
    fn test_html_template_page() {
        let template = HtmlTemplate::page(TestTemplate {
            name: "World".to_string(),
        });
        assert_eq!(template.mode(), RenderMode::FullPage);
    }

    #[test]
    fn test_html_template_fragment() {
        let template = HtmlTemplate::fragment(TestTemplate {
            name: "World".to_string(),
        });
        assert_eq!(template.mode(), RenderMode::Fragment);
    }

    #[test]
    fn test_html_template_render() {
        let template = HtmlTemplate::new(TestTemplate {
            name: "Test".to_string(),
        });
        let response = template.into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn test_html_template_custom_status() {
        let template = HtmlTemplate::new(TestTemplate {
            name: "Test".to_string(),
        })
        .with_status(StatusCode::CREATED);
        let response = template.into_response();
        assert_eq!(response.status(), StatusCode::CREATED);
    }
}
