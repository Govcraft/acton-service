//! Askama template engine integration for server-rendered applications.
//!
//! This module provides Askama-based template rendering with seamless integration
//! into acton-service's session system, including flash messages and CSRF protection.
//!
//! # Features
//!
//! - **Compile-time templates**: Templates are validated and compiled at build time
//! - **Template context**: Automatic aggregation of flash messages, CSRF tokens, session data
//! - **HTMX support**: First-class support for fragment rendering and HTMX headers
//! - **Layout inheritance**: Full Jinja2-style template inheritance
//!
//! # Quick Start
//!
//! ```toml
//! [dependencies]
//! acton-service = { version = "0.9", features = ["askama", "session-memory"] }
//! ```
//!
//! Create templates in `templates/` directory:
//!
//! ```html
//! <!-- templates/base.html -->
//! <!DOCTYPE html>
//! <html>
//!   <head>
//!     <title>{% block title %}My App{% endblock %}</title>
//!     {{ ctx.csrf_meta()|safe }}
//!   </head>
//!   <body hx-headers='{"X-CSRF-Token": "{{ ctx.csrf_token.as_deref().unwrap_or("") }}"}'>
//!     {% if ctx.has_flash() %}
//!       {% for msg in ctx.flash_messages %}
//!         <div class="alert alert-{{ msg.kind.css_class() }}">{{ msg.message }}</div>
//!       {% endfor %}
//!     {% endif %}
//!     {% block content %}{% endblock %}
//!   </body>
//! </html>
//! ```
//!
//! ```rust,ignore
//! use acton_service::prelude::*;
//! use acton_service::templates::{TemplateContext, HtmlTemplate};
//! use askama::Template;
//!
//! #[derive(Template)]
//! #[template(path = "pages/home.html")]
//! struct HomeTemplate {
//!     ctx: TemplateContext,
//!     message: String,
//! }
//!
//! async fn home(ctx: TemplateContext) -> impl IntoResponse {
//!     HomeTemplate {
//!         ctx,
//!         message: "Welcome!".to_string(),
//!     }
//! }
//! ```
//!
//! # HTMX-Aware Handlers
//!
//! ```rust,ignore
//! use acton_service::prelude::*;
//! use acton_service::htmx::HxRequest;
//! use acton_service::templates::{TemplateContext, HtmlTemplate, RenderMode};
//!
//! async fn users(
//!     ctx: TemplateContext,
//!     HxRequest(is_htmx): HxRequest,
//! ) -> impl IntoResponse {
//!     let template = UsersTemplate { ctx, users: fetch_users().await };
//!
//!     if is_htmx {
//!         HtmlTemplate::fragment(template).with_hx_push_url("/users")
//!     } else {
//!         HtmlTemplate::page(template)
//!     }
//! }
//! ```

mod context;
mod helpers;
mod response;

pub use context::TemplateContext;
pub use helpers::{classes, pluralize, truncate};
pub use response::{HtmlTemplate, RenderMode};

// Re-export askama Template derive for convenience
pub use askama::Template;
