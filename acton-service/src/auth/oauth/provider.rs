//! OAuth provider trait and types

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::Error;

/// OAuth tokens received from a provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokens {
    /// Access token from the provider
    pub access_token: String,

    /// Refresh token (if provided)
    pub refresh_token: Option<String>,

    /// Token lifetime in seconds (if provided)
    pub expires_in: Option<i64>,

    /// Token type (usually "Bearer")
    pub token_type: String,

    /// ID token for OIDC providers
    pub id_token: Option<String>,
}

/// Normalized user info from OAuth providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthUserInfo {
    /// Provider name (e.g., "google", "github")
    pub provider: String,

    /// User ID from the provider
    pub provider_user_id: String,

    /// User's email address
    pub email: Option<String>,

    /// Whether the email is verified
    pub email_verified: bool,

    /// User's display name
    pub name: Option<String>,

    /// User's profile picture URL
    pub picture: Option<String>,

    /// Raw provider-specific data
    pub raw: serde_json::Value,
}

/// OAuth provider trait
///
/// Implementations provide integration with specific OAuth providers.
#[async_trait]
pub trait OAuthProvider: Send + Sync {
    /// Get the provider name (e.g., "google", "github")
    fn name(&self) -> &str;

    /// Generate the authorization URL for redirecting users
    ///
    /// # Arguments
    ///
    /// * `state` - CSRF protection state value
    /// * `scopes` - Additional scopes to request
    fn authorization_url(&self, state: &str, scopes: &[String]) -> String;

    /// Exchange an authorization code for tokens
    ///
    /// # Arguments
    ///
    /// * `code` - Authorization code from the callback
    async fn exchange_code(&self, code: &str) -> Result<OAuthTokens, Error>;

    /// Get user information using an access token
    ///
    /// # Arguments
    ///
    /// * `access_token` - Access token from the provider
    async fn get_user_info(&self, access_token: &str) -> Result<OAuthUserInfo, Error>;

    /// Refresh an access token (if supported)
    ///
    /// # Arguments
    ///
    /// * `refresh_token` - Refresh token from the provider
    async fn refresh_token(&self, refresh_token: &str) -> Result<OAuthTokens, Error>;
}
