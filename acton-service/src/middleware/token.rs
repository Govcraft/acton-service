//! Token authentication abstraction layer
//!
//! Provides a unified interface for token validation supporting both PASETO (default)
//! and JWT (feature-gated).

use std::collections::HashMap;

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

    /// Custom claims (arbitrary key-value pairs)
    ///
    /// Any claims not matching the known fields above are captured here.
    /// This supports both rusty_paseto's `CustomClaim` and JWT's flexible
    /// payload structure.
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub custom: HashMap<String, serde_json::Value>,
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

    /// Get a custom claim value by key
    pub fn custom_claim(&self, key: &str) -> Option<&serde_json::Value> {
        self.custom.get(key)
    }

    /// Get a custom claim as a typed value, returning `None` if the key is
    /// missing or deserialization fails
    pub fn custom_claim_as<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.custom
            .get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
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
            custom: HashMap::new(),
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
            custom: HashMap::new(),
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
            custom: HashMap::new(),
        };

        assert!(claims.has_role("admin"));
        assert!(claims.has_role("user"));
        assert!(!claims.has_role("super_admin"));
        assert!(claims.has_permission("ban_user"));
        assert!(!claims.has_permission("delete_system"));
    }

    #[test]
    fn test_custom_claims() {
        let mut custom = HashMap::new();
        custom.insert(
            "tenant_id".to_string(),
            serde_json::Value::String("org-42".to_string()),
        );
        custom.insert(
            "feature_flags".to_string(),
            serde_json::json!(["beta", "dark_mode"]),
        );

        let claims = Claims {
            sub: "user:123".to_string(),
            email: None,
            username: None,
            roles: vec![],
            perms: vec![],
            exp: 0,
            iat: None,
            jti: None,
            iss: None,
            aud: None,
            custom,
        };

        assert_eq!(
            claims.custom_claim("tenant_id"),
            Some(&serde_json::Value::String("org-42".to_string()))
        );
        assert_eq!(
            claims.custom_claim_as::<String>("tenant_id"),
            Some("org-42".to_string())
        );
        assert_eq!(
            claims.custom_claim_as::<Vec<String>>("feature_flags"),
            Some(vec!["beta".to_string(), "dark_mode".to_string()])
        );
        assert_eq!(claims.custom_claim("nonexistent"), None);
    }

    #[test]
    fn test_custom_claims_serde_flatten() {
        let json = serde_json::json!({
            "sub": "user:1",
            "exp": 9999999999_i64,
            "tenant_id": "org-42",
            "level": 5
        });

        let claims: Claims = serde_json::from_value(json).unwrap();
        assert_eq!(claims.sub, "user:1");
        assert_eq!(
            claims.custom_claim_as::<String>("tenant_id"),
            Some("org-42".to_string())
        );
        assert_eq!(claims.custom_claim_as::<i64>("level"), Some(5));

        // Round-trip: custom claims should serialize back
        let serialized = serde_json::to_value(&claims).unwrap();
        assert_eq!(serialized["tenant_id"], "org-42");
        assert_eq!(serialized["level"], 5);
    }

    #[cfg(feature = "cache")]
    #[test]
    fn test_token_revocation_trait_is_object_safe() {
        // This test ensures TokenRevocation can be used as a trait object
        // If this compiles, the trait is object-safe
        fn _assert_object_safe(_: &dyn TokenRevocation) {}
    }
}
