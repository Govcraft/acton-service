//! Session data extractors for type-safe session access.
//!
//! This module provides extractors that make working with session data ergonomic
//! and type-safe.
//!
//! # Extractors
//!
//! - [`TypedSession<T>`]: Type-safe session data with automatic serialization
//! - [`AuthSession`]: Pre-built session structure for authentication
//!
//! # Example
//!
//! ```rust,ignore
//! use acton_service::session::{TypedSession, AuthSession};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Default, Serialize, Deserialize)]
//! struct CartSession {
//!     items: Vec<String>,
//! }
//!
//! async fn add_to_cart(mut session: TypedSession<CartSession>) -> impl IntoResponse {
//!     session.data_mut().items.push("item-123".to_string());
//!     session.save().await?;
//!     Ok::<_, Error>("Added to cart")
//! }
//! ```

use async_trait::async_trait;
use axum::{extract::FromRequestParts, http::request::Parts};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tower_sessions::Session;

use crate::error::Error;

/// Type-safe session data wrapper.
///
/// Provides ergonomic access to session data with automatic serialization
/// and deserialization. The session data type must implement `Default`,
/// `Serialize`, and `Deserialize`.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Default, Serialize, Deserialize)]
/// struct UserPrefs {
///     theme: String,
///     language: String,
/// }
///
/// async fn get_prefs(session: TypedSession<UserPrefs>) -> impl IntoResponse {
///     let theme = &session.data().theme;
///     Json(session.into_data())
/// }
///
/// async fn set_theme(mut session: TypedSession<UserPrefs>) -> impl IntoResponse {
///     session.data_mut().theme = "dark".to_string();
///     session.save().await?;
///     Ok::<_, Error>("Theme updated")
/// }
/// ```
pub struct TypedSession<T> {
    session: Session,
    data: T,
}

impl<T> TypedSession<T>
where
    T: Default + DeserializeOwned + Serialize + Send + Sync,
{
    const DATA_KEY: &'static str = "_typed_session_data";

    /// Get a reference to the session data.
    #[must_use]
    pub fn data(&self) -> &T {
        &self.data
    }

    /// Get a mutable reference to the session data.
    ///
    /// After modifying, call [`save`](Self::save) to persist changes.
    pub fn data_mut(&mut self) -> &mut T {
        &mut self.data
    }

    /// Take ownership of the session data.
    #[must_use]
    pub fn into_data(self) -> T {
        self.data
    }

    /// Get a reference to the underlying session.
    #[must_use]
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// Save the session data.
    ///
    /// Call this after modifying data via [`data_mut`](Self::data_mut).
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be written.
    pub async fn save(&self) -> Result<(), Error> {
        self.session
            .insert(Self::DATA_KEY, &self.data)
            .await
            .map_err(|e| Error::Session(format!("Failed to save session data: {e}")))
    }

    /// Update session data with a closure and save.
    ///
    /// This is a convenience method that applies a function to the data
    /// and saves in one step.
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be written.
    pub async fn update<F>(&mut self, f: F) -> Result<(), Error>
    where
        F: FnOnce(&mut T),
    {
        f(&mut self.data);
        self.save().await
    }

    /// Clear the session data (reset to default).
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be written.
    pub async fn clear(&mut self) -> Result<(), Error> {
        self.data = T::default();
        self.session
            .remove::<T>(Self::DATA_KEY)
            .await
            .map_err(|e| Error::Session(format!("Failed to clear session data: {e}")))?;
        Ok(())
    }

    /// Destroy the entire session (invalidate session ID).
    ///
    /// This removes all session data and invalidates the session cookie.
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be flushed.
    pub async fn destroy(&self) -> Result<(), Error> {
        self.session
            .flush()
            .await
            .map_err(|e| Error::Session(format!("Failed to destroy session: {e}")))
    }

    /// Regenerate the session ID.
    ///
    /// Call this after login to prevent session fixation attacks.
    /// The session data is preserved with a new session ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the session ID cannot be regenerated.
    pub async fn regenerate(&self) -> Result<(), Error> {
        self.session
            .cycle_id()
            .await
            .map_err(|e| Error::Session(format!("Failed to regenerate session ID: {e}")))
    }
}

impl<S, T> FromRequestParts<S> for TypedSession<T>
where
    S: Send + Sync,
    T: Default + DeserializeOwned + Serialize + Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // Get session from request extensions (set by SessionManagerLayer)
        let session = parts
            .extensions
            .get::<Session>()
            .cloned()
            .ok_or_else(|| Error::Session("Session not found in request extensions. Is SessionManagerLayer configured?".to_string()))?;

        let data: T = session
            .get(Self::DATA_KEY)
            .await
            .map_err(|e| Error::Session(format!("Failed to read session data: {e}")))?
            .unwrap_or_default();

        Ok(Self { session, data })
    }
}

/// Extension trait for working with session data.
///
/// Provides convenience methods for getting and setting arbitrary session values.
#[async_trait]
pub trait SessionData {
    /// Get a value from the session.
    async fn get_value<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, Error>;

    /// Set a value in the session.
    async fn set_value<T: Serialize + Send + Sync>(&self, key: &str, value: &T)
        -> Result<(), Error>;

    /// Remove a value from the session.
    async fn remove_value<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, Error>;

    /// Check if a key exists in the session.
    async fn has_key(&self, key: &str) -> Result<bool, Error>;
}

#[async_trait]
impl SessionData for Session {
    async fn get_value<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, Error> {
        self.get(key)
            .await
            .map_err(|e| Error::Session(format!("Session get error: {e}")))
    }

    async fn set_value<T: Serialize + Send + Sync>(
        &self,
        key: &str,
        value: &T,
    ) -> Result<(), Error> {
        self.insert(key, value)
            .await
            .map_err(|e| Error::Session(format!("Session set error: {e}")))
    }

    async fn remove_value<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, Error> {
        self.remove(key)
            .await
            .map_err(|e| Error::Session(format!("Session remove error: {e}")))
    }

    async fn has_key(&self, key: &str) -> Result<bool, Error> {
        let value: Option<serde_json::Value> = self
            .get(key)
            .await
            .map_err(|e| Error::Session(format!("Session check error: {e}")))?;
        Ok(value.is_some())
    }
}

/// Pre-built session data structure for authentication.
///
/// Use this for session-based authentication in HTMX/server-rendered applications.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::session::{TypedSession, AuthSession};
///
/// async fn login(mut auth: TypedSession<AuthSession>) -> impl IntoResponse {
///     // After validating credentials...
///     auth.data_mut().login("user-123".to_string(), vec!["admin".to_string()]);
///     auth.save().await?;
///     auth.regenerate().await?; // Prevent session fixation
///     Redirect::to("/dashboard")
/// }
///
/// async fn dashboard(auth: TypedSession<AuthSession>) -> impl IntoResponse {
///     if !auth.data().is_authenticated() {
///         return Redirect::to("/login").into_response();
///     }
///     Html("Welcome!").into_response()
/// }
///
/// async fn logout(mut auth: TypedSession<AuthSession>) -> impl IntoResponse {
///     auth.data_mut().logout();
///     auth.save().await?;
///     Redirect::to("/")
/// }
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthSession {
    /// Authenticated user ID (None if not logged in).
    pub user_id: Option<String>,
    /// User roles for authorization.
    pub roles: Vec<String>,
    /// Timestamp when the user authenticated (Unix timestamp).
    pub authenticated_at: Option<i64>,
    /// Additional user data (username, email, etc.).
    #[serde(default)]
    pub extra: std::collections::HashMap<String, String>,
}

impl AuthSession {
    /// Check if the user is authenticated.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.user_id.is_some()
    }

    /// Get the user ID if authenticated.
    #[must_use]
    pub fn user_id(&self) -> Option<&str> {
        self.user_id.as_deref()
    }

    /// Login - set user ID and regenerate session.
    ///
    /// Sets the user ID, roles, and authentication timestamp.
    pub fn login(&mut self, user_id: String, roles: Vec<String>) {
        self.user_id = Some(user_id);
        self.roles = roles;
        self.authenticated_at = Some(chrono::Utc::now().timestamp());
    }

    /// Login with extra data.
    pub fn login_with_extra(
        &mut self,
        user_id: String,
        roles: Vec<String>,
        extra: std::collections::HashMap<String, String>,
    ) {
        self.login(user_id, roles);
        self.extra = extra;
    }

    /// Logout - clear authentication data.
    pub fn logout(&mut self) {
        self.user_id = None;
        self.roles.clear();
        self.authenticated_at = None;
        self.extra.clear();
    }

    /// Check if the user has a specific role.
    #[must_use]
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }

    /// Check if the user has any of the specified roles.
    #[must_use]
    pub fn has_any_role(&self, roles: &[&str]) -> bool {
        roles.iter().any(|r| self.has_role(r))
    }

    /// Check if the user has all of the specified roles.
    #[must_use]
    pub fn has_all_roles(&self, roles: &[&str]) -> bool {
        roles.iter().all(|r| self.has_role(r))
    }

    /// Get extra data by key.
    #[must_use]
    pub fn get_extra(&self, key: &str) -> Option<&str> {
        self.extra.get(key).map(String::as_str)
    }

    /// Set extra data.
    pub fn set_extra(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.extra.insert(key.into(), value.into());
    }

    /// Get authentication duration since login.
    #[must_use]
    pub fn session_age(&self) -> Option<chrono::Duration> {
        self.authenticated_at.map(|ts| {
            let now = chrono::Utc::now().timestamp();
            chrono::Duration::seconds(now - ts)
        })
    }
}

/// Type alias for authentication-focused typed session.
pub type SessionAuth = TypedSession<AuthSession>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_session_login_logout() {
        let mut auth = AuthSession::default();
        assert!(!auth.is_authenticated());

        auth.login("user-123".to_string(), vec!["admin".to_string()]);
        assert!(auth.is_authenticated());
        assert_eq!(auth.user_id(), Some("user-123"));
        assert!(auth.has_role("admin"));
        assert!(!auth.has_role("superuser"));

        auth.logout();
        assert!(!auth.is_authenticated());
        assert!(auth.roles.is_empty());
    }

    #[test]
    fn test_auth_session_roles() {
        let mut auth = AuthSession::default();
        auth.login(
            "user".to_string(),
            vec!["admin".to_string(), "editor".to_string()],
        );

        assert!(auth.has_any_role(&["admin", "viewer"]));
        assert!(auth.has_all_roles(&["admin", "editor"]));
        assert!(!auth.has_all_roles(&["admin", "superuser"]));
    }

    #[test]
    fn test_auth_session_extra_data() {
        let mut auth = AuthSession::default();
        auth.set_extra("email", "user@example.com");
        assert_eq!(auth.get_extra("email"), Some("user@example.com"));
        assert_eq!(auth.get_extra("missing"), None);
    }
}
