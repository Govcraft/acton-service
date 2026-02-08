//! SurrealDB database connection management
//!
//! Supports runtime protocol selection via URL scheme:
//! - `ws://` / `wss://` - WebSocket connections
//! - `http://` / `https://` - HTTP connections
//! - `mem://` - In-memory database (for testing)

use std::time::Duration;

use crate::{config::SurrealDbConfig, error::Result};

/// SurrealDB client type alias using the `Any` engine for runtime protocol selection
pub type SurrealClient = surrealdb::Surreal<surrealdb::engine::any::Any>;

/// Create a SurrealDB client with retry logic
///
/// This is an internal function used by the SurrealDbAgent.
/// It will retry connection attempts based on the configuration.
pub(crate) async fn create_client(config: &SurrealDbConfig) -> Result<SurrealClient> {
    create_client_with_retries(config, config.max_retries).await
}

/// Create a SurrealDB client with configurable retries
///
/// Uses exponential backoff strategy for retries
async fn create_client_with_retries(
    config: &SurrealDbConfig,
    max_retries: u32,
) -> Result<SurrealClient> {
    let mut attempt = 0;
    let base_delay = Duration::from_secs(config.retry_delay_secs);

    loop {
        match try_create_client(config).await {
            Ok(client) => {
                if attempt > 0 {
                    tracing::info!(
                        "SurrealDB connection established after {} attempt(s)",
                        attempt + 1
                    );
                } else {
                    tracing::info!(
                        "SurrealDB connected: url={}, ns={}, db={}",
                        sanitize_connection_url(&config.url),
                        config.namespace,
                        config.database
                    );
                }
                return Ok(client);
            }
            Err(e) => {
                attempt += 1;

                if attempt > max_retries {
                    tracing::error!(
                        "Failed to connect to SurrealDB after {} attempts: {}",
                        max_retries + 1,
                        e
                    );
                    return Err(e);
                }

                // Calculate exponential backoff
                let delay_multiplier = 2_u32.pow(attempt.saturating_sub(1));
                let delay = base_delay * delay_multiplier;

                tracing::warn!(
                    "SurrealDB connection attempt {} failed: {}. Retrying in {:?}...",
                    attempt,
                    e,
                    delay
                );

                tokio::time::sleep(delay).await;
            }
        }
    }
}

/// Attempt to create a SurrealDB client (single try)
async fn try_create_client(config: &SurrealDbConfig) -> Result<SurrealClient> {
    let url_safe = sanitize_connection_url(&config.url);
    tracing::debug!("Connecting to SurrealDB: {}", url_safe);

    // Connect using the any engine (protocol determined by URL scheme)
    let client = surrealdb::engine::any::connect(&config.url).await.map_err(|e| {
        crate::error::Error::Internal(format!(
            "Failed to connect to SurrealDB at '{}': {}\n\n\
            Troubleshooting:\n\
            1. Verify the database URL is correct (e.g., ws://localhost:8000, mem://)\n\
            2. Check that the SurrealDB server is running and accessible\n\
            3. Verify network connectivity\n\n\
            Original error: {}",
            url_safe,
            categorize_surrealdb_error(&e),
            e
        ))
    })?;

    // Authenticate if credentials are provided
    if let (Some(username), Some(password)) = (&config.username, &config.password) {
        client
            .signin(surrealdb::opt::auth::Root {
                username,
                password,
            })
            .await
            .map_err(|e| {
                crate::error::Error::Internal(format!(
                    "Failed to authenticate with SurrealDB at '{}': {}\n\n\
                    Troubleshooting:\n\
                    1. Verify your username and password are correct\n\
                    2. Check that the user has appropriate permissions\n\n\
                    Original error: {}",
                    url_safe,
                    categorize_surrealdb_error(&e),
                    e
                ))
            })?;
    }

    // Select namespace and database
    client.use_ns(&config.namespace).use_db(&config.database).await.map_err(|e| {
        crate::error::Error::Internal(format!(
            "Failed to select namespace '{}' / database '{}' on SurrealDB at '{}': {}\n\n\
            Troubleshooting:\n\
            1. Verify the namespace and database names are correct\n\
            2. Check that you have permission to access them\n\n\
            Original error: {}",
            config.namespace,
            config.database,
            url_safe,
            categorize_surrealdb_error(&e),
            e
        ))
    })?;

    Ok(client)
}

/// Sanitize connection URL for safe logging (remove credentials if present)
///
/// Public alias used by health monitoring.
pub fn sanitize_url(url: &str) -> String {
    sanitize_connection_url(url)
}

/// Sanitize connection URL for safe logging (remove credentials if present)
fn sanitize_connection_url(url: &str) -> String {
    if let Some(at_pos) = url.find('@') {
        if let Some(scheme_end) = url.find("://") {
            let scheme = &url[..=scheme_end + 2];
            let after_at = &url[at_pos..];
            return format!("{}***{}", scheme, after_at);
        }
    }
    url.to_string()
}

/// Categorize SurrealDB error for better user guidance
fn categorize_surrealdb_error(err: &surrealdb::Error) -> &'static str {
    let err_str = err.to_string().to_lowercase();

    if err_str.contains("auth") || err_str.contains("credentials") || err_str.contains("signin") {
        "Authentication error - check your credentials"
    } else if err_str.contains("connect")
        || err_str.contains("network")
        || err_str.contains("dns")
        || err_str.contains("refused")
    {
        "Network connection error - check connectivity"
    } else if err_str.contains("permission") || err_str.contains("denied") || err_str.contains("not allowed") {
        "Permission error - check database permissions"
    } else if err_str.contains("not found") || err_str.contains("no such") {
        "Resource not found - check namespace/database exists"
    } else if err_str.contains("timeout") {
        "Connection timeout - database may be overloaded"
    } else {
        "Connection error"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_connection_url_no_credentials() {
        let url = "ws://localhost:8000";
        assert_eq!(sanitize_connection_url(url), url);
    }

    #[test]
    fn test_sanitize_connection_url_with_credentials() {
        let url = "ws://user:pass@localhost:8000";
        let sanitized = sanitize_connection_url(url);
        assert!(sanitized.contains("***"));
        assert!(sanitized.contains("localhost:8000"));
        assert!(!sanitized.contains("user"));
        assert!(!sanitized.contains("pass"));
    }

    #[test]
    fn test_sanitize_connection_url_mem() {
        let url = "mem://";
        assert_eq!(sanitize_connection_url(url), url);
    }

    #[tokio::test]
    async fn test_mem_connection() {
        let config = SurrealDbConfig {
            url: "mem://".to_string(),
            namespace: "test".to_string(),
            database: "test".to_string(),
            username: None,
            password: None,
            max_retries: 0,
            retry_delay_secs: 1,
            optional: false,
            lazy_init: false,
        };

        let result = create_client(&config).await;
        assert!(result.is_ok(), "Failed to connect to in-memory SurrealDB: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_mem_connection_with_auth() {
        let config = SurrealDbConfig {
            url: "mem://".to_string(),
            namespace: "test".to_string(),
            database: "test".to_string(),
            username: Some("root".to_string()),
            password: Some("root".to_string()),
            max_retries: 0,
            retry_delay_secs: 1,
            optional: false,
            lazy_init: false,
        };

        let result = create_client(&config).await;
        assert!(result.is_ok(), "Failed to connect to in-memory SurrealDB with auth: {:?}", result.err());
    }
}
