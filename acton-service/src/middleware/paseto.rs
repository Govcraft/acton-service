//! PASETO authentication middleware
//!
//! Implements token validation using rusty_paseto for V4 Local and V4 Public tokens.

use axum::{
    body::Body,
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use rusty_paseto::prelude::*;
use std::{fs, sync::Arc};

#[cfg(feature = "cache")]
use super::token::TokenRevocation;

use super::token::{extract_token, Claims, TokenValidator};
use crate::{config::PasetoConfig, error::Error};

/// Internal key storage for PASETO authentication
enum PasetoKey {
    /// V4 Local (symmetric encryption)
    V4Local {
        key_bytes: [u8; 32],
        issuer: Option<String>,
        audience: Option<String>,
    },
    /// V4 Public (asymmetric signature)
    V4Public {
        key_bytes: [u8; 32],
        issuer: Option<String>,
        audience: Option<String>,
    },
}

/// PASETO authentication middleware state
#[derive(Clone)]
pub struct PasetoAuth {
    inner: Arc<PasetoKey>,
    #[cfg(feature = "cache")]
    revocation: Option<Arc<dyn TokenRevocation>>,
}

impl PasetoAuth {
    /// Create a new PASETO authentication middleware from configuration
    pub fn new(config: &PasetoConfig) -> Result<Self, Error> {
        let key_bytes = fs::read(&config.key_path).map_err(|e| {
            let path_display = config.key_path.display().to_string();
            Error::Config(Box::new(figment::Error::from(format!(
                "Failed to read PASETO key from path '{}'\n\n\
                Troubleshooting:\n\
                1. Verify the file exists: ls -la {}\n\
                2. Check file permissions (must be readable)\n\
                3. Verify the path is correct in configuration\n\
                4. For v4.local: Use a 32-byte symmetric key\n\
                5. For v4.public: Use a 32-byte Ed25519 public key\n\n\
                Error: {}",
                path_display, path_display, e
            ))))
        })?;

        let inner = match (config.version.as_str(), config.purpose.as_str()) {
            ("v4", "local") => {
                if key_bytes.len() != 32 {
                    return Err(Error::Config(Box::new(figment::Error::from(format!(
                        "PASETO v4.local requires a 32-byte symmetric key, got {} bytes",
                        key_bytes.len()
                    )))));
                }
                let key_array: [u8; 32] = key_bytes
                    .try_into()
                    .map_err(|_| Error::Config(Box::new(figment::Error::from(
                        "Failed to convert key bytes to 32-byte array"
                    ))))?;
                PasetoKey::V4Local {
                    key_bytes: key_array,
                    issuer: config.issuer.clone(),
                    audience: config.audience.clone(),
                }
            }
            ("v4", "public") => {
                if key_bytes.len() != 32 {
                    return Err(Error::Config(Box::new(figment::Error::from(format!(
                        "PASETO v4.public requires a 32-byte Ed25519 public key, got {} bytes",
                        key_bytes.len()
                    )))));
                }
                let key_array: [u8; 32] = key_bytes
                    .try_into()
                    .map_err(|_| Error::Config(Box::new(figment::Error::from(
                        "Failed to convert key bytes to 32-byte array"
                    ))))?;
                PasetoKey::V4Public {
                    key_bytes: key_array,
                    issuer: config.issuer.clone(),
                    audience: config.audience.clone(),
                }
            }
            (version, purpose) => {
                return Err(Error::Config(Box::new(figment::Error::from(format!(
                    "Unsupported PASETO version/purpose: {}.{}\n\
                    Supported combinations: v4.local, v4.public",
                    version, purpose
                )))));
            }
        };

        Ok(Self {
            inner: Arc::new(inner),
            #[cfg(feature = "cache")]
            revocation: None,
        })
    }

    /// Set the token revocation checker
    ///
    /// This allows the middleware to check if tokens have been revoked.
    #[cfg(feature = "cache")]
    pub fn with_revocation<R: TokenRevocation + 'static>(mut self, revocation: R) -> Self {
        self.revocation = Some(Arc::new(revocation));
        self
    }

    /// Middleware function to validate PASETO and inject claims
    pub async fn middleware(
        State(auth): State<Self>,
        mut request: Request<Body>,
        next: Next,
    ) -> Result<Response, Error> {
        // Skip authentication for health and readiness endpoints
        let path = request.uri().path();
        if path == "/health" || path == "/ready" {
            return Ok(next.run(request).await);
        }

        // Extract token from headers
        let token = extract_token(request.headers())?;

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
                tracing::warn!("Token revocation is enabled but token has no jti claim");
            }
        }

        // Inject claims into request extensions
        request.extensions_mut().insert(claims);

        Ok(next.run(request).await)
    }

    /// Convert serde_json::Value to Claims
    fn json_to_claims(json: serde_json::Value) -> Result<Claims, Error> {
        let sub = json["sub"]
            .as_str()
            .ok_or_else(|| Error::Paseto("Missing 'sub' claim".to_string()))?
            .to_string();

        // PASETO uses ISO8601 for exp, convert to Unix timestamp
        let exp = if let Some(exp_str) = json["exp"].as_str() {
            chrono::DateTime::parse_from_rfc3339(exp_str)
                .map(|dt| dt.timestamp())
                .map_err(|_| Error::Paseto("Invalid 'exp' claim format".to_string()))?
        } else if let Some(exp_num) = json["exp"].as_i64() {
            // Some implementations may use Unix timestamp directly
            exp_num
        } else {
            return Err(Error::Paseto("Missing or invalid 'exp' claim".to_string()));
        };

        // Parse iat if present
        let iat = if let Some(iat_str) = json["iat"].as_str() {
            chrono::DateTime::parse_from_rfc3339(iat_str)
                .map(|dt| Some(dt.timestamp()))
                .unwrap_or(None)
        } else {
            json["iat"].as_i64()
        };

        Ok(Claims {
            sub,
            email: json["email"].as_str().map(String::from),
            username: json["username"].as_str().map(String::from),
            roles: json["roles"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            perms: json["perms"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            exp,
            iat,
            jti: json["jti"].as_str().map(String::from),
            iss: json["iss"].as_str().map(String::from),
            aud: json["aud"].as_str().map(String::from),
        })
    }
}

impl TokenValidator for PasetoAuth {
    fn validate_token(&self, token: &str) -> Result<Claims, Error> {
        let json_value = match self.inner.as_ref() {
            PasetoKey::V4Local {
                key_bytes,
                issuer,
                audience,
            } => {
                let key = PasetoSymmetricKey::<V4, Local>::from(Key::from(key_bytes));
                let mut parser = PasetoParser::<V4, Local>::default();

                if let Some(iss) = issuer {
                    parser.check_claim(IssuerClaim::from(iss.as_str()));
                }
                if let Some(aud) = audience {
                    parser.check_claim(AudienceClaim::from(aud.as_str()));
                }

                parser
                    .parse(token, &key)
                    .map_err(|e| Error::Paseto(format!("Invalid PASETO token: {}", e)))?
            }
            PasetoKey::V4Public {
                key_bytes,
                issuer,
                audience,
            } => {
                let raw_key = Key::from(key_bytes);
                let key = PasetoAsymmetricPublicKey::<V4, Public>::from(&raw_key);
                let mut parser = PasetoParser::<V4, Public>::default();

                if let Some(iss) = issuer {
                    parser.check_claim(IssuerClaim::from(iss.as_str()));
                }
                if let Some(aud) = audience {
                    parser.check_claim(AudienceClaim::from(aud.as_str()));
                }

                parser
                    .parse(token, &key)
                    .map_err(|e| Error::Paseto(format!("Invalid PASETO token: {}", e)))?
            }
        };

        Self::json_to_claims(json_value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_to_claims_with_rfc3339_exp() {
        let json = serde_json::json!({
            "sub": "user:123",
            "exp": "2099-01-01T00:00:00+00:00",
            "iat": "2024-01-01T00:00:00+00:00",
            "roles": ["admin", "user"],
            "perms": ["read", "write"],
            "email": "test@example.com"
        });

        let claims = PasetoAuth::json_to_claims(json).unwrap();
        assert_eq!(claims.sub, "user:123");
        assert!(claims.exp > 0);
        assert!(claims.iat.is_some());
        assert_eq!(claims.roles, vec!["admin", "user"]);
        assert_eq!(claims.perms, vec!["read", "write"]);
        assert_eq!(claims.email, Some("test@example.com".to_string()));
    }

    #[test]
    fn test_json_to_claims_with_unix_exp() {
        let json = serde_json::json!({
            "sub": "client:abc",
            "exp": 4102444800_i64,
            "roles": []
        });

        let claims = PasetoAuth::json_to_claims(json).unwrap();
        assert_eq!(claims.sub, "client:abc");
        assert_eq!(claims.exp, 4102444800);
    }

    #[test]
    fn test_json_to_claims_missing_sub() {
        let json = serde_json::json!({
            "exp": "2099-01-01T00:00:00+00:00"
        });

        let result = PasetoAuth::json_to_claims(json);
        assert!(result.is_err());
    }
}
