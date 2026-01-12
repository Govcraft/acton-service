//! HTMX SSE extension helpers.
//!
//! HTMX's SSE extension expects events in specific formats for
//! DOM manipulation. This module provides helpers for common patterns.
//!
//! # Example HTML
//!
//! ```html
//! <!-- Connect to SSE endpoint and swap content into element -->
//! <div hx-ext="sse" sse-connect="/events" sse-swap="message">
//!   Waiting for messages...
//! </div>
//!
//! <!-- Multiple event types -->
//! <div hx-ext="sse" sse-connect="/events">
//!   <div sse-swap="notifications">No notifications</div>
//!   <div sse-swap="alerts">No alerts</div>
//! </div>
//! ```
//!
//! # Example Rust
//!
//! ```rust,ignore
//! use acton_service::sse::htmx::{htmx_event, htmx_close_event};
//!
//! // Send an event that will swap into elements with sse-swap="message"
//! let event = htmx_event("message", "<p>Hello, World!</p>");
//!
//! // Signal HTMX to close the connection
//! let close_event = htmx_close_event();
//! ```

use axum::response::sse::Event;
use serde::Serialize;

/// HTMX SSE swap targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HtmxSwap {
    /// Replace inner HTML (default).
    InnerHtml,
    /// Replace outer HTML (entire element).
    OuterHtml,
    /// Insert before element.
    BeforeBegin,
    /// Insert as first child.
    AfterBegin,
    /// Insert as last child.
    BeforeEnd,
    /// Insert after element.
    AfterEnd,
    /// Delete the element.
    Delete,
    /// Do nothing.
    None,
}

impl HtmxSwap {
    /// Get the HTMX swap attribute value.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InnerHtml => "innerHTML",
            Self::OuterHtml => "outerHTML",
            Self::BeforeBegin => "beforebegin",
            Self::AfterBegin => "afterbegin",
            Self::BeforeEnd => "beforeend",
            Self::AfterEnd => "afterend",
            Self::Delete => "delete",
            Self::None => "none",
        }
    }
}

/// Create an HTMX-compatible SSE event.
///
/// The event name should match the `sse-swap` attribute in your HTML.
///
/// # Example HTML
///
/// ```html
/// <div hx-ext="sse" sse-connect="/events" sse-swap="message">
///   <!-- Content will be replaced when 'message' event arrives -->
/// </div>
/// ```
///
/// # Example Rust
///
/// ```rust,ignore
/// use acton_service::sse::htmx::htmx_event;
///
/// let event = htmx_event("message", "<p>New content!</p>");
/// ```
pub fn htmx_event(event_name: &str, html: impl Into<String>) -> Event {
    Event::default().event(event_name).data(html.into())
}

/// Create an HTMX event with JSON data.
///
/// Useful when your HTMX handler processes JSON on the client side.
///
/// # Errors
///
/// Returns an error if JSON serialization fails.
pub fn htmx_json_event<T: Serialize>(
    event_name: &str,
    data: &T,
) -> Result<Event, serde_json::Error> {
    let json = serde_json::to_string(data)?;
    Ok(Event::default().event(event_name).data(json))
}

/// Create a close event to signal HTMX to close the SSE connection.
///
/// HTMX will close the connection when it receives an event named "htmx:closeSSE".
pub fn htmx_close_event() -> Event {
    Event::default().event("htmx:closeSSE").data("")
}

/// Trigger an HTMX event on the client.
///
/// This sends an event that can be caught by HTMX's event system.
/// The client should handle this with `hx-trigger="sse:eventName"`.
pub fn htmx_trigger(trigger_name: &str) -> Event {
    Event::default().event("htmx:trigger").data(trigger_name)
}

/// Create an out-of-band swap event.
///
/// This sends HTML that contains `hx-swap-oob="true"` to update
/// elements outside the normal swap target.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::sse::htmx::htmx_oob_event;
///
/// // This will update the element with id="notification-count"
/// let event = htmx_oob_event("update", r#"<span id="notification-count" hx-swap-oob="true">5</span>"#);
/// ```
pub fn htmx_oob_event(event_name: &str, html: impl Into<String>) -> Event {
    Event::default().event(event_name).data(html.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_htmx_swap_as_str() {
        assert_eq!(HtmxSwap::InnerHtml.as_str(), "innerHTML");
        assert_eq!(HtmxSwap::OuterHtml.as_str(), "outerHTML");
        assert_eq!(HtmxSwap::BeforeBegin.as_str(), "beforebegin");
        assert_eq!(HtmxSwap::Delete.as_str(), "delete");
    }

    #[test]
    fn test_htmx_event() {
        let _event = htmx_event("message", "<p>Hello</p>");
        // Event is opaque, but we can verify it doesn't panic
    }

    #[test]
    fn test_htmx_close_event() {
        let _event = htmx_close_event();
        // Event is opaque, but we can verify it doesn't panic
    }

    #[test]
    fn test_htmx_trigger() {
        let _event = htmx_trigger("refreshList");
        // Event is opaque, but we can verify it doesn't panic
    }

    #[derive(Serialize)]
    struct TestData {
        count: i32,
    }

    #[test]
    fn test_htmx_json_event() {
        let result = htmx_json_event("data", &TestData { count: 42 });
        assert!(result.is_ok());
    }
}
