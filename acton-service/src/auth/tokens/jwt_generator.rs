//! JWT token generation (requires `jwt` feature)
//!
//! Generates JWT tokens for authentication. This complements the existing
//! `JwtAuth` validator.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};

use crate::auth::config::{JwtGenerationConfig, TokenGenerationConfig};
use crate::error::Error;
use crate::middleware::Claims;

use super::TokenGenerator;

/// JWT claims for encoding (internal use)
#[derive(Debug, Serialize, Deserialize)]
struct JwtClaims {
    sub: String,
    exp: i64,
    iat: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    jti: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    iss: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aud: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    roles: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    perms: Vec<String>,
}

/// JWT token generator
#[derive(Clone)]
pub struct JwtGenerator {
    encoding_key: Arc<EncodingKey>,
    algorithm: Algorithm,
    config: TokenGenerationConfig,
    issuer: Option<String>,
    audience: Option<String>,
}

impl JwtGenerator {
    /// Create a new JWT generator from configuration
    pub fn new(
        jwt_config: &JwtGenerationConfig,
        token_config: &TokenGenerationConfig,
    ) -> Result<Self, Error> {
        let key_bytes = fs::read(&jwt_config.private_key_path).map_err(|e| {
            Error::Config(Box::new(figment::Error::from(format!(
                "Failed to read JWT private key '{}': {}",
                jwt_config.private_key_path.display(),
                e
            ))))
        })?;

        let algorithm = parse_algorithm(&jwt_config.algorithm)?;
        let encoding_key = create_encoding_key(&key_bytes, algorithm)?;

        let issuer = jwt_config
            .issuer
            .clone()
            .or_else(|| token_config.issuer.clone());
        let audience = jwt_config
            .audience
            .clone()
            .or_else(|| token_config.audience.clone());

        Ok(Self {
            encoding_key: Arc::new(encoding_key),
            algorithm,
            config: token_config.clone(),
            issuer,
            audience,
        })
    }

    /// Set the issuer claim
    pub fn with_issuer(mut self, issuer: impl Into<String>) -> Self {
        self.issuer = Some(issuer.into());
        self
    }

    /// Set the audience claim
    pub fn with_audience(mut self, audience: impl Into<String>) -> Self {
        self.audience = Some(audience.into());
        self
    }

    fn generate_internal(&self, claims: &Claims, expires_in: Duration) -> Result<String, Error> {
        let now = Utc::now();
        let exp = now.timestamp() + expires_in.as_secs() as i64;

        let jti = if self.config.include_jti {
            Some(uuid::Uuid::new_v4().to_string())
        } else {
            claims.jti.clone()
        };

        let jwt_claims = JwtClaims {
            sub: claims.sub.clone(),
            exp,
            iat: now.timestamp(),
            jti,
            iss: self.issuer.clone().or_else(|| claims.iss.clone()),
            aud: self.audience.clone().or_else(|| claims.aud.clone()),
            email: claims.email.clone(),
            username: claims.username.clone(),
            roles: claims.roles.clone(),
            perms: claims.perms.clone(),
        };

        let header = Header::new(self.algorithm);
        encode(&header, &jwt_claims, &self.encoding_key).map_err(|e| Error::Jwt(Box::new(e)))
    }
}

impl TokenGenerator for JwtGenerator {
    fn generate_token(&self, claims: &Claims) -> Result<String, Error> {
        let expires_in = Duration::from_secs(self.config.access_token_lifetime_secs as u64);
        self.generate_internal(claims, expires_in)
    }

    fn generate_token_with_expiry(
        &self,
        claims: &Claims,
        expires_in: Duration,
    ) -> Result<String, Error> {
        self.generate_internal(claims, expires_in)
    }

    fn default_lifetime(&self) -> Duration {
        Duration::from_secs(self.config.access_token_lifetime_secs as u64)
    }
}

fn parse_algorithm(alg: &str) -> Result<Algorithm, Error> {
    match alg.to_uppercase().as_str() {
        "RS256" => Ok(Algorithm::RS256),
        "RS384" => Ok(Algorithm::RS384),
        "RS512" => Ok(Algorithm::RS512),
        "ES256" => Ok(Algorithm::ES256),
        "ES384" => Ok(Algorithm::ES384),
        "HS256" => Ok(Algorithm::HS256),
        "HS384" => Ok(Algorithm::HS384),
        "HS512" => Ok(Algorithm::HS512),
        _ => Err(Error::Config(Box::new(figment::Error::from(format!(
            "Unsupported JWT algorithm: {}",
            alg
        ))))),
    }
}

fn create_encoding_key(key_bytes: &[u8], algorithm: Algorithm) -> Result<EncodingKey, Error> {
    match algorithm {
        Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 => {
            EncodingKey::from_rsa_pem(key_bytes).map_err(|e| Error::Jwt(Box::new(e)))
        }
        Algorithm::ES256 | Algorithm::ES384 => {
            EncodingKey::from_ec_pem(key_bytes).map_err(|e| Error::Jwt(Box::new(e)))
        }
        Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512 => {
            Ok(EncodingKey::from_secret(key_bytes))
        }
        _ => Err(Error::Config(Box::new(figment::Error::from(
            "Unsupported algorithm for key creation",
        )))),
    }
}
