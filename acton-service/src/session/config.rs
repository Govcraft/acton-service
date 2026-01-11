//! Session configuration types.

use serde::{Deserialize, Serialize};

/// Session storage backend type.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStorage {
    /// In-memory storage (development only, not persistent).
    #[default]
    Memory,
    /// Redis-backed storage (production, distributed).
    Redis,
}

/// Session configuration.
///
/// Configure session behavior including cookie settings, storage backend,
/// and expiration policies.
///
/// # Example
///
/// ```toml
/// [session]
/// cookie_name = "session_id"
/// expiry_secs = 86400
/// secure = true
/// http_only = true
/// same_site = "lax"
/// storage = "redis"
/// redis_url = "redis://localhost:6379"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Session cookie name.
    ///
    /// Default: `"session_id"`
    #[serde(default = "default_cookie_name")]
    pub cookie_name: String,

    /// Session expiry in seconds.
    ///
    /// - `0`: Session cookie (expires when browser closes)
    /// - `> 0`: Persistent cookie with specified lifetime
    ///
    /// Default: `86400` (24 hours)
    #[serde(default = "default_expiry_secs")]
    pub expiry_secs: u64,

    /// Session inactivity timeout in seconds (optional).
    ///
    /// If set, the session expires after this duration of inactivity.
    /// If not set, `expiry_secs` is used as an inactivity timeout.
    #[serde(default)]
    pub inactivity_timeout_secs: Option<u64>,

    /// Cookie path.
    ///
    /// Default: `"/"`
    #[serde(default = "default_cookie_path")]
    pub cookie_path: String,

    /// Cookie domain (optional).
    ///
    /// If not set, defaults to the request's domain.
    #[serde(default)]
    pub cookie_domain: Option<String>,

    /// Secure cookie flag (HTTPS only).
    ///
    /// Should be `true` in production. Set to `false` for local development
    /// without HTTPS.
    ///
    /// Default: `true`
    #[serde(default = "default_secure")]
    pub secure: bool,

    /// HttpOnly cookie flag.
    ///
    /// Prevents JavaScript access to the session cookie, mitigating XSS attacks.
    ///
    /// Default: `true`
    #[serde(default = "default_http_only")]
    pub http_only: bool,

    /// SameSite cookie policy.
    ///
    /// - `"strict"`: Cookie only sent in first-party context
    /// - `"lax"`: Cookie sent with top-level navigations and GET from third-party
    /// - `"none"`: Cookie sent in all contexts (requires `secure = true`)
    ///
    /// Default: `"lax"`
    #[serde(default = "default_same_site")]
    pub same_site: String,

    /// Session storage backend.
    ///
    /// - `"memory"`: In-memory storage (development)
    /// - `"redis"`: Redis storage (production)
    ///
    /// Default: `"memory"`
    #[serde(default)]
    pub storage: SessionStorage,

    /// Redis URL for session storage.
    ///
    /// Required when `storage = "redis"`.
    ///
    /// Example: `"redis://localhost:6379"` or `"redis://:password@host:6379/0"`
    #[serde(default)]
    pub redis_url: Option<String>,

    /// CSRF protection configuration.
    #[serde(default)]
    pub csrf: CsrfConfig,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            cookie_name: default_cookie_name(),
            expiry_secs: default_expiry_secs(),
            inactivity_timeout_secs: None,
            cookie_path: default_cookie_path(),
            cookie_domain: None,
            secure: default_secure(),
            http_only: default_http_only(),
            same_site: default_same_site(),
            storage: SessionStorage::default(),
            redis_url: None,
            csrf: CsrfConfig::default(),
        }
    }
}

/// CSRF protection configuration.
///
/// Controls how CSRF tokens are generated, stored, and validated.
///
/// # Example
///
/// ```toml
/// [session.csrf]
/// enabled = true
/// token_length = 32
/// header_name = "X-CSRF-Token"
/// form_field_name = "_csrf"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsrfConfig {
    /// Enable CSRF protection.
    ///
    /// When enabled, non-safe HTTP methods (POST, PUT, DELETE, PATCH) require
    /// a valid CSRF token.
    ///
    /// Default: `true`
    #[serde(default = "default_csrf_enabled")]
    pub enabled: bool,

    /// CSRF token length in bytes.
    ///
    /// Default: `32`
    #[serde(default = "default_token_length")]
    pub token_length: usize,

    /// HTTP header name for CSRF token.
    ///
    /// Default: `"X-CSRF-Token"`
    #[serde(default = "default_header_name")]
    pub header_name: String,

    /// Form field name for CSRF token.
    ///
    /// Default: `"_csrf"`
    #[serde(default = "default_form_field_name")]
    pub form_field_name: String,

    /// HTTP methods that skip CSRF validation (safe methods).
    ///
    /// Default: `["GET", "HEAD", "OPTIONS", "TRACE"]`
    #[serde(default = "default_safe_methods")]
    pub safe_methods: Vec<String>,
}

impl Default for CsrfConfig {
    fn default() -> Self {
        Self {
            enabled: default_csrf_enabled(),
            token_length: default_token_length(),
            header_name: default_header_name(),
            form_field_name: default_form_field_name(),
            safe_methods: default_safe_methods(),
        }
    }
}

// Default value functions
fn default_cookie_name() -> String {
    "session_id".to_string()
}

fn default_expiry_secs() -> u64 {
    86400 // 24 hours
}

fn default_cookie_path() -> String {
    "/".to_string()
}

fn default_secure() -> bool {
    true
}

fn default_http_only() -> bool {
    true
}

fn default_same_site() -> String {
    "lax".to_string()
}

fn default_csrf_enabled() -> bool {
    true
}

fn default_token_length() -> usize {
    32
}

fn default_header_name() -> String {
    "X-CSRF-Token".to_string()
}

fn default_form_field_name() -> String {
    "_csrf".to_string()
}

fn default_safe_methods() -> Vec<String> {
    vec![
        "GET".to_string(),
        "HEAD".to_string(),
        "OPTIONS".to_string(),
        "TRACE".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_config_defaults() {
        let config = SessionConfig::default();
        assert_eq!(config.cookie_name, "session_id");
        assert_eq!(config.expiry_secs, 86400);
        assert!(config.secure);
        assert!(config.http_only);
        assert_eq!(config.same_site, "lax");
        assert_eq!(config.storage, SessionStorage::Memory);
    }

    #[test]
    fn test_csrf_config_defaults() {
        let config = CsrfConfig::default();
        assert!(config.enabled);
        assert_eq!(config.token_length, 32);
        assert_eq!(config.header_name, "X-CSRF-Token");
        assert_eq!(config.form_field_name, "_csrf");
        assert!(config.safe_methods.contains(&"GET".to_string()));
    }

    #[test]
    fn test_session_storage_serialization() {
        let memory = SessionStorage::Memory;
        let redis = SessionStorage::Redis;

        assert_eq!(serde_json::to_string(&memory).unwrap(), "\"memory\"");
        assert_eq!(serde_json::to_string(&redis).unwrap(), "\"redis\"");
    }
}
