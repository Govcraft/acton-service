//! OAuth/OIDC provider integration (requires `oauth` feature)
//!
//! Provides integration with OAuth providers like Google, GitHub, and
//! custom OIDC-compliant providers.
//!
//! # Example
//!
//! ```rust,ignore
//! use acton_service::auth::oauth::{GoogleProvider, OAuthProvider};
//! use acton_service::auth::config::OAuthProviderConfig;
//!
//! let config = OAuthProviderConfig {
//!     client_id: "your-client-id".to_string(),
//!     client_secret: "your-secret".to_string(),
//!     redirect_uri: "https://example.com/callback".to_string(),
//!     scopes: vec!["openid".to_string(), "email".to_string()],
//! };
//!
//! let provider = GoogleProvider::new(&config)?;
//!
//! // Generate authorization URL
//! let auth_url = provider.authorization_url("state-value", &[]);
//!
//! // After callback, exchange code for tokens
//! let tokens = provider.exchange_code("authorization-code").await?;
//!
//! // Get user info
//! let user_info = provider.get_user_info(&tokens.access_token).await?;
//! ```

pub mod provider;
pub mod providers;
pub mod state;

// Core trait and types
pub use provider::{OAuthProvider, OAuthTokens, OAuthUserInfo};

// Provider implementations
pub use providers::{
    google::GoogleProvider, github::GitHubProvider,
    custom::{CustomOidcProvider, CustomOidcConfig},
};

// State management
pub use state::{OAuthStateManager, StateData, generate_state};

#[cfg(feature = "cache")]
pub use state::RedisOAuthStateManager;
