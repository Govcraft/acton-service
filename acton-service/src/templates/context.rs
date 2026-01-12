//! Template context for passing common data to templates.
//!
//! Provides a unified way to pass flash messages, CSRF tokens, session data,
//! and custom application data to Askama templates.

use std::collections::HashMap;

#[cfg(feature = "session")]
use crate::session::{FlashKind, FlashMessage};

/// Common context data available to all templates.
///
/// This struct aggregates commonly needed data for server-rendered pages:
/// - Flash messages (success/error/info notifications)
/// - CSRF token for form protection
/// - Current request path for navigation highlighting
/// - Optional authenticated user info
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::templates::{TemplateContext, HtmlTemplate};
/// use askama::Template;
///
/// #[derive(Template)]
/// #[template(path = "pages/dashboard.html")]
/// struct DashboardTemplate {
///     ctx: TemplateContext,
///     user_name: String,
///     stats: DashboardStats,
/// }
///
/// async fn dashboard(
///     ctx: TemplateContext,
///     session: TypedSession<AuthSession>,
/// ) -> impl IntoResponse {
///     let template = DashboardTemplate {
///         ctx,
///         user_name: session.data().user_id().unwrap_or("Guest").to_string(),
///         stats: fetch_stats().await,
///     };
///     template.into_response()
/// }
/// ```
#[derive(Debug, Clone, Default)]
pub struct TemplateContext {
    /// Flash messages from the session (consumed on read).
    #[cfg(feature = "session")]
    pub flash_messages: Vec<FlashMessage>,

    /// CSRF token for form protection.
    pub csrf_token: Option<String>,

    /// Current request path (for navigation highlighting).
    pub current_path: String,

    /// Whether the current user is authenticated.
    pub is_authenticated: bool,

    /// Current user ID if authenticated.
    pub user_id: Option<String>,

    /// Additional metadata (key-value pairs for custom data).
    pub meta: HashMap<String, String>,
}

impl TemplateContext {
    /// Create a new empty template context.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the current request path.
    #[must_use]
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.current_path = path.into();
        self
    }

    /// Set authentication status.
    #[must_use]
    pub fn with_auth(mut self, user_id: Option<String>) -> Self {
        self.is_authenticated = user_id.is_some();
        self.user_id = user_id;
        self
    }

    /// Add metadata key-value pair.
    #[must_use]
    pub fn with_meta(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.meta.insert(key.into(), value.into());
        self
    }

    /// Set CSRF token.
    #[must_use]
    pub fn with_csrf(mut self, token: impl Into<String>) -> Self {
        self.csrf_token = Some(token.into());
        self
    }

    /// Set flash messages.
    #[cfg(feature = "session")]
    #[must_use]
    pub fn with_flash(mut self, messages: Vec<FlashMessage>) -> Self {
        self.flash_messages = messages;
        self
    }

    /// Check if there are any flash messages.
    #[cfg(feature = "session")]
    #[must_use]
    pub fn has_flash(&self) -> bool {
        !self.flash_messages.is_empty()
    }

    /// Get success flash messages.
    #[cfg(feature = "session")]
    #[must_use]
    pub fn success_messages(&self) -> Vec<&FlashMessage> {
        self.flash_messages
            .iter()
            .filter(|m| m.kind == FlashKind::Success)
            .collect()
    }

    /// Get error flash messages.
    #[cfg(feature = "session")]
    #[must_use]
    pub fn error_messages(&self) -> Vec<&FlashMessage> {
        self.flash_messages
            .iter()
            .filter(|m| m.kind == FlashKind::Error)
            .collect()
    }

    /// Get warning flash messages.
    #[cfg(feature = "session")]
    #[must_use]
    pub fn warning_messages(&self) -> Vec<&FlashMessage> {
        self.flash_messages
            .iter()
            .filter(|m| m.kind == FlashKind::Warning)
            .collect()
    }

    /// Get info flash messages.
    #[cfg(feature = "session")]
    #[must_use]
    pub fn info_messages(&self) -> Vec<&FlashMessage> {
        self.flash_messages
            .iter()
            .filter(|m| m.kind == FlashKind::Info)
            .collect()
    }

    /// Generate hidden CSRF input field HTML.
    ///
    /// Returns HTML like: `<input type="hidden" name="_csrf" value="...">`
    #[must_use]
    pub fn csrf_field(&self) -> String {
        match &self.csrf_token {
            Some(token) => format!(
                r#"<input type="hidden" name="_csrf" value="{}">"#,
                html_escape(token)
            ),
            None => String::new(),
        }
    }

    /// Generate CSRF meta tag HTML.
    ///
    /// Returns HTML like: `<meta name="csrf-token" content="...">`
    #[must_use]
    pub fn csrf_meta(&self) -> String {
        match &self.csrf_token {
            Some(token) => format!(r#"<meta name="csrf-token" content="{}">"#, html_escape(token)),
            None => String::new(),
        }
    }

    /// Check if the given path matches the current path.
    ///
    /// Useful for highlighting active navigation items.
    #[must_use]
    pub fn is_active(&self, path: &str) -> bool {
        self.current_path == path
    }

    /// Check if the current path starts with the given prefix.
    ///
    /// Useful for highlighting parent navigation items.
    #[must_use]
    pub fn is_active_prefix(&self, prefix: &str) -> bool {
        self.current_path.starts_with(prefix)
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
    fn test_template_context_builder() {
        let ctx = TemplateContext::new()
            .with_path("/dashboard")
            .with_auth(Some("user123".to_string()))
            .with_meta("theme", "dark")
            .with_csrf("token123");

        assert_eq!(ctx.current_path, "/dashboard");
        assert!(ctx.is_authenticated);
        assert_eq!(ctx.user_id, Some("user123".to_string()));
        assert_eq!(ctx.meta.get("theme"), Some(&"dark".to_string()));
        assert_eq!(ctx.csrf_token, Some("token123".to_string()));
    }

    #[test]
    fn test_csrf_field() {
        let ctx = TemplateContext::new().with_csrf("test-token");
        let field = ctx.csrf_field();
        assert!(field.contains("test-token"));
        assert!(field.contains("name=\"_csrf\""));
    }

    #[test]
    fn test_csrf_meta() {
        let ctx = TemplateContext::new().with_csrf("test-token");
        let meta = ctx.csrf_meta();
        assert!(meta.contains("test-token"));
        assert!(meta.contains("csrf-token"));
    }

    #[test]
    fn test_is_active() {
        let ctx = TemplateContext::new().with_path("/users/123");
        assert!(!ctx.is_active("/users"));
        assert!(ctx.is_active("/users/123"));
        assert!(ctx.is_active_prefix("/users"));
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("test"), "test");
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("\"hello\""), "&quot;hello&quot;");
    }
}
