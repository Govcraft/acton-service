//! Authentication module for token generation, password hashing, and more
//!
//! This module complements the existing token validation middleware with
//! token generation capabilities, password hashing, API key management,
//! and OAuth/OIDC support.
//!
//! # Features
//!
//! - `auth` - Core authentication: password hashing (Argon2id) + token generation
//! - `oauth` - OAuth/OIDC provider support (requires `auth`)
//! - `auth-full` - All auth features combined
//!
//! # Example
//!
//! ```rust,ignore
//! use acton_service::auth::{PasswordHasher, PasetoGenerator, TokenGenerator};
//! use acton_service::middleware::Claims;
//!
//! // Hash a password
//! let hasher = PasswordHasher::default();
//! let hash = hasher.hash("my_secure_password")?;
//!
//! // Verify a password
//! assert!(hasher.verify("my_secure_password", &hash)?);
//!
//! // Generate a token
//! let generator = PasetoGenerator::new(&config)?;
//! let claims = Claims { sub: "user:123".to_string(), /* ... */ };
//! let token = generator.generate_token(&claims)?;
//! ```

// Configuration
pub mod config;

// Password hashing (Argon2id)
pub mod password;

// Token generation
pub mod tokens;

// API key management
pub mod api_keys;

// OAuth/OIDC providers (requires oauth feature)
#[cfg(feature = "oauth")]
pub mod oauth;

// Re-exports for convenience
pub use config::{
    AuthConfig, PasswordConfig, TokenGenerationConfig, PasetoGenerationConfig,
    RefreshTokenConfig,
};

#[cfg(feature = "oauth")]
pub use config::{ApiKeyConfig, OAuthConfig, OAuthProviderConfig};

pub use password::PasswordHasher;

pub use tokens::{TokenGenerator, TokenPair};
pub use tokens::paseto_generator::PasetoGenerator;
pub use tokens::refresh::{RefreshTokenStorage, RefreshTokenData, RefreshTokenMetadata};

#[cfg(feature = "cache")]
pub use tokens::refresh::RedisRefreshStorage;

#[cfg(feature = "database")]
pub use tokens::refresh::PgRefreshStorage;

#[cfg(feature = "turso")]
pub use tokens::refresh::TursoRefreshStorage;

#[cfg(feature = "jwt")]
pub use tokens::jwt_generator::JwtGenerator;

// API key exports
pub use api_keys::{ApiKey, ApiKeyGenerator, ApiKeyStorage};

#[cfg(feature = "cache")]
pub use api_keys::RedisApiKeyStorage;

#[cfg(feature = "database")]
pub use api_keys::PgApiKeyStorage;

#[cfg(feature = "turso")]
pub use api_keys::TursoApiKeyStorage;

// OAuth exports (requires oauth feature)
#[cfg(feature = "oauth")]
pub use oauth::{
    OAuthProvider, OAuthTokens, OAuthUserInfo,
    GoogleProvider, GitHubProvider, CustomOidcProvider, CustomOidcConfig,
    OAuthStateManager, StateData, generate_state,
};

#[cfg(all(feature = "oauth", feature = "cache"))]
pub use oauth::RedisOAuthStateManager;
