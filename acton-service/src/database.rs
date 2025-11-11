//! Database connection pool management

#[cfg(feature = "database")]
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::time::Duration;

use crate::{config::DatabaseConfig, error::Result};

/// Create a PostgreSQL connection pool with retry logic
///
/// This is an internal function used by AppStateBuilder.
/// It will retry connection attempts based on the configuration.
#[cfg(feature = "database")]
pub(crate) async fn create_pool(config: &DatabaseConfig) -> Result<PgPool> {
    create_pool_with_retries(config, config.max_retries).await
}

/// Create a PostgreSQL connection pool with configurable retries
///
/// Uses exponential backoff strategy for retries
#[cfg(feature = "database")]
async fn create_pool_with_retries(config: &DatabaseConfig, max_retries: u32) -> Result<PgPool> {
    let mut attempt = 0;
    let base_delay = Duration::from_secs(config.retry_delay_secs);

    loop {
        match try_create_pool(config).await {
            Ok(pool) => {
                if attempt > 0 {
                    tracing::info!(
                        "Database connection established after {} attempt(s)",
                        attempt + 1
                    );
                } else {
                    tracing::info!(
                        "Database connection pool created: max={}, min={}",
                        config.max_connections,
                        config.min_connections
                    );
                }
                return Ok(pool);
            }
            Err(e) => {
                attempt += 1;

                if attempt > max_retries {
                    tracing::error!(
                        "Failed to connect to database after {} attempts: {}",
                        max_retries + 1,
                        e
                    );
                    return Err(e);
                }

                // Calculate exponential backoff
                let delay_multiplier = 2_u32.pow(attempt.saturating_sub(1));
                let delay = base_delay * delay_multiplier;

                tracing::warn!(
                    "Database connection attempt {} failed: {}. Retrying in {:?}...",
                    attempt,
                    e,
                    delay
                );

                tokio::time::sleep(delay).await;
            }
        }
    }
}

/// Attempt to create a database pool (single try)
#[cfg(feature = "database")]
async fn try_create_pool(config: &DatabaseConfig) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .acquire_timeout(Duration::from_secs(config.connection_timeout_secs))
        .connect(&config.url)
        .await
        .map_err(|e| {
            let url_safe = sanitize_connection_url(&config.url);
            crate::error::Error::Internal(format!(
                "Failed to connect to database at '{}': {}\n\n\
                Troubleshooting:\n\
                1. Verify database is running and accessible\n\
                2. Check connection URL format: postgres://user:pass@host:port/database\n\
                3. Verify network connectivity (firewall, security groups)\n\
                4. Check credentials and database permissions\n\
                5. Ensure max_connections ({}) doesn't exceed database limits\n\n\
                Original error: {}",
                url_safe,
                categorize_db_error(&e),
                config.max_connections,
                e
            ))
        })?;

    Ok(pool)
}

/// Sanitize connection URL for safe logging (remove password)
#[cfg(feature = "database")]
fn sanitize_connection_url(url: &str) -> String {
    if let Some(at_pos) = url.find('@') {
        if let Some(scheme_end) = url.find("://") {
            let scheme = &url[..=scheme_end + 2];
            let after_at = &url[at_pos..];
            // Find username start
            if let Some(colon_pos) = url[scheme_end + 3..at_pos].find(':') {
                let username = &url[scheme_end + 3..scheme_end + 3 + colon_pos];
                return format!("{}{}:***{}", scheme, username, after_at);
            }
        }
    }
    url.to_string()
}

/// Categorize database error for better user guidance
#[cfg(feature = "database")]
fn categorize_db_error(err: &sqlx::Error) -> &'static str {
    use sqlx::Error;
    match err {
        Error::Configuration(_) => "Configuration error",
        Error::Database(_) => "Database query error",
        Error::Io(_) => "Network I/O error - check connectivity",
        Error::Tls(_) => "TLS/SSL error - check certificate configuration",
        Error::PoolTimedOut => "Connection pool timeout - database may be overloaded",
        Error::PoolClosed => "Connection pool closed",
        Error::WorkerCrashed => "Database worker crashed",
        _ => "Connection error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_config() {
        let config = DatabaseConfig {
            url: "postgres://user:pass@localhost/db".to_string(),
            max_connections: 50,
            min_connections: 5,
            connection_timeout_secs: 10,
            max_retries: 5,
            retry_delay_secs: 2,
            optional: false,
            lazy_init: true,
        };

        assert_eq!(config.max_connections, 50);
        assert_eq!(config.min_connections, 5);
        assert_eq!(config.max_retries, 5);
        assert!(config.lazy_init);
    }
}
