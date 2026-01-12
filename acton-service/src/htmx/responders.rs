//! HTMX response types and helpers.
//!
//! This module provides custom response types for common HTMX patterns.

use axum::{
    http::{HeaderName, HeaderValue},
    response::{Html, IntoResponse, IntoResponseParts, Response, ResponseParts},
};

/// HTML fragment response for HTMX partial updates.
///
/// This is a convenience wrapper around `Html` that makes intent clear
/// when returning HTML fragments (as opposed to full pages).
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::htmx::{HxRequest, HtmlFragment};
///
/// async fn list_item(HxRequest(is_htmx): HxRequest) -> impl IntoResponse {
///     if is_htmx {
///         HtmlFragment("<li>New item</li>")
///     } else {
///         // Redirect to full page
///         axum::response::Redirect::to("/items").into_response()
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct HtmlFragment<T>(pub T);

impl<T: AsRef<str>> IntoResponse for HtmlFragment<T> {
    fn into_response(self) -> Response {
        Html(self.0.as_ref().to_string()).into_response()
    }
}

/// Multiple HX-Trigger events response part.
///
/// Use this to trigger multiple client-side events in a single response.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::htmx::{HtmlFragment, HxTriggerEvents};
///
/// async fn update_item() -> impl IntoResponse {
///     (
///         HxTriggerEvents::new()
///             .event("itemUpdated")
///             .event_with_data("notification", serde_json::json!({"message": "Saved!"})),
///         HtmlFragment("<div>Updated</div>"),
///     )
/// }
/// ```
#[derive(Debug, Clone, Default)]
pub struct HxTriggerEvents {
    events: Vec<HxEventEntry>,
    timing: TriggerTiming,
}

#[derive(Debug, Clone)]
enum HxEventEntry {
    Simple(String),
    WithData(String, serde_json::Value),
}

/// When to trigger the events.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TriggerTiming {
    /// Trigger immediately (HX-Trigger header)
    #[default]
    Immediate,
    /// Trigger after settle (HX-Trigger-After-Settle header)
    AfterSettle,
    /// Trigger after swap (HX-Trigger-After-Swap header)
    AfterSwap,
}

impl HxTriggerEvents {
    /// Create a new empty trigger events builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set when events should trigger.
    #[must_use]
    pub fn timing(mut self, timing: TriggerTiming) -> Self {
        self.timing = timing;
        self
    }

    /// Add a simple event (no data).
    #[must_use]
    pub fn event(mut self, name: impl Into<String>) -> Self {
        self.events.push(HxEventEntry::Simple(name.into()));
        self
    }

    /// Add an event with JSON data.
    #[must_use]
    pub fn event_with_data(mut self, name: impl Into<String>, data: serde_json::Value) -> Self {
        self.events.push(HxEventEntry::WithData(name.into(), data));
        self
    }

    fn header_name(&self) -> HeaderName {
        match self.timing {
            TriggerTiming::Immediate => HeaderName::from_static("hx-trigger"),
            TriggerTiming::AfterSettle => HeaderName::from_static("hx-trigger-after-settle"),
            TriggerTiming::AfterSwap => HeaderName::from_static("hx-trigger-after-swap"),
        }
    }
}

impl IntoResponseParts for HxTriggerEvents {
    type Error = std::convert::Infallible;

    fn into_response_parts(self, mut res: ResponseParts) -> Result<ResponseParts, Self::Error> {
        if self.events.is_empty() {
            return Ok(res);
        }

        // Get header name before consuming events
        let header_name = self.header_name();

        // Build the header value
        let all_simple = self
            .events
            .iter()
            .all(|e| matches!(e, HxEventEntry::Simple(_)));

        let header_value = if all_simple {
            // All simple events - use comma-separated format
            self.events
                .iter()
                .filter_map(|e| match e {
                    HxEventEntry::Simple(name) => Some(name.as_str()),
                    HxEventEntry::WithData(_, _) => None,
                })
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            // Has events with data - use JSON format
            let mut map = serde_json::Map::new();
            for event in self.events {
                match event {
                    HxEventEntry::Simple(name) => {
                        map.insert(name, serde_json::Value::Null);
                    }
                    HxEventEntry::WithData(name, data) => {
                        map.insert(name, data);
                    }
                }
            }
            serde_json::to_string(&serde_json::Value::Object(map)).unwrap_or_default()
        };

        if let Ok(value) = HeaderValue::from_str(&header_value) {
            res.headers_mut().insert(header_name, value);
        }

        Ok(res)
    }
}

/// Out-of-band swap response wrapper.
///
/// Wraps HTML content in an element with `hx-swap-oob="true"` for
/// out-of-band updates.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::htmx::{HtmlFragment, OutOfBandSwap};
///
/// async fn update_with_notification() -> impl IntoResponse {
///     // Main content + notification update via OOB swap
///     (
///         HtmlFragment("<div id='main'>Updated content</div>"),
///         OutOfBandSwap::new("notifications", "<span>Item saved!</span>"),
///     )
/// }
/// ```
#[derive(Debug, Clone)]
pub struct OutOfBandSwap {
    target_id: String,
    content: String,
    swap_style: String,
}

impl OutOfBandSwap {
    /// Create a new out-of-band swap (default: innerHTML).
    #[must_use]
    pub fn new(target_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            target_id: target_id.into(),
            content: content.into(),
            swap_style: "true".to_string(),
        }
    }

    /// Create an OOB swap with a specific swap style.
    ///
    /// Styles: "true", "innerHTML", "outerHTML", "beforebegin", "afterbegin",
    /// "beforeend", "afterend", "delete", "none"
    #[must_use]
    pub fn with_style(mut self, style: impl Into<String>) -> Self {
        self.swap_style = style.into();
        self
    }

    /// Create an OOB swap that replaces the entire element (outerHTML).
    #[must_use]
    pub fn replace(target_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self::new(target_id, content).with_style("outerHTML")
    }

    /// Create an OOB swap that appends content (beforeend).
    #[must_use]
    pub fn append(target_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self::new(target_id, content).with_style("beforeend")
    }

    /// Create an OOB swap that prepends content (afterbegin).
    #[must_use]
    pub fn prepend(target_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self::new(target_id, content).with_style("afterbegin")
    }

    /// Create an OOB swap that deletes the target element.
    #[must_use]
    pub fn delete(target_id: impl Into<String>) -> Self {
        Self {
            target_id: target_id.into(),
            content: String::new(),
            swap_style: "delete".to_string(),
        }
    }
}

impl IntoResponse for OutOfBandSwap {
    fn into_response(self) -> Response {
        let html = format!(
            r#"<div id="{}" hx-swap-oob="{}">{}</div>"#,
            html_escape(&self.target_id),
            html_escape(&self.swap_style),
            self.content
        );
        Html(html).into_response()
    }
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
    fn test_html_fragment() {
        let fragment = HtmlFragment("<div>Test</div>");
        let response = fragment.into_response();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    #[test]
    fn test_oob_swap() {
        let swap = OutOfBandSwap::new("test-id", "<p>Content</p>");
        let response = swap.into_response();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    #[test]
    fn test_oob_swap_delete() {
        let swap = OutOfBandSwap::delete("test-id");
        assert_eq!(swap.swap_style, "delete");
        assert!(swap.content.is_empty());
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("test"), "test");
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("\"hello\""), "&quot;hello&quot;");
        assert_eq!(html_escape("foo & bar"), "foo &amp; bar");
    }

    #[test]
    fn test_trigger_events_simple() {
        let events = HxTriggerEvents::new()
            .event("event1")
            .event("event2");
        assert_eq!(events.events.len(), 2);
    }
}
