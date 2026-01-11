//! Authentication configuration structures
//!
//! Configuration for password hashing, token generation, API keys, and OAuth.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[cfg(feature = "oauth")]
use std::collections::HashMap;

/// Main authentication configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Password hashing configuration
    #[serde(default)]
    pub password: PasswordConfig,

    /// Token generation configuration
    #[serde(default)]
    pub tokens: TokenGenerationConfig,

    /// PASETO-specific generation config (default token format)
    #[serde(default)]
    pub paseto: Option<PasetoGenerationConfig>,

    /// JWT-specific generation config (requires jwt feature)
    #[cfg(feature = "jwt")]
    #[serde(default)]
    pub jwt: Option<JwtGenerationConfig>,

    /// Refresh token configuration
    #[serde(default)]
    pub refresh_tokens: RefreshTokenConfig,

    /// API key configuration
    #[serde(default)]
    pub api_keys: Option<ApiKeyConfig>,

    /// OAuth providers configuration (requires oauth feature)
    #[cfg(feature = "oauth")]
    #[serde(default)]
    pub oauth: Option<OAuthConfig>,
}

/// Password hashing configuration following OWASP guidelines
///
/// Default values are based on OWASP recommendations for Argon2id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordConfig {
    /// Memory cost in KiB (default: 65536 = 64 MiB)
    #[serde(default = "default_memory_cost")]
    pub memory_cost_kib: u32,

    /// Time cost / iterations (default: 3)
    #[serde(default = "default_time_cost")]
    pub time_cost: u32,

    /// Parallelism degree (default: 4)
    #[serde(default = "default_parallelism")]
    pub parallelism: u32,

    /// Minimum password length (default: 8)
    #[serde(default = "default_min_length")]
    pub min_password_length: usize,
}

impl Default for PasswordConfig {
    fn default() -> Self {
        Self {
            memory_cost_kib: default_memory_cost(),
            time_cost: default_time_cost(),
            parallelism: default_parallelism(),
            min_password_length: default_min_length(),
        }
    }
}

/// Token generation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenGenerationConfig {
    /// Access token lifetime in seconds (default: 900 = 15 min)
    #[serde(default = "default_access_token_lifetime")]
    pub access_token_lifetime_secs: i64,

    /// Issuer claim
    #[serde(default)]
    pub issuer: Option<String>,

    /// Audience claim (optional)
    #[serde(default)]
    pub audience: Option<String>,

    /// Include jti (token ID) for revocation support (default: true)
    #[serde(default = "default_true")]
    pub include_jti: bool,
}

impl Default for TokenGenerationConfig {
    fn default() -> Self {
        Self {
            access_token_lifetime_secs: default_access_token_lifetime(),
            issuer: None,
            audience: None,
            include_jti: true,
        }
    }
}

/// PASETO token generation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasetoGenerationConfig {
    /// PASETO version (default: "v4")
    #[serde(default = "default_paseto_version")]
    pub version: String,

    /// Token purpose: "local" (symmetric) or "public" (asymmetric)
    /// Default: "local"
    #[serde(default = "default_paseto_purpose")]
    pub purpose: String,

    /// Path to key file
    /// - local: 32-byte symmetric key
    /// - public: 64-byte Ed25519 private key (for generation)
    pub key_path: PathBuf,

    /// Issuer claim (overrides tokens.issuer if set)
    #[serde(default)]
    pub issuer: Option<String>,

    /// Audience claim (overrides tokens.audience if set)
    #[serde(default)]
    pub audience: Option<String>,
}

/// JWT token generation configuration (requires jwt feature)
#[cfg(feature = "jwt")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtGenerationConfig {
    /// Path to private key file (for signing)
    pub private_key_path: PathBuf,

    /// JWT algorithm (RS256, RS384, RS512, ES256, ES384, HS256, HS384, HS512)
    pub algorithm: String,

    /// Issuer claim (overrides tokens.issuer if set)
    #[serde(default)]
    pub issuer: Option<String>,

    /// Audience claim (overrides tokens.audience if set)
    #[serde(default)]
    pub audience: Option<String>,
}

/// Refresh token configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshTokenConfig {
    /// Enable refresh tokens (default: true)
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Refresh token lifetime in seconds (default: 604800 = 7 days)
    #[serde(default = "default_refresh_lifetime")]
    pub lifetime_secs: i64,

    /// Enable token rotation on refresh (default: true)
    #[serde(default = "default_true")]
    pub rotate_on_refresh: bool,

    /// Detect reuse of rotated tokens (security feature, default: true)
    #[serde(default = "default_true")]
    pub detect_reuse: bool,

    /// Storage backend: "redis", "postgres", or "turso"
    #[serde(default = "default_storage_backend")]
    pub storage: String,
}

impl Default for RefreshTokenConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            lifetime_secs: default_refresh_lifetime(),
            rotate_on_refresh: true,
            detect_reuse: true,
            storage: default_storage_backend(),
        }
    }
}

/// API key configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyConfig {
    /// Enable API key authentication (default: true)
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Key prefix (e.g., "sk_live", "acton")
    #[serde(default = "default_api_key_prefix")]
    pub prefix: String,

    /// Header name for API key (default: "X-API-Key")
    #[serde(default = "default_api_key_header")]
    pub header: String,

    /// Default rate limit per key (requests/minute)
    #[serde(default)]
    pub default_rate_limit: Option<u32>,

    /// Storage backend: "redis", "postgres", or "turso"
    #[serde(default = "default_storage_backend")]
    pub storage: String,
}

impl Default for ApiKeyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            prefix: default_api_key_prefix(),
            header: default_api_key_header(),
            default_rate_limit: None,
            storage: default_storage_backend(),
        }
    }
}

/// OAuth configuration (requires oauth feature)
#[cfg(feature = "oauth")]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OAuthConfig {
    /// Enable OAuth (default: true)
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// OAuth state TTL in seconds (default: 600 = 10 min)
    #[serde(default = "default_oauth_state_ttl")]
    pub state_ttl_secs: u64,

    /// Configured providers
    #[serde(default)]
    pub providers: HashMap<String, OAuthProviderConfig>,
}

/// Individual OAuth provider configuration
#[cfg(feature = "oauth")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthProviderConfig {
    /// OAuth client ID
    pub client_id: String,

    /// OAuth client secret
    pub client_secret: String,

    /// Redirect URI after authentication
    pub redirect_uri: String,

    /// OAuth scopes to request
    #[serde(default)]
    pub scopes: Vec<String>,

    /// Authorization endpoint (for custom OIDC providers)
    #[serde(default)]
    pub authorization_endpoint: Option<String>,

    /// Token endpoint (for custom OIDC providers)
    #[serde(default)]
    pub token_endpoint: Option<String>,

    /// Userinfo endpoint (for custom OIDC providers)
    #[serde(default)]
    pub userinfo_endpoint: Option<String>,
}

// Default value functions

fn default_memory_cost() -> u32 {
    65536 // 64 MiB
}

fn default_time_cost() -> u32 {
    3
}

fn default_parallelism() -> u32 {
    4
}

fn default_min_length() -> usize {
    8
}

fn default_access_token_lifetime() -> i64 {
    900 // 15 minutes
}

fn default_refresh_lifetime() -> i64 {
    604800 // 7 days
}

fn default_true() -> bool {
    true
}

fn default_storage_backend() -> String {
    "redis".to_string()
}

fn default_paseto_version() -> String {
    "v4".to_string()
}

fn default_paseto_purpose() -> String {
    "local".to_string()
}

fn default_api_key_prefix() -> String {
    "sk_live".to_string()
}

fn default_api_key_header() -> String {
    "X-API-Key".to_string()
}

#[cfg(feature = "oauth")]
fn default_oauth_state_ttl() -> u64 {
    600 // 10 minutes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_password_config_defaults() {
        let config = PasswordConfig::default();
        assert_eq!(config.memory_cost_kib, 65536);
        assert_eq!(config.time_cost, 3);
        assert_eq!(config.parallelism, 4);
        assert_eq!(config.min_password_length, 8);
    }

    #[test]
    fn test_token_config_defaults() {
        let config = TokenGenerationConfig::default();
        assert_eq!(config.access_token_lifetime_secs, 900);
        assert!(config.include_jti);
    }

    #[test]
    fn test_refresh_token_config_defaults() {
        let config = RefreshTokenConfig::default();
        assert!(config.enabled);
        assert_eq!(config.lifetime_secs, 604800);
        assert!(config.rotate_on_refresh);
        assert!(config.detect_reuse);
        assert_eq!(config.storage, "redis");
    }

    #[test]
    fn test_api_key_config_defaults() {
        let config = ApiKeyConfig::default();
        assert!(config.enabled);
        assert_eq!(config.prefix, "sk_live");
        assert_eq!(config.header, "X-API-Key");
    }
}
