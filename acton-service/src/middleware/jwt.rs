//! JWT authentication middleware

use axum::{
    body::Body,
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::{fs, sync::Arc};

#[cfg(feature = "cache")]
use async_trait::async_trait;

#[cfg(feature = "cache")]
use deadpool_redis::Pool as RedisPool;

use crate::{config::JwtConfig, error::Error};

/// JWT Claims structure
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

    /// JWT ID (for revocation)
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

/// Trait for JWT revocation storage
///
/// Implementations of this trait provide storage for revoked JWT IDs (jti).
/// This allows tokens to be invalidated before their expiration time.
#[cfg(feature = "cache")]
#[async_trait]
pub trait JwtRevocation: Send + Sync {
    /// Check if a JWT ID (jti) has been revoked
    async fn is_revoked(&self, jti: &str) -> Result<bool, Error>;

    /// Revoke a JWT ID (jti) with a TTL in seconds
    ///
    /// The TTL should typically match the token's expiration time to prevent
    /// the revocation list from growing unbounded.
    async fn revoke(&self, jti: &str, ttl_secs: u64) -> Result<(), Error>;
}

/// Redis-based JWT revocation implementation
///
/// Stores revoked JTIs in Redis with automatic expiration (SETEX).
/// The key pattern is `jwt:revoked:{jti}`.
#[cfg(feature = "cache")]
#[derive(Clone)]
pub struct RedisJwtRevocation {
    pool: RedisPool,
}

#[cfg(feature = "cache")]
impl RedisJwtRevocation {
    /// Create a new Redis JWT revocation checker
    pub fn new(pool: RedisPool) -> Self {
        Self { pool }
    }

    /// Get the Redis key for a given JTI
    fn revocation_key(jti: &str) -> String {
        format!("jwt:revoked:{}", jti)
    }
}

#[cfg(feature = "cache")]
#[async_trait]
impl JwtRevocation for RedisJwtRevocation {
    async fn is_revoked(&self, jti: &str) -> Result<bool, Error> {
        use deadpool_redis::redis::AsyncCommands;

        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| Error::Internal(format!("Failed to get Redis connection: {}", e)))?;

        let key = Self::revocation_key(jti);
        let exists: bool = conn
            .exists(&key)
            .await
            .map_err(|e| Error::Internal(format!("Failed to check revocation status: {}", e)))?;

        Ok(exists)
    }

    async fn revoke(&self, jti: &str, ttl_secs: u64) -> Result<(), Error> {
        use deadpool_redis::redis::AsyncCommands;

        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| Error::Internal(format!("Failed to get Redis connection: {}", e)))?;

        let key = Self::revocation_key(jti);
        // Store "1" as a marker with TTL
        conn.set_ex::<_, _, ()>(&key, 1, ttl_secs)
            .await
            .map_err(|e| Error::Internal(format!("Failed to revoke JWT: {}", e)))?;

        Ok(())
    }
}

/// JWT authentication middleware state
#[derive(Clone)]
pub struct JwtAuth {
    decoding_key: Arc<DecodingKey>,
    validation: Validation,
    #[cfg(feature = "cache")]
    revocation: Option<Arc<dyn JwtRevocation>>,
}

impl JwtAuth {
    /// Create a new JWT authentication middleware
    pub fn new(config: &JwtConfig) -> Result<Self, Error> {
        // Read the public key file
        let public_key = fs::read(&config.public_key_path).map_err(|e| {
            let path_display = config.public_key_path.display().to_string();
            Error::Config(Box::new(figment::Error::from(format!(
                "Failed to read JWT public key from path '{}'\n\n\
                Troubleshooting:\n\
                1. Verify the file exists: ls -la {}\n\
                2. Check file permissions (must be readable)\n\
                3. Verify the path is correct in configuration\n\
                4. For RS256/ES256: Use PEM format public key\n\
                5. For HS256: Use raw secret file\n\n\
                Error: {}",
                path_display,
                path_display,
                e
            ))))
        })?;

        // Parse the algorithm
        let algorithm = match config.algorithm.to_uppercase().as_str() {
            "RS256" => Algorithm::RS256,
            "RS384" => Algorithm::RS384,
            "RS512" => Algorithm::RS512,
            "ES256" => Algorithm::ES256,
            "ES384" => Algorithm::ES384,
            "HS256" => Algorithm::HS256,
            "HS384" => Algorithm::HS384,
            "HS512" => Algorithm::HS512,
            alg => {
                return Err(Error::Config(Box::new(figment::Error::from(format!(
                    "Unsupported JWT algorithm: {}",
                    alg
                )))))
            }
        };

        // Create decoding key based on algorithm
        let decoding_key = match algorithm {
            Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 => {
                DecodingKey::from_rsa_pem(&public_key)?
            }
            Algorithm::ES256 | Algorithm::ES384 => DecodingKey::from_ec_pem(&public_key)?,
            Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512 => {
                DecodingKey::from_secret(&public_key)
            }
            _ => {
                return Err(Error::Config(Box::new(figment::Error::from(format!(
                    "Unsupported algorithm: {:?}",
                    algorithm
                )))))
            }
        };

        // Create validation rules
        let mut validation = Validation::new(algorithm);
        if let Some(issuer) = &config.issuer {
            validation.set_issuer(&[issuer]);
        }
        if let Some(audience) = &config.audience {
            validation.set_audience(&[audience]);
        }

        Ok(Self {
            decoding_key: Arc::new(decoding_key),
            validation,
            #[cfg(feature = "cache")]
            revocation: None,
        })
    }

    /// Set the JWT revocation checker
    ///
    /// This allows the middleware to check if tokens have been revoked.
    /// Typically used with `RedisJwtRevocation`.
    #[cfg(feature = "cache")]
    pub fn with_revocation<R: JwtRevocation + 'static>(mut self, revocation: R) -> Self {
        self.revocation = Some(Arc::new(revocation));
        self
    }

    /// Validate and decode a JWT token
    pub fn validate_token(&self, token: &str) -> Result<Claims, Error> {
        let token_data = decode::<Claims>(token, &self.decoding_key, &self.validation)?;
        Ok(token_data.claims)
    }

    /// Extract token from Authorization header
    pub fn extract_token(headers: &HeaderMap) -> Result<String, Error> {
        let auth_header = headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| Error::Unauthorized("Missing Authorization header".to_string()))?;

        if let Some(token) = auth_header.strip_prefix("Bearer ") {
            Ok(token.to_string())
        } else {
            Err(Error::Unauthorized("Invalid Authorization header format".to_string()))
        }
    }

    /// Middleware function to validate JWT and inject claims
    pub async fn middleware(
        State(auth): State<Self>,
        mut request: Request<Body>,
        next: Next,
    ) -> Result<Response, Error> {
        // Extract token from headers
        let token = Self::extract_token(request.headers())?;

        // Validate token and extract claims
        let claims = auth.validate_token(&token)?;

        // Check JTI revocation if cache feature is enabled and revocation checker is configured
        #[cfg(feature = "cache")]
        if let Some(revocation) = &auth.revocation {
            if let Some(jti) = &claims.jti {
                if revocation.is_revoked(jti).await? {
                    return Err(Error::Unauthorized("Token has been revoked".to_string()));
                }
            } else {
                // If revocation is configured but token has no JTI, log a warning
                // but allow the request (for backward compatibility)
                tracing::warn!("JWT revocation is enabled but token has no JTI claim");
            }
        }

        // Inject claims into request extensions
        request.extensions_mut().insert(claims);

        Ok(next.run(request).await)
    }
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

    #[test]
    fn test_revocation_key_format() {
        #[cfg(feature = "cache")]
        {
            let jti = "test-jwt-id-123";
            let key = RedisJwtRevocation::revocation_key(jti);
            assert_eq!(key, "jwt:revoked:test-jwt-id-123");
        }
    }

    // Integration tests for Redis revocation would require a Redis instance
    // These should be in integration tests with testcontainers
    #[cfg(all(test, feature = "cache"))]
    mod revocation_tests {
        use super::*;

        #[test]
        fn test_revocation_trait_is_object_safe() {
            // This test ensures JwtRevocation can be used as a trait object
            // If this compiles, the trait is object-safe
            fn _assert_object_safe(_: &dyn JwtRevocation) {}
        }
    }
}
