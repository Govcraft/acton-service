//! HTMX support for server-rendered applications.
//!
//! This module provides HTMX integration for acton-service, enabling
//! ergonomic development of HTMX-based applications.
//!
//! # Features
//!
//! - **Header Extractors**: Type-safe access to HTMX request headers
//! - **Response Builders**: Convenient response types for HTMX patterns
//! - **Auto-Vary Middleware**: Automatic `Vary` header management for caching
//! - **CSRF Integration**: Works seamlessly with existing CSRF protection
//! - **Fragment Responses**: Helpers for partial HTML responses
//!
//! # Quick Start
//!
//! ```toml
//! [dependencies]
//! acton-service = { version = "0.9", features = ["htmx", "session-memory"] }
//! ```
//!
//! ```rust,ignore
//! use acton_service::prelude::*;
//! use acton_service::htmx::{HxRequest, HtmlFragment};
//!
//! async fn handler(HxRequest(is_htmx): HxRequest) -> impl IntoResponse {
//!     if is_htmx {
//!         // Return just the fragment for HTMX
//!         HtmlFragment("<div>Updated content</div>")
//!     } else {
//!         // Return full page for regular requests
//!         Html(include_str!("templates/full_page.html")).into_response()
//!     }
//! }
//! ```
//!
//! # CSRF Integration
//!
//! The session module's CSRF support works seamlessly with HTMX:
//!
//! ```html
//! <head>
//!     {{ csrf.as_meta_tag() }}
//! </head>
//! <body hx-headers='{"X-CSRF-Token": "{{ csrf.token() }}"}'>
//!     <!-- All HTMX requests will include the CSRF token -->
//! </body>
//! ```

mod helpers;
mod responders;

// Custom response types and helpers
pub use helpers::{fragment_or_full, is_boosted_request, is_htmx_request};
pub use responders::{HtmlFragment, HxTriggerEvents, OutOfBandSwap, TriggerTiming};

// Re-export extractors from axum-htmx
pub use axum_htmx::{
    HxBoosted, HxCurrentUrl, HxHistoryRestoreRequest, HxPrompt, HxRequest, HxTarget, HxTrigger,
    HxTriggerName,
};

// Re-export response headers from axum-htmx
pub use axum_htmx::{
    HxLocation, HxPushUrl, HxRedirect, HxRefresh, HxReplaceUrl, HxReselect, HxResponseTrigger,
    HxReswap, HxRetarget, SwapOption,
};

// Re-export Vary responders from axum-htmx
pub use axum_htmx::{VaryHxRequest, VaryHxTarget, VaryHxTrigger, VaryHxTriggerName};

// Re-export auto-vary middleware from axum-htmx
pub use axum_htmx::{AutoVaryLayer, AutoVaryMiddleware};

// Re-export event types
pub use axum_htmx::HxEvent;
