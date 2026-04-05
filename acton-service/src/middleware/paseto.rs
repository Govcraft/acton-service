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

#[cfg(feature = "auth")]
use crate::auth::key_rotation::manager::KeyManager;

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
///
/// When a [`KeyManager`] is configured via [`with_key_manager`](Self::with_key_manager),
/// validation will first try the static key, and on failure will try each
/// verification key from the key rotation system. This ensures both pre-rotation
/// tokens (signed with the static key) and post-rotation tokens (signed with
/// rotated keys) validate correctly.
#[derive(Clone)]
pub struct PasetoAuth {
    inner: Arc<PasetoKey>,
    #[cfg(feature = "cache")]
    revocation: Option<Arc<dyn TokenRevocation>>,
    #[cfg(feature = "auth")]
    key_manager: Option<Arc<KeyManager>>,
    /// Path prefixes that bypass token authentication.
    public_paths: Arc<[String]>,
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
                let key_array: [u8; 32] = key_bytes.try_into().map_err(|_| {
                    Error::Config(Box::new(figment::Error::from(
                        "Failed to convert key bytes to 32-byte array",
                    )))
                })?;
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
                let key_array: [u8; 32] = key_bytes.try_into().map_err(|_| {
                    Error::Config(Box::new(figment::Error::from(
                        "Failed to convert key bytes to 32-byte array",
                    )))
                })?;
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
            #[cfg(feature = "auth")]
            key_manager: None,
            public_paths: config.public_paths.clone().into(),
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

    /// Set the key manager for key rotation support
    ///
    /// When a key manager is configured, token validation will first try the
    /// static key. If that fails, it will try each verification key (Active +
    /// Draining) from the key rotation system. This ensures backward compatibility:
    /// tokens issued before rotation was enabled validate with the static key,
    /// while rotated tokens validate with the appropriate rotated key.
    #[cfg(feature = "auth")]
    pub fn with_key_manager(mut self, key_manager: Arc<KeyManager>) -> Self {
        self.key_manager = Some(key_manager);
        self
    }

    /// Middleware function to validate PASETO and inject claims
    pub async fn middleware(
        State(auth): State<Self>,
        mut request: Request<Body>,
        next: Next,
    ) -> Result<Response, Error> {
        // Skip authentication for infrastructure endpoints and configured public paths
        let path = request.uri().path();
        if path == "/health"
            || path == "/ready"
            || path.starts_with("/swagger-ui")
            || path.starts_with("/api-docs")
            || auth.public_paths.iter().any(|p| path.starts_with(p.as_str()))
        {
            return Ok(next.run(request).await);
        }

        // Build audit source info before validation (available regardless of outcome)
        #[cfg(feature = "audit")]
        let audit_source = {
            use crate::audit::event::AuditSource;
            AuditSource {
                ip: request
                    .headers()
                    .get("x-forwarded-for")
                    .or_else(|| request.headers().get("x-real-ip"))
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.split(',').next().unwrap_or(s).trim().to_string()),
                user_agent: request
                    .headers()
                    .get("user-agent")
                    .and_then(|v| v.to_str().ok())
                    .map(String::from),
                subject: None, // Not yet known
                request_id: request
                    .headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .map(String::from),
            }
        };

        #[cfg(feature = "audit")]
        let audit_logger = request
            .extensions()
            .get::<crate::audit::AuditLogger>()
            .cloned();

        // Extract token from headers
        let token = match extract_token(request.headers()) {
            Ok(t) => t,
            Err(e) => {
                #[cfg(feature = "audit")]
                if let Some(ref logger) = audit_logger {
                    if logger.config().audit_auth_events {
                        logger
                            .log_auth(
                                crate::audit::event::AuditEventKind::AuthLoginFailed,
                                crate::audit::event::AuditSeverity::Warning,
                                audit_source,
                            )
                            .await;
                    }
                }
                return Err(e);
            }
        };

        // Validate token and extract claims
        let claims = match auth.validate_token(&token) {
            Ok(c) => c,
            Err(e) => {
                #[cfg(feature = "audit")]
                if let Some(ref logger) = audit_logger {
                    if logger.config().audit_auth_events {
                        logger
                            .log_auth(
                                crate::audit::event::AuditEventKind::AuthLoginFailed,
                                crate::audit::event::AuditSeverity::Warning,
                                audit_source,
                            )
                            .await;
                    }
                }
                return Err(e);
            }
        };

        // Check JTI revocation if cache feature is enabled and revocation checker is configured
        #[cfg(feature = "cache")]
        if let Some(revocation) = &auth.revocation {
            if let Some(jti) = &claims.jti {
                if revocation.is_revoked(jti).await? {
                    #[cfg(feature = "audit")]
                    if let Some(ref logger) = audit_logger {
                        if logger.config().audit_auth_events {
                            let mut source = audit_source.clone();
                            source.subject = Some(claims.sub.clone());
                            logger
                                .log_auth(
                                    crate::audit::event::AuditEventKind::AuthTokenRevoked,
                                    crate::audit::event::AuditSeverity::Warning,
                                    source,
                                )
                                .await;
                        }
                    }
                    return Err(Error::Unauthorized("Token has been revoked".to_string()));
                }
            } else {
                // If revocation is configured but token has no JTI, log a warning
                // but allow the request (for backward compatibility)
                tracing::warn!("Token revocation is enabled but token has no jti claim");
            }
        }

        // Emit successful auth audit event
        #[cfg(feature = "audit")]
        if let Some(ref logger) = audit_logger {
            if logger.config().audit_auth_events {
                let mut source = audit_source;
                source.subject = Some(claims.sub.clone());
                logger
                    .log_auth(
                        crate::audit::event::AuditEventKind::AuthLoginSuccess,
                        crate::audit::event::AuditSeverity::Informational,
                        source,
                    )
                    .await;
            }
        }

        // Inject claims into request extensions
        request.extensions_mut().insert(claims);

        Ok(next.run(request).await)
    }

    /// Convert serde_json::Value to Claims
    /// Parse a JSON value that might be an array or a stringified JSON array.
    ///
    /// PASETO custom claims can only store strings, so arrays like `["admin"]`
    /// get serialized to `"[\"admin\"]"` during token generation. This function
    /// handles both the native array case and the stringified case.
    fn parse_string_or_array(value: &serde_json::Value) -> Vec<String> {
        // Direct array
        if let Some(arr) = value.as_array() {
            return arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
        }
        // Stringified JSON array
        if let Some(s) = value.as_str() {
            if let Ok(arr) = serde_json::from_str::<Vec<String>>(s) {
                return arr;
            }
        }
        Vec::new()
    }

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

        // Collect custom claims (any keys not in the standard set)
        let known_keys: &[&str] = &[
            "sub", "exp", "iat", "jti", "iss", "aud", "email", "username", "roles", "perms",
        ];
        let custom = json
            .as_object()
            .map(|obj| {
                obj.iter()
                    .filter(|(k, _)| !known_keys.contains(&k.as_str()))
                    .map(|(k, v)| {
                        // Try to parse stringified JSON back into structured values
                        let parsed = if let Some(s) = v.as_str() {
                            serde_json::from_str(s).unwrap_or_else(|_| v.clone())
                        } else {
                            v.clone()
                        };
                        (k.clone(), parsed)
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(Claims {
            sub,
            email: json["email"].as_str().map(String::from),
            username: json["username"].as_str().map(String::from),
            roles: Self::parse_string_or_array(&json["roles"]),
            perms: Self::parse_string_or_array(&json["perms"]),
            exp,
            iat,
            jti: json["jti"].as_str().map(String::from),
            iss: json["iss"].as_str().map(String::from),
            aud: json["aud"].as_str().map(String::from),
            custom,
        })
    }
}

impl TokenValidator for PasetoAuth {
    fn validate_token(&self, token: &str) -> Result<Claims, Error> {
        // First, try with the static key (backward compatible)
        let static_result = self.validate_with_static_key(token);

        // If static key succeeds, return immediately
        if static_result.is_ok() {
            return static_result;
        }

        // If key_manager is configured, try each verification key from the
        // rotation system (Active + Draining keys). PASETO v4.local tokens do
        // not carry a `kid` in their header, so we must try each key.
        #[cfg(feature = "auth")]
        if let Some(ref km) = self.key_manager {
            let verification_keys = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(km.get_all_verification_keys())
            })?;

            for cached_key in &verification_keys {
                if let Ok(claims) = self.validate_with_key_bytes(token, &cached_key.key_material) {
                    return Ok(claims);
                }
            }
        }

        // Return the original static key error
        static_result
    }
}

impl PasetoAuth {
    /// Validate a token using the static key
    fn validate_with_static_key(&self, token: &str) -> Result<Claims, Error> {
        // Extract footer once (owned String) so it outlives the parser borrow
        let footer_owned = Footer::try_from_token(token).ok().flatten();

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
                if let Some(ref f) = footer_owned {
                    parser.set_footer(Footer::from(f.as_str()));
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
                if let Some(ref f) = footer_owned {
                    parser.set_footer(Footer::from(f.as_str()));
                }

                parser
                    .parse(token, &key)
                    .map_err(|e| Error::Paseto(format!("Invalid PASETO token: {}", e)))?
            }
        };

        Self::json_to_claims(json_value)
    }

    /// Validate a token using raw key bytes from the key rotation system
    #[cfg(feature = "auth")]
    fn validate_with_key_bytes(&self, token: &str, key_bytes: &[u8]) -> Result<Claims, Error> {
        let footer_owned = Footer::try_from_token(token).ok().flatten();

        let json_value = match self.inner.as_ref() {
            PasetoKey::V4Local {
                issuer, audience, ..
            } => {
                let key_arr: [u8; 32] = key_bytes.try_into().map_err(|_| {
                    Error::Internal(format!(
                        "rotated key material must be 32 bytes, got {}",
                        key_bytes.len()
                    ))
                })?;
                let key = PasetoSymmetricKey::<V4, Local>::from(Key::from(&key_arr));
                let mut parser = PasetoParser::<V4, Local>::default();

                if let Some(iss) = issuer {
                    parser.check_claim(IssuerClaim::from(iss.as_str()));
                }
                if let Some(aud) = audience {
                    parser.check_claim(AudienceClaim::from(aud.as_str()));
                }
                if let Some(ref f) = footer_owned {
                    parser.set_footer(Footer::from(f.as_str()));
                }

                parser
                    .parse(token, &key)
                    .map_err(|e| Error::Paseto(format!("Invalid PASETO token: {}", e)))?
            }
            PasetoKey::V4Public {
                issuer, audience, ..
            } => {
                let key_arr: [u8; 32] = key_bytes.try_into().map_err(|_| {
                    Error::Internal(format!(
                        "rotated key material must be 32 bytes, got {}",
                        key_bytes.len()
                    ))
                })?;
                let raw_key = Key::from(&key_arr);
                let key = PasetoAsymmetricPublicKey::<V4, Public>::from(&raw_key);
                let mut parser = PasetoParser::<V4, Public>::default();

                if let Some(iss) = issuer {
                    parser.check_claim(IssuerClaim::from(iss.as_str()));
                }
                if let Some(aud) = audience {
                    parser.check_claim(AudienceClaim::from(aud.as_str()));
                }
                if let Some(ref f) = footer_owned {
                    parser.set_footer(Footer::from(f.as_str()));
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
