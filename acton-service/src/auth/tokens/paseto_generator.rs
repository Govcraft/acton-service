//! PASETO token generation
//!
//! Generates PASETO V4 tokens (local or public) for authentication.
//! This complements the existing `PasetoAuth` validator.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use rusty_paseto::prelude::*;
use serde_json::json;

use crate::auth::config::{PasetoGenerationConfig, TokenGenerationConfig};
use crate::error::Error;
use crate::middleware::Claims;

use super::TokenGenerator;

/// PASETO key for token generation
enum PasetoGeneratorKey {
    /// V4 Local (symmetric encryption)
    V4Local { key_bytes: [u8; 32] },
    /// V4 Public (Ed25519 signing)
    V4Public { private_key_bytes: [u8; 64] },
}

/// PASETO token generator
///
/// Generates PASETO V4 tokens using either symmetric (local) or
/// asymmetric (public) cryptography.
#[derive(Clone)]
pub struct PasetoGenerator {
    key: Arc<PasetoGeneratorKey>,
    config: TokenGenerationConfig,
    issuer: Option<String>,
    audience: Option<String>,
}

impl PasetoGenerator {
    /// Create a new PASETO generator from configuration
    ///
    /// # Arguments
    ///
    /// * `paseto_config` - PASETO-specific configuration
    /// * `token_config` - General token generation configuration
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let generator = PasetoGenerator::new(&paseto_config, &token_config)?;
    /// ```
    pub fn new(
        paseto_config: &PasetoGenerationConfig,
        token_config: &TokenGenerationConfig,
    ) -> Result<Self, Error> {
        let key_bytes = fs::read(&paseto_config.key_path).map_err(|e| {
            Error::Config(Box::new(figment::Error::from(format!(
                "Failed to read PASETO key file '{}': {}",
                paseto_config.key_path.display(),
                e
            ))))
        })?;

        let key = match (
            paseto_config.version.as_str(),
            paseto_config.purpose.as_str(),
        ) {
            ("v4", "local") => {
                if key_bytes.len() != 32 {
                    return Err(Error::Config(Box::new(figment::Error::from(format!(
                        "V4 local key must be exactly 32 bytes, got {} bytes. \
                        Generate with: head -c 32 /dev/urandom > {}",
                        key_bytes.len(),
                        paseto_config.key_path.display()
                    )))));
                }
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&key_bytes);
                PasetoGeneratorKey::V4Local { key_bytes: arr }
            }
            ("v4", "public") => {
                if key_bytes.len() != 64 {
                    return Err(Error::Config(Box::new(figment::Error::from(format!(
                        "V4 public (private) key must be exactly 64 bytes (Ed25519 secret key), got {} bytes. \
                        See documentation for key generation instructions.",
                        key_bytes.len()
                    )))));
                }
                let mut arr = [0u8; 64];
                arr.copy_from_slice(&key_bytes);
                PasetoGeneratorKey::V4Public {
                    private_key_bytes: arr,
                }
            }
            (version, purpose) => {
                return Err(Error::Config(Box::new(figment::Error::from(format!(
                    "Unsupported PASETO version/purpose: {}/{}. Only v4/local and v4/public are supported.",
                    version, purpose
                )))));
            }
        };

        // Use PASETO-specific issuer/audience if set, otherwise fall back to token config
        let issuer = paseto_config
            .issuer
            .clone()
            .or_else(|| token_config.issuer.clone());
        let audience = paseto_config
            .audience
            .clone()
            .or_else(|| token_config.audience.clone());

        Ok(Self {
            key: Arc::new(key),
            config: token_config.clone(),
            issuer,
            audience,
        })
    }

    /// Create a generator with a symmetric key for V4 local tokens
    ///
    /// # Arguments
    ///
    /// * `key` - 32-byte symmetric key
    /// * `config` - Token generation configuration
    pub fn with_symmetric_key(key: [u8; 32], config: TokenGenerationConfig) -> Self {
        Self {
            key: Arc::new(PasetoGeneratorKey::V4Local { key_bytes: key }),
            config,
            issuer: None,
            audience: None,
        }
    }

    /// Create a generator with an Ed25519 private key for V4 public tokens
    ///
    /// # Arguments
    ///
    /// * `private_key` - 64-byte Ed25519 private key
    /// * `config` - Token generation configuration
    pub fn with_private_key(private_key: [u8; 64], config: TokenGenerationConfig) -> Self {
        Self {
            key: Arc::new(PasetoGeneratorKey::V4Public {
                private_key_bytes: private_key,
            }),
            config,
            issuer: None,
            audience: None,
        }
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

    /// Generate a token with specific claims and expiration
    fn generate_internal(&self, claims: &Claims, expires_in: Duration) -> Result<String, Error> {
        let now = Utc::now();
        let exp = now + chrono::Duration::seconds(expires_in.as_secs() as i64);

        // Generate JTI if configured
        let jti = if self.config.include_jti {
            Some(uuid::Uuid::new_v4().to_string())
        } else {
            claims.jti.clone()
        };

        // Build the payload
        let mut payload = json!({
            "sub": claims.sub,
            "exp": exp.to_rfc3339(),
            "iat": now.to_rfc3339(),
        });

        // Add optional claims
        if let Some(ref jti) = jti {
            payload["jti"] = json!(jti);
        }
        if let Some(ref email) = claims.email {
            payload["email"] = json!(email);
        }
        if let Some(ref username) = claims.username {
            payload["username"] = json!(username);
        }
        if !claims.roles.is_empty() {
            payload["roles"] = json!(claims.roles);
        }
        if !claims.perms.is_empty() {
            payload["perms"] = json!(claims.perms);
        }

        // Use config issuer/audience, or claims issuer/audience
        let issuer = self.issuer.as_ref().or(claims.iss.as_ref());
        let audience = self.audience.as_ref().or(claims.aud.as_ref());

        if let Some(ref iss) = issuer {
            payload["iss"] = json!(iss);
        }
        if let Some(ref aud) = audience {
            payload["aud"] = json!(aud);
        }

        // Generate the token based on key type
        match self.key.as_ref() {
            PasetoGeneratorKey::V4Local { key_bytes } => {
                self.generate_v4_local(key_bytes, &payload)
            }
            PasetoGeneratorKey::V4Public { private_key_bytes } => {
                self.generate_v4_public(private_key_bytes, &payload)
            }
        }
    }

    fn generate_v4_local(
        &self,
        key_bytes: &[u8; 32],
        payload: &serde_json::Value,
    ) -> Result<String, Error> {
        let key = PasetoSymmetricKey::<V4, Local>::from(Key::from(key_bytes));

        // Build claims from payload using mutable reference pattern
        let mut builder = PasetoBuilder::<V4, Local>::default();

        // Add standard claims
        if let Some(sub) = payload.get("sub").and_then(|v| v.as_str()) {
            builder.set_claim(SubjectClaim::from(sub));
        }
        if let Some(exp) = payload.get("exp").and_then(|v| v.as_str()) {
            let claim = ExpirationClaim::try_from(exp)
                .map_err(|e| Error::Paseto(format!("Invalid expiration: {}", e)))?;
            builder.set_claim(claim);
        }
        if let Some(iat) = payload.get("iat").and_then(|v| v.as_str()) {
            let claim = IssuedAtClaim::try_from(iat)
                .map_err(|e| Error::Paseto(format!("Invalid issued at: {}", e)))?;
            builder.set_claim(claim);
        }
        if let Some(jti) = payload.get("jti").and_then(|v| v.as_str()) {
            builder.set_claim(TokenIdentifierClaim::from(jti));
        }
        if let Some(iss) = payload.get("iss").and_then(|v| v.as_str()) {
            builder.set_claim(IssuerClaim::from(iss));
        }
        if let Some(aud) = payload.get("aud").and_then(|v| v.as_str()) {
            builder.set_claim(AudienceClaim::from(aud));
        }

        // Add custom claims
        if let Some(email) = payload.get("email").and_then(|v| v.as_str()) {
            let claim = CustomClaim::try_from(("email", email))
                .map_err(|e| Error::Paseto(format!("Invalid email claim: {}", e)))?;
            builder.set_claim(claim);
        }
        if let Some(username) = payload.get("username").and_then(|v| v.as_str()) {
            let claim = CustomClaim::try_from(("username", username))
                .map_err(|e| Error::Paseto(format!("Invalid username claim: {}", e)))?;
            builder.set_claim(claim);
        }
        if let Some(roles) = payload.get("roles") {
            let claim = CustomClaim::try_from(("roles", roles.clone()))
                .map_err(|e| Error::Paseto(format!("Invalid roles claim: {}", e)))?;
            builder.set_claim(claim);
        }
        if let Some(perms) = payload.get("perms") {
            let claim = CustomClaim::try_from(("perms", perms.clone()))
                .map_err(|e| Error::Paseto(format!("Invalid perms claim: {}", e)))?;
            builder.set_claim(claim);
        }

        builder
            .build(&key)
            .map_err(|e| Error::Paseto(format!("Failed to build PASETO token: {}", e)))
    }

    fn generate_v4_public(
        &self,
        private_key_bytes: &[u8; 64],
        payload: &serde_json::Value,
    ) -> Result<String, Error> {
        let key = PasetoAsymmetricPrivateKey::<V4, Public>::from(private_key_bytes.as_slice());

        // Build claims from payload using mutable reference pattern
        let mut builder = PasetoBuilder::<V4, Public>::default();

        // Add standard claims
        if let Some(sub) = payload.get("sub").and_then(|v| v.as_str()) {
            builder.set_claim(SubjectClaim::from(sub));
        }
        if let Some(exp) = payload.get("exp").and_then(|v| v.as_str()) {
            let claim = ExpirationClaim::try_from(exp)
                .map_err(|e| Error::Paseto(format!("Invalid expiration: {}", e)))?;
            builder.set_claim(claim);
        }
        if let Some(iat) = payload.get("iat").and_then(|v| v.as_str()) {
            let claim = IssuedAtClaim::try_from(iat)
                .map_err(|e| Error::Paseto(format!("Invalid issued at: {}", e)))?;
            builder.set_claim(claim);
        }
        if let Some(jti) = payload.get("jti").and_then(|v| v.as_str()) {
            builder.set_claim(TokenIdentifierClaim::from(jti));
        }
        if let Some(iss) = payload.get("iss").and_then(|v| v.as_str()) {
            builder.set_claim(IssuerClaim::from(iss));
        }
        if let Some(aud) = payload.get("aud").and_then(|v| v.as_str()) {
            builder.set_claim(AudienceClaim::from(aud));
        }

        // Add custom claims
        if let Some(email) = payload.get("email").and_then(|v| v.as_str()) {
            let claim = CustomClaim::try_from(("email", email))
                .map_err(|e| Error::Paseto(format!("Invalid email claim: {}", e)))?;
            builder.set_claim(claim);
        }
        if let Some(username) = payload.get("username").and_then(|v| v.as_str()) {
            let claim = CustomClaim::try_from(("username", username))
                .map_err(|e| Error::Paseto(format!("Invalid username claim: {}", e)))?;
            builder.set_claim(claim);
        }
        if let Some(roles) = payload.get("roles") {
            let claim = CustomClaim::try_from(("roles", roles.clone()))
                .map_err(|e| Error::Paseto(format!("Invalid roles claim: {}", e)))?;
            builder.set_claim(claim);
        }
        if let Some(perms) = payload.get("perms") {
            let claim = CustomClaim::try_from(("perms", perms.clone()))
                .map_err(|e| Error::Paseto(format!("Invalid perms claim: {}", e)))?;
            builder.set_claim(claim);
        }

        builder
            .build(&key)
            .map_err(|e| Error::Paseto(format!("Failed to build PASETO token: {}", e)))
    }
}

impl TokenGenerator for PasetoGenerator {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PasetoConfig;
    use crate::middleware::paseto::PasetoAuth;
    use crate::middleware::TokenValidator;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_key_file() -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        // 32 random bytes for V4 local
        let key: [u8; 32] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c,
            0x1d, 0x1e, 0x1f, 0x20,
        ];
        file.write_all(&key).unwrap();
        file
    }

    #[test]
    fn test_generate_v4_local_token() {
        let key: [u8; 32] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c,
            0x1d, 0x1e, 0x1f, 0x20,
        ];

        let config = TokenGenerationConfig::default();
        let generator = PasetoGenerator::with_symmetric_key(key, config);

        let claims = Claims {
            sub: "user:123".to_string(),
            email: Some("test@example.com".to_string()),
            username: Some("testuser".to_string()),
            roles: vec!["user".to_string(), "admin".to_string()],
            perms: vec!["read:docs".to_string()],
            exp: 0,
            iat: None,
            jti: None,
            iss: None,
            aud: None,
        };

        let token = generator.generate_token(&claims).unwrap();
        assert!(token.starts_with("v4.local."));
    }

    #[test]
    fn test_generate_and_validate_round_trip() {
        let key_file = create_test_key_file();

        // Create generator
        let paseto_gen_config = PasetoGenerationConfig {
            version: "v4".to_string(),
            purpose: "local".to_string(),
            key_path: key_file.path().to_path_buf(),
            issuer: Some("test-issuer".to_string()),
            audience: Some("test-audience".to_string()),
        };
        let token_config = TokenGenerationConfig::default();
        let generator = PasetoGenerator::new(&paseto_gen_config, &token_config).unwrap();

        // Create validator (using the same key)
        let paseto_config = PasetoConfig {
            version: "v4".to_string(),
            purpose: "local".to_string(),
            key_path: key_file.path().to_path_buf(),
            issuer: Some("test-issuer".to_string()),
            audience: Some("test-audience".to_string()),
        };
        let validator = PasetoAuth::new(&paseto_config).unwrap();

        // Generate a token
        let claims = Claims {
            sub: "user:456".to_string(),
            email: Some("user@example.com".to_string()),
            username: None,
            roles: vec!["user".to_string()],
            perms: vec![],
            exp: 0,
            iat: None,
            jti: None,
            iss: None,
            aud: None,
        };

        let token = generator.generate_token(&claims).unwrap();

        // Validate the token
        let validated_claims = validator.validate_token(&token).unwrap();

        assert_eq!(validated_claims.sub, "user:456");
        assert_eq!(validated_claims.email, Some("user@example.com".to_string()));
        assert_eq!(validated_claims.roles, vec!["user"]);
        assert!(validated_claims.jti.is_some()); // JTI should be generated
    }

    #[test]
    fn test_custom_expiry() {
        let key: [u8; 32] = [0x42; 32];
        let config = TokenGenerationConfig::default();
        let generator = PasetoGenerator::with_symmetric_key(key, config);

        let claims = Claims {
            sub: "user:789".to_string(),
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

        // Generate with 1 hour expiry
        let token = generator
            .generate_token_with_expiry(&claims, Duration::from_secs(3600))
            .unwrap();

        assert!(token.starts_with("v4.local."));
    }

    #[test]
    fn test_issuer_and_audience() {
        let key: [u8; 32] = [0x42; 32];
        let config = TokenGenerationConfig::default();
        let generator = PasetoGenerator::with_symmetric_key(key, config)
            .with_issuer("my-auth-service")
            .with_audience("my-api");

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
        };

        let token = generator.generate_token(&claims).unwrap();
        assert!(token.starts_with("v4.local."));
    }
}
