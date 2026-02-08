//! Turso/libsql database connection management
//!
//! Supports three connection modes:
//! - **Local**: SQLite file, no network (like regular SQLite)
//! - **Remote**: Connect to Turso cloud or libsql-server
//! - **EmbeddedReplica**: Local SQLite that syncs with remote Turso

use std::time::Duration;

use crate::{
    config::{TursoConfig, TursoMode},
    error::Result,
};

/// Create a Turso/libsql database connection with retry logic
///
/// This is an internal function used by the TursoDbAgent.
/// It will retry connection attempts based on the configuration.
pub(crate) async fn create_database(config: &TursoConfig) -> Result<libsql::Database> {
    create_database_with_retries(config, config.max_retries).await
}

/// Create a Turso/libsql database with configurable retries
///
/// Uses exponential backoff strategy for retries
async fn create_database_with_retries(
    config: &TursoConfig,
    max_retries: u32,
) -> Result<libsql::Database> {
    let mut attempt = 0;
    let base_delay = Duration::from_secs(config.retry_delay_secs);

    loop {
        match try_create_database(config).await {
            Ok(db) => {
                if attempt > 0 {
                    tracing::info!(
                        "Turso database connection established after {} attempt(s)",
                        attempt + 1
                    );
                } else {
                    tracing::info!("Turso database connected: mode={:?}", config.mode);
                }
                return Ok(db);
            }
            Err(e) => {
                attempt += 1;

                if attempt > max_retries {
                    tracing::error!(
                        "Failed to connect to Turso database after {} attempts: {}",
                        max_retries + 1,
                        e
                    );
                    return Err(e);
                }

                // Calculate exponential backoff
                let delay_multiplier = 2_u32.pow(attempt.saturating_sub(1));
                let delay = base_delay * delay_multiplier;

                tracing::warn!(
                    "Turso connection attempt {} failed: {}. Retrying in {:?}...",
                    attempt,
                    e,
                    delay
                );

                tokio::time::sleep(delay).await;
            }
        }
    }
}

/// Attempt to create a database connection (single try)
async fn try_create_database(config: &TursoConfig) -> Result<libsql::Database> {
    match config.mode {
        TursoMode::Local => build_local_database(config).await,
        TursoMode::Remote => build_remote_database(config).await,
        TursoMode::EmbeddedReplica => build_embedded_replica(config).await,
    }
}

/// Build a local SQLite database
async fn build_local_database(config: &TursoConfig) -> Result<libsql::Database> {
    let path = config.path.as_ref().ok_or_else(|| {
        crate::error::Error::Internal("Turso local mode requires 'path' configuration".into())
    })?;

    tracing::debug!("Creating local Turso database at: {}", path.display());

    let mut builder = libsql::Builder::new_local(path);

    if let Some(ref key) = config.encryption_key {
        let key_bytes: Vec<u8> = key.as_bytes().to_vec();
        builder = builder.encryption_config(libsql::EncryptionConfig::new(
            libsql::Cipher::Aes256Cbc,
            key_bytes.into(),
        ));
    }

    builder.build().await.map_err(|e| {
        crate::error::Error::Internal(format!(
            "Failed to create local Turso database at '{}': {}\n\n\
            Troubleshooting:\n\
            1. Verify the directory exists and is writable\n\
            2. Check file permissions\n\
            3. Ensure the path is valid\n\n\
            Original error: {}",
            path.display(),
            categorize_turso_error(&e),
            e
        ))
    })
}

/// Build a remote-only database connection
async fn build_remote_database(config: &TursoConfig) -> Result<libsql::Database> {
    let url = config.url.as_ref().ok_or_else(|| {
        crate::error::Error::Internal("Turso remote mode requires 'url' configuration".into())
    })?;
    let token = config.auth_token.as_ref().ok_or_else(|| {
        crate::error::Error::Internal(
            "Turso remote mode requires 'auth_token' configuration".into(),
        )
    })?;

    let url_safe = sanitize_connection_url(url);
    tracing::debug!("Connecting to remote Turso database: {}", url_safe);

    libsql::Builder::new_remote(url.clone(), token.clone())
        .build()
        .await
        .map_err(|e| {
            crate::error::Error::Internal(format!(
                "Failed to connect to Turso at '{}': {}\n\n\
                Troubleshooting:\n\
                1. Verify the database URL is correct (format: libsql://your-db.turso.io)\n\
                2. Check that your auth token is valid and not expired\n\
                3. Verify network connectivity to Turso cloud\n\
                4. Check if the database exists and is accessible\n\n\
                Original error: {}",
                url_safe,
                categorize_turso_error(&e),
                e
            ))
        })
}

/// Build an embedded replica database
async fn build_embedded_replica(config: &TursoConfig) -> Result<libsql::Database> {
    let path = config.path.as_ref().ok_or_else(|| {
        crate::error::Error::Internal(
            "Turso embedded_replica mode requires 'path' configuration".into(),
        )
    })?;
    let url = config.url.as_ref().ok_or_else(|| {
        crate::error::Error::Internal(
            "Turso embedded_replica mode requires 'url' configuration".into(),
        )
    })?;
    let token = config.auth_token.as_ref().ok_or_else(|| {
        crate::error::Error::Internal(
            "Turso embedded_replica mode requires 'auth_token' configuration".into(),
        )
    })?;

    let url_safe = sanitize_connection_url(url);
    tracing::debug!(
        "Creating embedded replica at '{}' syncing with '{}'",
        path.display(),
        url_safe
    );

    let mut builder = libsql::Builder::new_remote_replica(path.clone(), url.clone(), token.clone());

    builder = builder.read_your_writes(config.read_your_writes);

    if let Some(secs) = config.sync_interval_secs {
        builder = builder.sync_interval(Duration::from_secs(secs));
    }

    if let Some(ref key) = config.encryption_key {
        let key_bytes: Vec<u8> = key.as_bytes().to_vec();
        builder = builder.encryption_config(libsql::EncryptionConfig::new(
            libsql::Cipher::Aes256Cbc,
            key_bytes.into(),
        ));
    }

    builder.build().await.map_err(|e| {
        crate::error::Error::Internal(format!(
            "Failed to create embedded replica at '{}' syncing with '{}': {}\n\n\
            Troubleshooting:\n\
            1. Verify the local path exists and is writable\n\
            2. Check that the remote URL is correct\n\
            3. Verify the auth token is valid\n\
            4. Check network connectivity to Turso cloud\n\n\
            Original error: {}",
            path.display(),
            url_safe,
            categorize_turso_error(&e),
            e
        ))
    })
}

/// Sanitize connection URL for safe logging (remove auth token from URL if present)
fn sanitize_connection_url(url: &str) -> String {
    // libsql URLs typically don't embed tokens, but let's be safe
    if let Some(at_pos) = url.find('@') {
        if let Some(scheme_end) = url.find("://") {
            let scheme = &url[..=scheme_end + 2];
            let after_at = &url[at_pos..];
            return format!("{}***{}", scheme, after_at);
        }
    }
    url.to_string()
}

/// Categorize Turso error for better user guidance
fn categorize_turso_error(err: &libsql::Error) -> &'static str {
    let err_str = err.to_string().to_lowercase();

    if err_str.contains("auth") || err_str.contains("token") || err_str.contains("unauthorized") {
        "Authentication error - check your auth token"
    } else if err_str.contains("connect") || err_str.contains("network") || err_str.contains("dns")
    {
        "Network connection error - check connectivity"
    } else if err_str.contains("permission") || err_str.contains("denied") {
        "Permission error - check file/database permissions"
    } else if err_str.contains("not found") || err_str.contains("no such") {
        "Resource not found - check database exists"
    } else if err_str.contains("timeout") {
        "Connection timeout - database may be overloaded"
    } else if err_str.contains("corrupt") || err_str.contains("malformed") {
        "Database corruption - may need recovery"
    } else {
        "Connection error"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_sanitize_connection_url_no_credentials() {
        let url = "libsql://my-database.turso.io";
        assert_eq!(sanitize_connection_url(url), url);
    }

    #[test]
    fn test_sanitize_connection_url_with_at_sign() {
        let url = "libsql://user:token@my-database.turso.io";
        let sanitized = sanitize_connection_url(url);
        assert!(sanitized.contains("***"));
        assert!(sanitized.contains("my-database.turso.io"));
    }

    #[test]
    fn test_turso_config_local_mode() {
        let config = TursoConfig {
            mode: TursoMode::Local,
            path: Some(PathBuf::from("./test.db")),
            url: None,
            auth_token: None,
            sync_interval_secs: None,
            encryption_key: None,
            read_your_writes: true,
            max_retries: 5,
            retry_delay_secs: 2,
            optional: false,
            lazy_init: true,
        };

        assert_eq!(config.mode, TursoMode::Local);
        assert!(config.path.is_some());
    }

    #[test]
    fn test_turso_config_remote_mode() {
        let config = TursoConfig {
            mode: TursoMode::Remote,
            path: None,
            url: Some("libsql://my-db.turso.io".to_string()),
            auth_token: Some("test-token".to_string()),
            sync_interval_secs: None,
            encryption_key: None,
            read_your_writes: true,
            max_retries: 5,
            retry_delay_secs: 2,
            optional: false,
            lazy_init: true,
        };

        assert_eq!(config.mode, TursoMode::Remote);
        assert!(config.url.is_some());
        assert!(config.auth_token.is_some());
    }

    #[test]
    fn test_turso_config_embedded_replica_mode() {
        let config = TursoConfig {
            mode: TursoMode::EmbeddedReplica,
            path: Some(PathBuf::from("./replica.db")),
            url: Some("libsql://my-db.turso.io".to_string()),
            auth_token: Some("test-token".to_string()),
            sync_interval_secs: Some(60),
            encryption_key: None,
            read_your_writes: true,
            max_retries: 5,
            retry_delay_secs: 2,
            optional: false,
            lazy_init: true,
        };

        assert_eq!(config.mode, TursoMode::EmbeddedReplica);
        assert!(config.path.is_some());
        assert!(config.url.is_some());
        assert!(config.auth_token.is_some());
        assert_eq!(config.sync_interval_secs, Some(60));
    }

    // =========================================================================
    // Integration Tests - Local Database
    // =========================================================================

    /// Helper to create a temporary database path
    fn temp_db_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("turso_test_{}_{}.db", name, std::process::id()));
        path
    }

    /// Helper to clean up test database files
    fn cleanup_db(path: &PathBuf) {
        let _ = std::fs::remove_file(path);
        // Also remove WAL and SHM files if they exist
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    #[tokio::test]
    async fn test_local_database_creation() {
        let db_path = temp_db_path("creation");

        let config = TursoConfig {
            mode: TursoMode::Local,
            path: Some(db_path.clone()),
            url: None,
            auth_token: None,
            sync_interval_secs: None,
            encryption_key: None,
            read_your_writes: true,
            max_retries: 0,
            retry_delay_secs: 1,
            optional: false,
            lazy_init: false,
        };

        let result = create_database(&config).await;
        assert!(
            result.is_ok(),
            "Failed to create local database: {:?}",
            result.err()
        );

        // libsql doesn't create the file until first write operation, so perform one
        let db = result.unwrap();
        let conn = db.connect().expect("Failed to connect");
        conn.execute("CREATE TABLE IF NOT EXISTS _check (id INTEGER)", ())
            .await
            .expect("Failed to create table");

        // Verify file was created after operation
        assert!(db_path.exists(), "Database file was not created");

        cleanup_db(&db_path);
    }

    #[tokio::test]
    async fn test_local_database_basic_operations() {
        let db_path = temp_db_path("operations");

        let config = TursoConfig {
            mode: TursoMode::Local,
            path: Some(db_path.clone()),
            url: None,
            auth_token: None,
            sync_interval_secs: None,
            encryption_key: None,
            read_your_writes: true,
            max_retries: 0,
            retry_delay_secs: 1,
            optional: false,
            lazy_init: false,
        };

        let db = create_database(&config)
            .await
            .expect("Failed to create database");
        let conn = db.connect().expect("Failed to get connection");

        // Create a table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
            (),
        )
        .await
        .expect("Failed to create table");

        // Insert data
        conn.execute("INSERT INTO users (name) VALUES (?1)", ["Alice"])
            .await
            .expect("Failed to insert data");

        conn.execute("INSERT INTO users (name) VALUES (?1)", ["Bob"])
            .await
            .expect("Failed to insert data");

        // Query data
        let mut rows = conn
            .query("SELECT id, name FROM users ORDER BY id", ())
            .await
            .expect("Failed to query data");

        let row1 = rows
            .next()
            .await
            .expect("Failed to get row")
            .expect("No row found");
        assert_eq!(row1.get::<i64>(0).unwrap(), 1);
        assert_eq!(row1.get::<String>(1).unwrap(), "Alice");

        let row2 = rows
            .next()
            .await
            .expect("Failed to get row")
            .expect("No row found");
        assert_eq!(row2.get::<i64>(0).unwrap(), 2);
        assert_eq!(row2.get::<String>(1).unwrap(), "Bob");

        // Verify no more rows
        assert!(rows
            .next()
            .await
            .expect("Failed to check next row")
            .is_none());

        cleanup_db(&db_path);
    }

    #[tokio::test]
    async fn test_local_database_transactions() {
        let db_path = temp_db_path("transactions");

        let config = TursoConfig {
            mode: TursoMode::Local,
            path: Some(db_path.clone()),
            url: None,
            auth_token: None,
            sync_interval_secs: None,
            encryption_key: None,
            read_your_writes: true,
            max_retries: 0,
            retry_delay_secs: 1,
            optional: false,
            lazy_init: false,
        };

        let db = create_database(&config)
            .await
            .expect("Failed to create database");
        let conn = db.connect().expect("Failed to get connection");

        // Create table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS accounts (id INTEGER PRIMARY KEY, balance INTEGER NOT NULL)",
            (),
        )
        .await
        .expect("Failed to create table");

        // Insert initial data
        conn.execute("INSERT INTO accounts (balance) VALUES (100)", ())
            .await
            .expect("Failed to insert");

        // Perform a transaction (simulated with multiple statements)
        conn.execute(
            "UPDATE accounts SET balance = balance - 50 WHERE id = 1",
            (),
        )
        .await
        .expect("Failed to update");

        // Verify the update
        let mut rows = conn
            .query("SELECT balance FROM accounts WHERE id = 1", ())
            .await
            .expect("Failed to query");

        let row = rows
            .next()
            .await
            .expect("Failed to get row")
            .expect("No row");
        assert_eq!(row.get::<i64>(0).unwrap(), 50);

        cleanup_db(&db_path);
    }

    #[tokio::test]
    async fn test_local_database_error_handling() {
        // libsql defers errors for invalid paths, so we test that an operation fails
        let config = TursoConfig {
            mode: TursoMode::Local,
            path: Some(PathBuf::from("/nonexistent/directory/test.db")),
            url: None,
            auth_token: None,
            sync_interval_secs: None,
            encryption_key: None,
            read_your_writes: true,
            max_retries: 0,
            retry_delay_secs: 1,
            optional: false,
            lazy_init: false,
        };

        let result = create_database(&config).await;
        // libsql may succeed in creating the database object, but fail on first operation
        if let Ok(db) = result {
            let conn = db.connect();
            if let Ok(conn) = conn {
                // Try to perform an operation - this should fail for invalid path
                let op_result = conn.execute("SELECT 1", ()).await;
                assert!(
                    op_result.is_err(),
                    "Operation should fail with invalid path"
                );
            }
            // If connect() fails, that's also acceptable
        }
        // If create_database fails immediately, that's also correct behavior
    }

    #[tokio::test]
    async fn test_local_mode_missing_path_error() {
        let config = TursoConfig {
            mode: TursoMode::Local,
            path: None, // Missing required path
            url: None,
            auth_token: None,
            sync_interval_secs: None,
            encryption_key: None,
            read_your_writes: true,
            max_retries: 0,
            retry_delay_secs: 1,
            optional: false,
            lazy_init: false,
        };

        let result = create_database(&config).await;
        assert!(
            result.is_err(),
            "Should fail when path is missing for local mode"
        );

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("path"),
            "Error should mention missing path"
        );
    }

    #[tokio::test]
    async fn test_remote_mode_missing_url_error() {
        let config = TursoConfig {
            mode: TursoMode::Remote,
            path: None,
            url: None, // Missing required URL
            auth_token: Some("token".to_string()),
            sync_interval_secs: None,
            encryption_key: None,
            read_your_writes: true,
            max_retries: 0,
            retry_delay_secs: 1,
            optional: false,
            lazy_init: false,
        };

        let result = create_database(&config).await;
        assert!(
            result.is_err(),
            "Should fail when URL is missing for remote mode"
        );

        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("url"), "Error should mention missing URL");
    }

    #[tokio::test]
    async fn test_remote_mode_missing_token_error() {
        let config = TursoConfig {
            mode: TursoMode::Remote,
            path: None,
            url: Some("libsql://test.turso.io".to_string()),
            auth_token: None, // Missing required token
            sync_interval_secs: None,
            encryption_key: None,
            read_your_writes: true,
            max_retries: 0,
            retry_delay_secs: 1,
            optional: false,
            lazy_init: false,
        };

        let result = create_database(&config).await;
        assert!(
            result.is_err(),
            "Should fail when auth_token is missing for remote mode"
        );

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("auth_token"),
            "Error should mention missing auth_token"
        );
    }

    #[tokio::test]
    async fn test_embedded_replica_missing_config_errors() {
        // Missing path
        let config = TursoConfig {
            mode: TursoMode::EmbeddedReplica,
            path: None,
            url: Some("libsql://test.turso.io".to_string()),
            auth_token: Some("token".to_string()),
            sync_interval_secs: None,
            encryption_key: None,
            read_your_writes: true,
            max_retries: 0,
            retry_delay_secs: 1,
            optional: false,
            lazy_init: false,
        };

        let result = create_database(&config).await;
        assert!(
            result.is_err(),
            "Should fail when path is missing for embedded replica mode"
        );

        // Missing URL
        let config = TursoConfig {
            mode: TursoMode::EmbeddedReplica,
            path: Some(PathBuf::from("./test.db")),
            url: None,
            auth_token: Some("token".to_string()),
            sync_interval_secs: None,
            encryption_key: None,
            read_your_writes: true,
            max_retries: 0,
            retry_delay_secs: 1,
            optional: false,
            lazy_init: false,
        };

        let result = create_database(&config).await;
        assert!(
            result.is_err(),
            "Should fail when URL is missing for embedded replica mode"
        );

        // Missing token
        let config = TursoConfig {
            mode: TursoMode::EmbeddedReplica,
            path: Some(PathBuf::from("./test.db")),
            url: Some("libsql://test.turso.io".to_string()),
            auth_token: None,
            sync_interval_secs: None,
            encryption_key: None,
            read_your_writes: true,
            max_retries: 0,
            retry_delay_secs: 1,
            optional: false,
            lazy_init: false,
        };

        let result = create_database(&config).await;
        assert!(
            result.is_err(),
            "Should fail when auth_token is missing for embedded replica mode"
        );
    }

    #[tokio::test]
    async fn test_local_database_concurrent_access() {
        let db_path = temp_db_path("concurrent");

        let config = TursoConfig {
            mode: TursoMode::Local,
            path: Some(db_path.clone()),
            url: None,
            auth_token: None,
            sync_interval_secs: None,
            encryption_key: None,
            read_your_writes: true,
            max_retries: 0,
            retry_delay_secs: 1,
            optional: false,
            lazy_init: false,
        };

        let db = std::sync::Arc::new(
            create_database(&config)
                .await
                .expect("Failed to create database"),
        );

        // Setup table
        let conn = db.connect().expect("Failed to get connection");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS counter (id INTEGER PRIMARY KEY, value INTEGER NOT NULL)",
            (),
        )
        .await
        .expect("Failed to create table");
        conn.execute("INSERT INTO counter (value) VALUES (0)", ())
            .await
            .expect("Failed to insert");

        // Spawn multiple tasks that increment the counter
        let mut handles = vec![];
        for _ in 0..5 {
            let db_clone = db.clone();
            let handle = tokio::spawn(async move {
                let conn = db_clone.connect().expect("Failed to get connection");
                conn.execute("UPDATE counter SET value = value + 1 WHERE id = 1", ())
                    .await
                    .expect("Failed to update");
            });
            handles.push(handle);
        }

        // Wait for all tasks
        for handle in handles {
            handle.await.expect("Task panicked");
        }

        // Verify final count
        let conn = db.connect().expect("Failed to get connection");
        let mut rows = conn
            .query("SELECT value FROM counter WHERE id = 1", ())
            .await
            .expect("Failed to query");

        let row = rows
            .next()
            .await
            .expect("Failed to get row")
            .expect("No row");
        let value: i64 = row.get(0).unwrap();
        assert_eq!(value, 5, "Counter should be 5 after 5 increments");

        cleanup_db(&db_path);
    }
}
