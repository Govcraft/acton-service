//! Token authentication abstraction layer
//!
//! Provides a unified interface for token validation supporting both PASETO (default)
//! and JWT (feature-gated).

use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};

#[cfg(feature = "cache")]
use async_trait::async_trait;

use crate::error::Error;

/// Claims structure for authenticated requests
///
/// This structure is format-agnostic and works with both PASETO and JWT tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject (user ID or client ID)
    pub sub: String,

    /// Email (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// Username (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// Roles
    #[serde(default)]
    pub roles: Vec<String>,

    /// Permissions
    #[serde(default)]
    pub perms: Vec<String>,

    /// Expiration time (Unix timestamp)
    pub exp: i64,

    /// Issued at (Unix timestamp)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<i64>,

    /// Token ID (for revocation)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,

    /// Issuer (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,

    /// Audience (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aud: Option<String>,
}

impl Claims {
    /// Check if the token has a specific role
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }

    /// Check if the token has a specific permission
    pub fn has_permission(&self, perm: &str) -> bool {
        self.perms.iter().any(|p| p == perm)
    }

    /// Check if the token has a specific role and permission
    pub fn has_role_and_permission(&self, role: &str, perm: &str) -> bool {
        self.has_role(role) && self.has_permission(perm)
    }

    /// Check if the token belongs to a user (sub starts with "user:")
    pub fn is_user(&self) -> bool {
        self.sub.starts_with("user:")
    }

    /// Check if the token belongs to a client (sub starts with "client:")
    pub fn is_client(&self) -> bool {
        self.sub.starts_with("client:")
    }

    /// Get the user ID (if this is a user token)
    pub fn user_id(&self) -> Option<&str> {
        if self.is_user() {
            self.sub.strip_prefix("user:")
        } else {
            None
        }
    }

    /// Get the client ID (if this is a client token)
    pub fn client_id(&self) -> Option<&str> {
        if self.is_client() {
            self.sub.strip_prefix("client:")
        } else {
            None
        }
    }
}

/// Token validator trait
///
/// Abstracts token validation for different formats (PASETO, JWT).
pub trait TokenValidator: Send + Sync + Clone {
    /// Validate a token and extract claims
    fn validate_token(&self, token: &str) -> Result<Claims, Error>;
}

/// Extract token from Authorization header (Bearer scheme)
///
/// This is a shared utility function used by all token validators.
pub fn extract_token(headers: &HeaderMap) -> Result<String, Error> {
    let auth_header = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::Unauthorized("Missing Authorization header".to_string()))?;

    if let Some(token) = auth_header.strip_prefix("Bearer ") {
        Ok(token.to_string())
    } else {
        Err(Error::Unauthorized(
            "Invalid Authorization header format".to_string(),
        ))
    }
}

/// Token revocation trait (requires cache feature)
///
/// Implementations of this trait provide storage for revoked token IDs (jti).
/// This allows tokens to be invalidated before their expiration time.
#[cfg(feature = "cache")]
#[async_trait]
pub trait TokenRevocation: Send + Sync {
    /// Check if a token ID (jti) has been revoked
    async fn is_revoked(&self, jti: &str) -> Result<bool, Error>;

    /// Revoke a token ID (jti) with a TTL in seconds
    ///
    /// The TTL should typically match the token's expiration time to prevent
    /// the revocation list from growing unbounded.
    async fn revoke(&self, jti: &str, ttl_secs: u64) -> Result<(), Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claims_user_detection() {
        let claims = Claims {
            sub: "user:123".to_string(),
            email: None,
            username: None,
            roles: vec!["user".to_string()],
            perms: vec![],
            exp: 0,
            iat: None,
            jti: None,
            iss: None,
            aud: None,
        };

        assert!(claims.is_user());
        assert!(!claims.is_client());
        assert_eq!(claims.user_id(), Some("123"));
        assert_eq!(claims.client_id(), None);
    }

    #[test]
    fn test_claims_client_detection() {
        let claims = Claims {
            sub: "client:abc123".to_string(),
            email: None,
            username: None,
            roles: vec![],
            perms: vec![],
            exp: 0,
            iat: None,
            jti: None,
            iss: None,
            aud: None,
        };

        assert!(!claims.is_user());
        assert!(claims.is_client());
        assert_eq!(claims.user_id(), None);
        assert_eq!(claims.client_id(), Some("abc123"));
    }

    #[test]
    fn test_claims_role_check() {
        let claims = Claims {
            sub: "user:123".to_string(),
            email: None,
            username: None,
            roles: vec!["admin".to_string(), "user".to_string()],
            perms: vec!["ban_user".to_string()],
            exp: 0,
            iat: None,
            jti: None,
            iss: None,
            aud: None,
        };

        assert!(claims.has_role("admin"));
        assert!(claims.has_role("user"));
        assert!(!claims.has_role("super_admin"));
        assert!(claims.has_permission("ban_user"));
        assert!(!claims.has_permission("delete_system"));
    }

    #[cfg(feature = "cache")]
    #[test]
    fn test_token_revocation_trait_is_object_safe() {
        // This test ensures TokenRevocation can be used as a trait object
        // If this compiles, the trait is object-safe
        fn _assert_object_safe(_: &dyn TokenRevocation) {}
    }
}
