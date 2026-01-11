//! Flash message support for HTMX and server-rendered applications.
//!
//! Flash messages are one-time messages stored in the session that are automatically
//! consumed when read. They're commonly used for displaying success/error messages
//! after form submissions (post-redirect-get pattern).
//!
//! # Example
//!
//! ```rust,ignore
//! use acton_service::session::{FlashMessage, FlashMessages};
//! use tower_sessions::Session;
//!
//! // In form handler - add flash message
//! async fn create_user(session: Session, Form(data): Form<CreateUser>) -> impl IntoResponse {
//!     // ... create user ...
//!     FlashMessages::push(&session, FlashMessage::success("User created!")).await?;
//!     Redirect::to("/users")
//! }
//!
//! // In page handler - read and consume flash messages
//! async fn list_users(flash: FlashMessages) -> impl IntoResponse {
//!     let messages = flash.messages(); // Auto-cleared from session
//!     Html(render_users_page(messages))
//! }
//! ```

use axum::{extract::FromRequestParts, http::request::Parts};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

use crate::error::Error;

const FLASH_SESSION_KEY: &str = "_flash_messages";

/// Flash message severity/type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FlashKind {
    /// Success message (e.g., "Item saved successfully")
    Success,
    /// Informational message (e.g., "Your session will expire soon")
    Info,
    /// Warning message (e.g., "This action cannot be undone")
    Warning,
    /// Error message (e.g., "Failed to save item")
    Error,
}

impl FlashKind {
    /// Returns the CSS class name for this flash kind.
    ///
    /// Useful for styling flash messages in templates.
    #[must_use]
    pub fn css_class(&self) -> &'static str {
        match self {
            Self::Success => "flash-success",
            Self::Info => "flash-info",
            Self::Warning => "flash-warning",
            Self::Error => "flash-error",
        }
    }

    /// Returns an icon name for this flash kind.
    ///
    /// Useful for displaying icons in flash messages.
    #[must_use]
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Success => "check-circle",
            Self::Info => "info-circle",
            Self::Warning => "exclamation-triangle",
            Self::Error => "x-circle",
        }
    }
}

/// A single flash message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashMessage {
    /// The type/severity of the message.
    pub kind: FlashKind,
    /// The message content.
    pub message: String,
}

impl FlashMessage {
    /// Create a new flash message.
    #[must_use]
    pub fn new(kind: FlashKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// Create a success flash message.
    #[must_use]
    pub fn success(message: impl Into<String>) -> Self {
        Self::new(FlashKind::Success, message)
    }

    /// Create an info flash message.
    #[must_use]
    pub fn info(message: impl Into<String>) -> Self {
        Self::new(FlashKind::Info, message)
    }

    /// Create a warning flash message.
    #[must_use]
    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(FlashKind::Warning, message)
    }

    /// Create an error flash message.
    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(FlashKind::Error, message)
    }
}

/// Flash messages extractor.
///
/// When extracted, flash messages are automatically removed from the session.
/// This implements the "consume on read" pattern typical for flash messages.
///
/// # Example
///
/// ```rust,ignore
/// async fn handler(flash: FlashMessages) -> impl IntoResponse {
///     if !flash.is_empty() {
///         for msg in flash.messages() {
///             println!("{}: {}", msg.kind.css_class(), msg.message);
///         }
///     }
///     Html("...")
/// }
/// ```
pub struct FlashMessages {
    messages: Vec<FlashMessage>,
}

impl FlashMessages {
    /// Get all flash messages.
    ///
    /// These messages have already been consumed from the session.
    #[must_use]
    pub fn messages(&self) -> &[FlashMessage] {
        &self.messages
    }

    /// Take ownership of all flash messages.
    #[must_use]
    pub fn into_messages(self) -> Vec<FlashMessage> {
        self.messages
    }

    /// Check if there are any flash messages.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Get the number of flash messages.
    #[must_use]
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Get messages of a specific kind.
    #[must_use]
    pub fn by_kind(&self, kind: FlashKind) -> Vec<&FlashMessage> {
        self.messages.iter().filter(|m| m.kind == kind).collect()
    }

    /// Check if there are any success messages.
    #[must_use]
    pub fn has_success(&self) -> bool {
        self.messages.iter().any(|m| m.kind == FlashKind::Success)
    }

    /// Check if there are any error messages.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.messages.iter().any(|m| m.kind == FlashKind::Error)
    }

    /// Push a flash message to the session.
    ///
    /// Use this in handlers to add flash messages before redirecting.
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be accessed.
    pub async fn push(session: &Session, message: FlashMessage) -> Result<(), Error> {
        let mut messages: Vec<FlashMessage> = session
            .get(FLASH_SESSION_KEY)
            .await
            .map_err(|e| Error::Session(format!("Failed to read flash messages: {e}")))?
            .unwrap_or_default();

        messages.push(message);

        session
            .insert(FLASH_SESSION_KEY, &messages)
            .await
            .map_err(|e| Error::Session(format!("Failed to write flash messages: {e}")))
    }

    /// Push multiple flash messages to the session.
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be accessed.
    pub async fn push_many(
        session: &Session,
        new_messages: impl IntoIterator<Item = FlashMessage>,
    ) -> Result<(), Error> {
        let mut messages: Vec<FlashMessage> = session
            .get(FLASH_SESSION_KEY)
            .await
            .map_err(|e| Error::Session(format!("Failed to read flash messages: {e}")))?
            .unwrap_or_default();

        messages.extend(new_messages);

        session
            .insert(FLASH_SESSION_KEY, &messages)
            .await
            .map_err(|e| Error::Session(format!("Failed to write flash messages: {e}")))
    }

    /// Clear all flash messages from the session without reading them.
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be accessed.
    pub async fn clear(session: &Session) -> Result<(), Error> {
        session
            .remove::<Vec<FlashMessage>>(FLASH_SESSION_KEY)
            .await
            .map_err(|e| Error::Session(format!("Failed to clear flash messages: {e}")))?;
        Ok(())
    }
}

impl<S> FromRequestParts<S> for FlashMessages
where
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // Get session from request extensions (set by SessionManagerLayer)
        let session = parts
            .extensions
            .get::<Session>()
            .cloned()
            .ok_or_else(|| Error::Session("Session not found in request extensions for flash messages".to_string()))?;

        // Read and remove flash messages (consume pattern)
        let messages: Vec<FlashMessage> = session
            .remove(FLASH_SESSION_KEY)
            .await
            .map_err(|e| Error::Session(format!("Failed to read flash messages: {e}")))?
            .unwrap_or_default();

        Ok(Self { messages })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flash_message_constructors() {
        let success = FlashMessage::success("Done!");
        assert_eq!(success.kind, FlashKind::Success);
        assert_eq!(success.message, "Done!");

        let error = FlashMessage::error("Failed");
        assert_eq!(error.kind, FlashKind::Error);
        assert_eq!(error.message, "Failed");
    }

    #[test]
    fn test_flash_kind_css_class() {
        assert_eq!(FlashKind::Success.css_class(), "flash-success");
        assert_eq!(FlashKind::Error.css_class(), "flash-error");
        assert_eq!(FlashKind::Warning.css_class(), "flash-warning");
        assert_eq!(FlashKind::Info.css_class(), "flash-info");
    }

    #[test]
    fn test_flash_messages_filtering() {
        let messages = FlashMessages {
            messages: vec![
                FlashMessage::success("OK"),
                FlashMessage::error("Bad"),
                FlashMessage::success("Also OK"),
            ],
        };

        assert_eq!(messages.len(), 3);
        assert!(!messages.is_empty());
        assert!(messages.has_success());
        assert!(messages.has_errors());

        let successes = messages.by_kind(FlashKind::Success);
        assert_eq!(successes.len(), 2);
    }
}
