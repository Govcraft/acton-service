//! Server-Sent Events (SSE) support for acton-service.
//!
//! This module provides one-way server-to-client real-time communication.
//! SSE is simpler than WebSocket for cases where bidirectional communication
//! isn't needed.
//!
//! # Features
//!
//! - **One-way streaming**: Efficient server-to-client event delivery
//! - **Automatic reconnection**: Browser handles reconnects with Last-Event-ID
//! - **Keep-alive**: Configurable heartbeat to prevent connection timeouts
//! - **Named events**: Support for event types with `event:` field
//! - **HTMX integration**: First-class support for HTMX SSE extension
//! - **Broadcasting**: Efficient multi-connection event delivery
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use acton_service::prelude::*;
//! use acton_service::sse::{Sse, Event, KeepAlive, SseEventExt};
//! use futures::stream::{self, Stream};
//! use std::convert::Infallible;
//! use std::time::Duration;
//!
//! async fn events_handler() -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
//!     let stream = stream::repeat_with(|| {
//!         Event::default().data("heartbeat")
//!     })
//!     .map(Ok)
//!     .throttle(Duration::from_secs(1));
//!
//!     Sse::new(stream).keep_alive(KeepAlive::default())
//! }
//! ```
//!
//! # HTMX Integration
//!
//! ```rust,ignore
//! use acton_service::sse::htmx::htmx_event;
//!
//! // In your handler
//! let event = htmx_event("notifications", "<li>New message!</li>");
//! ```
//!
//! ```html
//! <!-- In your HTML -->
//! <ul hx-ext="sse" sse-connect="/notifications" sse-swap="notifications">
//!   <!-- New items will be appended here -->
//! </ul>
//! ```
//!
//! # Broadcasting to Multiple Connections
//!
//! ```rust,ignore
//! use acton_service::sse::{SseBroadcaster, BroadcastMessage};
//! use std::sync::Arc;
//!
//! let broadcaster = Arc::new(SseBroadcaster::new());
//!
//! // In your SSE handler
//! let mut receiver = broadcaster.subscribe();
//!
//! // In your trigger endpoint
//! broadcaster.broadcast(BroadcastMessage::new("New data!"));
//! ```

mod broadcast;
mod config;
mod connection;
mod event;
pub mod htmx;

// Re-exports
pub use broadcast::{BroadcastMessage, BroadcastTarget, SseBroadcaster};
pub use config::SseConfig;
pub use connection::{ConnectionId, SseConnection};
pub use event::{SseEventExt, TypedEvent};
pub use htmx::{htmx_close_event, htmx_event, htmx_json_event, htmx_oob_event, htmx_trigger, HtmxSwap};

// Re-export axum SSE types for convenience
pub use axum::response::sse::{Event, KeepAlive, Sse};
