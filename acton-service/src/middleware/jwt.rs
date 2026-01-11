//! JWT authentication middleware (requires `jwt` feature)

use axum::{
    body::Body,
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use std::{fs, sync::Arc};

#[cfg(feature = "cache")]
use super::token::TokenRevocation;

use super::token::{extract_token, Claims, TokenValidator};
use crate::{config::JwtConfig, error::Error};

/// JWT authentication middleware state
#[derive(Clone)]
pub struct JwtAuth {
    decoding_key: Arc<DecodingKey>,
    validation: Validation,
    #[cfg(feature = "cache")]
    revocation: Option<Arc<dyn TokenRevocation>>,
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

    /// Set the token revocation checker
    ///
    /// This allows the middleware to check if tokens have been revoked.
    /// Typically used with `RedisTokenRevocation` from the revocation module.
    #[cfg(feature = "cache")]
    pub fn with_revocation<R: TokenRevocation + 'static>(mut self, revocation: R) -> Self {
        self.revocation = Some(Arc::new(revocation));
        self
    }

    /// Middleware function to validate JWT and inject claims
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
                tracing::warn!("JWT revocation is enabled but token has no JTI claim");
            }
        }

        // Inject claims into request extensions
        request.extensions_mut().insert(claims);

        Ok(next.run(request).await)
    }
}

impl TokenValidator for JwtAuth {
    fn validate_token(&self, token: &str) -> Result<Claims, Error> {
        let token_data = decode::<Claims>(token, &self.decoding_key, &self.validation)?;
        Ok(token_data.claims)
    }
}

// Note: Claims tests are in the token module since Claims is defined there.
// Integration tests for JWT validation would require generating test keys.
