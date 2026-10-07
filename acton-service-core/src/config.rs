//! Driver-independent connection configuration.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
/// Database configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Database connection URL
    pub url: String,

    /// Maximum number of connections in the pool
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,

    /// Minimum idle connections
    #[serde(default = "default_min_connections")]
    pub min_connections: u32,

    /// Connection timeout in seconds
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout_secs: u64,

    /// Maximum retry attempts for establishing database connection
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    /// Delay between retry attempts in seconds
    #[serde(default = "default_retry_delay")]
    pub retry_delay_secs: u64,

    /// Whether database is optional (service can start without it)
    #[serde(default = "default_false")]
    pub optional: bool,

    /// Whether to initialize connection lazily (in background)
    #[serde(default = "default_lazy_init")]
    pub lazy_init: bool,

    /// Microsoft SQL Server authentication mode.
    ///
    /// Ignored by non-MSSQL backends. `integrated` authenticates as the
    /// process identity through SSPI on Windows or GSSAPI/Kerberos on Unix.
    #[serde(default)]
    pub mssql_auth: MssqlAuthMode,
}

/// Authentication used by the Microsoft SQL Server connection pool.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MssqlAuthMode {
    /// Use the credentials encoded in the ADO connection string.
    #[default]
    ConnectionString,
    /// Authenticate as the service process using SSPI or Kerberos/GSSAPI.
    Integrated,
}

/// Turso/libsql connection mode
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TursoMode {
    /// Local SQLite file (no network, like regular SQLite)
    #[default]
    Local,
    /// Remote-only (connect to Turso cloud or libsql-server)
    Remote,
    /// Embedded replica (local SQLite that syncs with remote Turso)
    EmbeddedReplica,
}

/// Turso/libsql database configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TursoConfig {
    /// Connection mode
    #[serde(default)]
    pub mode: TursoMode,

    /// Local database file path (required for Local and EmbeddedReplica modes)
    #[serde(default)]
    pub path: Option<PathBuf>,

    /// Remote database URL (required for Remote and EmbeddedReplica modes)
    /// Format: libsql://your-db.turso.io or http://localhost:8080
    #[serde(default)]
    pub url: Option<String>,

    /// Authentication token (required for Remote and EmbeddedReplica modes)
    #[serde(default)]
    pub auth_token: Option<String>,

    /// Sync interval in seconds (EmbeddedReplica mode only)
    /// If set, enables automatic background sync
    #[serde(default)]
    pub sync_interval_secs: Option<u64>,

    /// Encryption key for local database (optional, all modes)
    #[serde(default)]
    pub encryption_key: Option<String>,

    /// Read-your-writes consistency (EmbeddedReplica mode only)
    /// When true, writes are visible locally before sync completes
    #[serde(default = "default_true")]
    pub read_your_writes: bool,

    /// Maximum retry attempts for connection
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    /// Delay between retry attempts in seconds
    #[serde(default = "default_retry_delay")]
    pub retry_delay_secs: u64,

    /// Whether database is optional (service can start without it)
    #[serde(default = "default_false")]
    pub optional: bool,

    /// Whether to initialize connection lazily (in background)
    #[serde(default = "default_lazy_init")]
    pub lazy_init: bool,
}

/// SurrealDB database configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurrealDbConfig {
    /// Connection URL (ws://localhost:8000, mem://, http://localhost:8000, etc.)
    pub url: String,

    /// Namespace to use
    #[serde(default = "default_surrealdb_namespace")]
    pub namespace: String,

    /// Database to use
    #[serde(default = "default_surrealdb_database")]
    pub database: String,

    /// Username for authentication (optional, for root-level access)
    #[serde(default)]
    pub username: Option<String>,

    /// Password for authentication (optional, for root-level access)
    #[serde(default)]
    pub password: Option<String>,

    /// Maximum retry attempts for establishing connection
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    /// Delay between retry attempts in seconds
    #[serde(default = "default_retry_delay")]
    pub retry_delay_secs: u64,

    /// Whether database is optional (service can start without it)
    #[serde(default = "default_false")]
    pub optional: bool,

    /// Whether to initialize connection lazily (in background)
    #[serde(default = "default_lazy_init")]
    pub lazy_init: bool,
}

/// ClickHouse analytical database configuration
///
/// ClickHouse is a columnar OLAP database used as a complementary analytical store.
/// Unlike the primary database backends (PostgreSQL, Turso, SurrealDB), ClickHouse
/// is composable and can be used alongside any of them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClickHouseConfig {
    /// ClickHouse HTTP URL (e.g., `http://localhost:8123`)
    pub url: String,

    /// Database name
    #[serde(default = "default_clickhouse_database")]
    pub database: String,

    /// Username for authentication
    #[serde(default)]
    pub username: Option<String>,

    /// Password for authentication
    #[serde(default)]
    pub password: Option<String>,

    /// Maximum retry attempts for establishing connection
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    /// Delay between retry attempts in seconds
    #[serde(default = "default_retry_delay")]
    pub retry_delay_secs: u64,

    /// Whether ClickHouse is optional (service can start without it)
    #[serde(default = "default_false")]
    pub optional: bool,

    /// Whether to initialize connection lazily (in background)
    #[serde(default = "default_lazy_init")]
    pub lazy_init: bool,
}

fn default_clickhouse_database() -> String {
    "default".to_string()
}

fn default_max_connections() -> u32 {
    50
}
fn default_min_connections() -> u32 {
    5
}
fn default_connection_timeout() -> u64 {
    10
}
fn default_max_retries() -> u32 {
    5
}
fn default_retry_delay() -> u64 {
    2
}
fn default_lazy_init() -> bool {
    true
}
fn default_true() -> bool {
    true
}
fn default_false() -> bool {
    false
}
fn default_surrealdb_namespace() -> String {
    "default".to_string()
}
fn default_surrealdb_database() -> String {
    "default".to_string()
}
