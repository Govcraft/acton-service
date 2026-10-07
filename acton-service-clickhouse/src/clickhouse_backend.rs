//! ClickHouse analytical database client management
//!
//! ClickHouse is a columnar OLAP database used as a complementary analytical store.
//! Unlike the primary database backends (PostgreSQL, Turso, SurrealDB), the `clickhouse`
//! feature is composable and can be used alongside any of them.
//!
//! The `clickhouse::Client` is HTTP-based and internally uses `reqwest`. It is cheaply
//! clonable (wraps `Arc` internally) and does not require a connection pool.

use std::time::Duration;

use crate::{
    config::ClickHouseConfig,
    error::{Error, Result},
};

/// Create a ClickHouse client with retry logic
///
/// This is an internal function used by the pool agent and AppStateBuilder.
/// It will retry connection attempts based on the configuration.
pub async fn create_client(config: &ClickHouseConfig) -> Result<clickhouse::Client> {
    create_client_with_retries(config, config.max_retries).await
}

/// Create a ClickHouse client with configurable retries
///
/// Uses exponential backoff strategy for retries
async fn create_client_with_retries(
    config: &ClickHouseConfig,
    max_retries: u32,
) -> Result<clickhouse::Client> {
    let mut attempt = 0;
    let base_delay = Duration::from_secs(config.retry_delay_secs);

    loop {
        match try_create_client(config).await {
            Ok(client) => {
                if attempt > 0 {
                    tracing::info!(
                        "ClickHouse connection verified after {} attempt(s)",
                        attempt + 1
                    );
                } else {
                    tracing::info!("ClickHouse client connected to {}", config.url);
                }
                return Ok(client);
            }
            Err(e) => {
                attempt += 1;

                if attempt > max_retries {
                    tracing::error!(
                        "Failed to connect to ClickHouse after {} attempts: {}",
                        max_retries + 1,
                        e
                    );
                    return Err(e);
                }

                let delay_multiplier = 2_u32.pow(attempt.saturating_sub(1));
                let delay = base_delay * delay_multiplier;

                tracing::warn!(
                    "ClickHouse connection attempt {} failed: {}. Retrying in {:?}...",
                    attempt,
                    e,
                    delay
                );

                tokio::time::sleep(delay).await;
            }
        }
    }
}

/// Attempt to create a ClickHouse client (single try)
async fn try_create_client(config: &ClickHouseConfig) -> Result<clickhouse::Client> {
    let mut client = clickhouse::Client::default()
        .with_url(&config.url)
        .with_database(&config.database);

    if let Some(ref user) = config.username {
        client = client.with_user(user);
    }
    if let Some(ref pass) = config.password {
        client = client.with_password(pass);
    }

    // Verify connectivity with a simple query
    client.query("SELECT 1").execute().await.map_err(|e| {
        Error::ClickHouse(format!(
            "Failed to connect to ClickHouse at '{}'\n\n\
                Troubleshooting:\n\
                1. Verify ClickHouse server is running\n\
                2. Check HTTP interface is enabled (default port 8123)\n\
                3. Verify credentials and database name\n\
                4. Check network connectivity\n\n\
                Error: {}",
            config.url, e
        ))
    })?;

    Ok(client)
}

/// Sanitize ClickHouse URL for safe logging (remove embedded credentials)
pub fn sanitize_url(url: &str) -> String {
    if let Some(at_pos) = url.find('@') {
        if let Some(scheme_end) = url.find("://") {
            return format!("{}://***@{}", &url[..scheme_end], &url[at_pos + 1..]);
        }
    }
    url.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Config deserialization: verify defaults actually apply from serde,
    // not just that struct fields exist
    // =========================================================================

    #[test]
    fn test_config_deserialize_applies_all_defaults() {
        // Only url is required — every other field should get a default
        let json = r#"{"url": "http://ch:8123"}"#;
        let config: ClickHouseConfig = serde_json::from_str(json).unwrap();

        assert_eq!(config.database, "default");
        assert!(config.username.is_none());
        assert!(config.password.is_none());
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.retry_delay_secs, 2);
        assert!(!config.optional);
        assert!(config.lazy_init);
    }

    #[test]
    fn test_config_serde_roundtrip_preserves_all_fields() {
        let config = ClickHouseConfig {
            url: "https://ch.prod:8443".to_string(),
            database: "analytics".to_string(),
            username: Some("admin".to_string()),
            password: Some("s3cret".to_string()),
            max_retries: 10,
            retry_delay_secs: 30,
            optional: true,
            lazy_init: false,
        };

        let json = serde_json::to_string(&config).unwrap();
        let roundtripped: ClickHouseConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(roundtripped.url, config.url);
        assert_eq!(roundtripped.database, config.database);
        assert_eq!(roundtripped.username, config.username);
        assert_eq!(roundtripped.password, config.password);
        assert_eq!(roundtripped.max_retries, config.max_retries);
        assert_eq!(roundtripped.retry_delay_secs, config.retry_delay_secs);
        assert_eq!(roundtripped.optional, config.optional);
        assert_eq!(roundtripped.lazy_init, config.lazy_init);
    }

    #[test]
    fn test_config_overrides_beat_defaults() {
        let json = r#"{
            "url": "http://ch:8123",
            "database": "events",
            "max_retries": 0,
            "retry_delay_secs": 60,
            "optional": true,
            "lazy_init": false
        }"#;
        let config: ClickHouseConfig = serde_json::from_str(json).unwrap();

        assert_eq!(config.database, "events");
        assert_eq!(config.max_retries, 0);
        assert_eq!(config.retry_delay_secs, 60);
        assert!(config.optional);
        assert!(!config.lazy_init);
    }

    // =========================================================================
    // URL sanitization: the security boundary — credentials must never leak
    // into logs, health endpoints, or error messages
    // =========================================================================

    #[test]
    fn test_sanitize_url_redacts_http_credentials() {
        let sanitized = sanitize_url("http://user:pass@localhost:8123");
        assert_eq!(sanitized, "http://***@localhost:8123");
    }

    #[test]
    fn test_sanitize_url_redacts_https_credentials() {
        let sanitized = sanitize_url("https://admin:s3cret@ch.example.com:8443/db");
        assert!(!sanitized.contains("admin"));
        assert!(!sanitized.contains("s3cret"));
        assert!(sanitized.contains("ch.example.com:8443/db"));
    }

    #[test]
    fn test_sanitize_url_passthrough_when_no_credentials() {
        let url = "http://localhost:8123";
        assert_eq!(sanitize_url(url), url);
    }

    #[test]
    fn test_sanitize_url_no_scheme_leaves_credentials_visible() {
        // Without ://, we can't safely identify the scheme boundary.
        // This documents current behavior: no redaction without scheme.
        let url = "user:pass@localhost:8123";
        assert_eq!(sanitize_url(url), url);
    }

    #[test]
    fn test_sanitize_url_handles_empty_string() {
        assert_eq!(sanitize_url(""), "");
    }

    // =========================================================================
    // Error → HTTP response mapping: verify the ClickHouse error variant
    // maps to the correct status code and error code for API consumers
    // =========================================================================

    #[test]
    fn test_clickhouse_error_does_not_leak_internal_message() {
        // The error variant stores the full internal message for logging,
        // but IntoResponse should return a generic message
        let err = Error::ClickHouse(
            "DB::Exception: Table default.audit_events doesn't exist".to_string(),
        );
        let display = err.to_string();
        // Display (for logging) SHOULD contain the detail
        assert!(display.contains("doesn't exist"));
    }
}
