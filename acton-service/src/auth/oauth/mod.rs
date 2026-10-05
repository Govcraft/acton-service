//! OAuth/OIDC provider integration (requires `oauth` feature)
//!
//! Provides integration with OAuth providers like Google, GitHub, and
//! custom OIDC-compliant providers.
//!
//! # Example
//!
//! ```rust,no_run
//! use acton_service::auth::config::{OAuthConfig, OAuthProviderConfig};
//! use acton_service::auth::oauth::{
//!     MemoryOAuthStateManager, OAuthProviderRegistry, OAuthStateManager, StateData,
//! };
//! # async fn example() -> Result<(), acton_service::error::Error> {
//! let config = OAuthConfig {
//!     enabled: true,
//!     state_ttl_secs: 600,
//!     providers: [("google".to_string(), OAuthProviderConfig {
//!         client_id: "your-client-id".to_string(),
//!         client_secret: "your-secret".to_string(),
//!         redirect_uri: "https://example.com/callback".to_string(),
//!         scopes: vec![],
//!         authorization_endpoint: None,
//!         token_endpoint: None,
//!         userinfo_endpoint: None,
//!     })].into(),
//! };
//! let providers = OAuthProviderRegistry::from_config(&config)?;
//! let states = MemoryOAuthStateManager::new(config.state_ttl_secs);
//! let provider = providers.get("google").ok_or_else(|| {
//!     acton_service::error::Error::NotFound("Unknown OAuth provider".to_string())
//! })?;
//!
//! let state = states.create_state(&StateData {
//!     provider: "google".to_string(),
//!     redirect_uri: Some("/dashboard".to_string()),
//!     created_at: chrono::Utc::now().timestamp(),
//!     extra: None,
//! }).await?;
//! let auth_url = provider.authorization_url(&state, &[]);
//! // Redirect the browser to auth_url. Keep the same states manager for callbacks.
//!
//! // In the callback, consume the returned state before exchanging the code.
//! let data = states.validate_state(&state).await?;
//! let provider = providers.get(&data.provider).ok_or_else(|| {
//!     acton_service::error::Error::NotFound("Unknown OAuth provider".to_string())
//! })?;
//! let tokens = provider.exchange_code("authorization-code").await?;
//! let user_info = provider.get_user_info(&tokens.access_token).await?;
//! # Ok(())
//! # }
//! ```
//!
//! The memory manager is per process and needs only `oauth`. For multiple
//! instances, use `RedisOAuthStateManager` with `cache` enabled so any instance
//! can consume a pending login. Applications own HTTP routes and account linking.

pub mod provider;
pub mod providers;
pub mod registry;
pub mod state;

// Core trait and types
pub use provider::{OAuthProvider, OAuthTokens, OAuthUserInfo};
pub use registry::OAuthProviderRegistry;

// Provider implementations
pub use providers::{
    custom::{CustomOidcConfig, CustomOidcProvider},
    github::GitHubProvider,
    google::GoogleProvider,
};

// State management
pub use state::{generate_state, MemoryOAuthStateManager, OAuthStateManager, StateData};

#[cfg(feature = "cache")]
pub use state::RedisOAuthStateManager;
