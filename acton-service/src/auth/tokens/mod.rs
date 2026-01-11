//! Token generation module
//!
//! Provides token generation capabilities that complement the existing
//! token validation middleware. Supports PASETO (default) and JWT (feature-gated).
//!
//! # Example
//!
//! ```rust,ignore
//! use acton_service::auth::{PasetoGenerator, TokenGenerator};
//! use acton_service::middleware::Claims;
//! use std::time::Duration;
//!
//! let generator = PasetoGenerator::new(&config)?;
//!
//! let claims = Claims {
//!     sub: "user:123".to_string(),
//!     email: Some("user@example.com".to_string()),
//!     roles: vec!["user".to_string()],
//!     // ... other fields
//! };
//!
//! // Generate with default expiration
//! let token = generator.generate_token(&claims)?;
//!
//! // Generate with custom expiration
//! let token = generator.generate_token_with_expiry(&claims, Duration::from_secs(3600))?;
//! ```

pub mod paseto_generator;

#[cfg(feature = "jwt")]
pub mod jwt_generator;

pub mod refresh;

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::error::Error;
use crate::middleware::Claims;

/// Token generation trait
///
/// Abstracts token generation for different formats (PASETO, JWT).
/// This complements the `TokenValidator` trait for validation.
pub trait TokenGenerator: Send + Sync + Clone {
    /// Generate a token from claims using default expiration
    ///
    /// The expiration time is determined by the generator's configuration.
    fn generate_token(&self, claims: &Claims) -> Result<String, Error>;

    /// Generate a token with a custom expiration duration
    ///
    /// # Arguments
    ///
    /// * `claims` - The claims to include in the token
    /// * `expires_in` - Duration from now until token expiration
    fn generate_token_with_expiry(
        &self,
        claims: &Claims,
        expires_in: Duration,
    ) -> Result<String, Error>;

    /// Get the default token lifetime
    fn default_lifetime(&self) -> Duration;
}

/// Token pair containing access and refresh tokens
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPair {
    /// The access token (short-lived)
    pub access_token: String,

    /// The refresh token (long-lived)
    pub refresh_token: String,

    /// Token type (always "Bearer")
    pub token_type: String,

    /// Access token lifetime in seconds
    pub expires_in: i64,

    /// Refresh token lifetime in seconds
    pub refresh_expires_in: i64,
}

impl TokenPair {
    /// Create a new token pair
    pub fn new(
        access_token: String,
        refresh_token: String,
        expires_in: i64,
        refresh_expires_in: i64,
    ) -> Self {
        Self {
            access_token,
            refresh_token,
            token_type: "Bearer".to_string(),
            expires_in,
            refresh_expires_in,
        }
    }
}

/// Builder for creating Claims with sensible defaults
#[derive(Debug, Clone, Default)]
pub struct ClaimsBuilder {
    sub: Option<String>,
    email: Option<String>,
    username: Option<String>,
    roles: Vec<String>,
    perms: Vec<String>,
    iss: Option<String>,
    aud: Option<String>,
}

impl ClaimsBuilder {
    /// Create a new claims builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the subject (user or client ID)
    pub fn subject(mut self, sub: impl Into<String>) -> Self {
        self.sub = Some(sub.into());
        self
    }

    /// Set a user subject (adds "user:" prefix)
    pub fn user(mut self, user_id: impl Into<String>) -> Self {
        self.sub = Some(format!("user:{}", user_id.into()));
        self
    }

    /// Set a client subject (adds "client:" prefix)
    pub fn client(mut self, client_id: impl Into<String>) -> Self {
        self.sub = Some(format!("client:{}", client_id.into()));
        self
    }

    /// Set the email
    pub fn email(mut self, email: impl Into<String>) -> Self {
        self.email = Some(email.into());
        self
    }

    /// Set the username
    pub fn username(mut self, username: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self
    }

    /// Add a role
    pub fn role(mut self, role: impl Into<String>) -> Self {
        self.roles.push(role.into());
        self
    }

    /// Add multiple roles
    pub fn roles(mut self, roles: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.roles.extend(roles.into_iter().map(Into::into));
        self
    }

    /// Add a permission
    pub fn permission(mut self, perm: impl Into<String>) -> Self {
        self.perms.push(perm.into());
        self
    }

    /// Add multiple permissions
    pub fn permissions(mut self, perms: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.perms.extend(perms.into_iter().map(Into::into));
        self
    }

    /// Set the issuer
    pub fn issuer(mut self, iss: impl Into<String>) -> Self {
        self.iss = Some(iss.into());
        self
    }

    /// Set the audience
    pub fn audience(mut self, aud: impl Into<String>) -> Self {
        self.aud = Some(aud.into());
        self
    }

    /// Build the Claims (without expiration - that's set by the generator)
    ///
    /// Note: `exp`, `iat`, and `jti` are set by the token generator.
    pub fn build(self) -> Result<Claims, Error> {
        let sub = self
            .sub
            .ok_or_else(|| Error::ValidationError("Subject (sub) is required".to_string()))?;

        Ok(Claims {
            sub,
            email: self.email,
            username: self.username,
            roles: self.roles,
            perms: self.perms,
            exp: 0, // Set by generator
            iat: None, // Set by generator
            jti: None, // Set by generator
            iss: self.iss,
            aud: self.aud,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claims_builder_user() {
        let claims = ClaimsBuilder::new()
            .user("123")
            .email("test@example.com")
            .role("user")
            .role("admin")
            .permission("read:docs")
            .build()
            .unwrap();

        assert_eq!(claims.sub, "user:123");
        assert_eq!(claims.email, Some("test@example.com".to_string()));
        assert_eq!(claims.roles, vec!["user", "admin"]);
        assert_eq!(claims.perms, vec!["read:docs"]);
    }

    #[test]
    fn test_claims_builder_client() {
        let claims = ClaimsBuilder::new()
            .client("api-client-abc")
            .roles(["service"])
            .build()
            .unwrap();

        assert_eq!(claims.sub, "client:api-client-abc");
        assert_eq!(claims.roles, vec!["service"]);
    }

    #[test]
    fn test_claims_builder_missing_subject() {
        let result = ClaimsBuilder::new().email("test@example.com").build();

        assert!(result.is_err());
    }

    #[test]
    fn test_token_pair_creation() {
        let pair = TokenPair::new(
            "access_token".to_string(),
            "refresh_token".to_string(),
            900,
            604800,
        );

        assert_eq!(pair.access_token, "access_token");
        assert_eq!(pair.refresh_token, "refresh_token");
        assert_eq!(pair.token_type, "Bearer");
        assert_eq!(pair.expires_in, 900);
        assert_eq!(pair.refresh_expires_in, 604800);
    }
}
