//! Configuration management using Figment
//!
//! Configuration is loaded from multiple sources with the following precedence (highest to lowest):
//! 1. Environment variables (prefix: ACTON_)
//! 2. Current working directory: ./config.toml
//! 3. Platform user config directory (`XDG_CONFIG_HOME` on Unix, `APPDATA` on Windows)
//! 4. Platform system directory (`/etc` on Unix, `PROGRAMDATA` on Windows)
//! 5. Default values

use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use crate::error::Result;

/// Main configuration structure with optional custom extensions
///
/// The generic parameter `T` allows users to extend the configuration with custom fields
/// that will be automatically loaded from the same config.toml file.
///
/// # Examples
///
/// ```rust,ignore
/// // No custom config (default)
/// let config = Config::<()>::load()?;
///
/// // With custom config
/// #[derive(Serialize, Deserialize, Clone, Default)]
/// struct MyCustomConfig {
///     api_key: String,
///     feature_flags: HashMap<String, bool>,
/// }
///
/// let config = Config::<MyCustomConfig>::load()?;
/// println!("API Key: {}", config.custom.api_key);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(serialize = "T: Serialize", deserialize = "T: DeserializeOwned"))]
pub struct Config<T = ()>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    /// Service configuration
    pub service: ServiceConfig,

    /// Token authentication configuration (PASETO by default, JWT with feature)
    #[serde(default)]
    pub token: Option<TokenConfig>,

    /// Rate limiting configuration
    pub rate_limit: RateLimitConfig,

    /// Middleware configuration
    #[serde(default)]
    pub middleware: MiddlewareConfig,

    /// Database configuration (optional)
    #[serde(default)]
    pub database: Option<DatabaseConfig>,

    /// Turso/libsql configuration (optional)
    #[cfg(feature = "turso")]
    #[serde(default)]
    pub turso: Option<TursoConfig>,

    /// SurrealDB configuration (optional)
    #[cfg(feature = "surrealdb")]
    #[serde(default)]
    pub surrealdb: Option<SurrealDbConfig>,

    /// Redis configuration (optional)
    #[serde(default)]
    pub redis: Option<RedisConfig>,

    /// NATS configuration (optional)
    #[serde(default)]
    pub nats: Option<NatsConfig>,

    /// ClickHouse configuration (optional)
    #[cfg(feature = "clickhouse")]
    #[serde(default)]
    pub clickhouse: Option<ClickHouseConfig>,

    /// OpenTelemetry configuration (optional)
    #[serde(default)]
    pub otlp: Option<OtlpConfig>,

    /// gRPC configuration (optional)
    #[serde(default)]
    pub grpc: Option<GrpcConfig>,

    /// WebSocket configuration (optional)
    #[cfg(feature = "websocket")]
    #[serde(default)]
    pub websocket: Option<crate::websocket::WebSocketConfig>,

    /// Cedar authorization configuration (optional)
    #[cfg(feature = "cedar-authz")]
    #[serde(default)]
    pub cedar: Option<CedarConfig>,

    /// GraphQL transport configuration (optional)
    #[cfg(feature = "graphql")]
    #[serde(default)]
    pub graphql: Option<GraphQLConfig>,

    /// Session configuration (optional)
    #[cfg(feature = "session")]
    #[serde(default)]
    pub session: Option<crate::session::SessionConfig>,

    /// Audit logging configuration (optional)
    #[cfg(feature = "audit")]
    #[serde(default)]
    pub audit: Option<crate::audit::AuditConfig>,

    /// Authentication configuration (optional, requires `auth` feature)
    #[cfg(feature = "auth")]
    #[serde(default)]
    pub auth: Option<crate::auth::AuthConfig>,

    /// Login lockout configuration (optional)
    #[cfg(feature = "login-lockout")]
    #[serde(default)]
    pub lockout: Option<crate::lockout::LockoutConfig>,

    /// TLS configuration (optional, requires `tls` feature)
    #[cfg(feature = "tls")]
    #[serde(default)]
    pub tls: Option<TlsConfig>,

    /// Caller authorization for mutual TLS (optional, requires `tls` feature)
    #[cfg(feature = "tls")]
    #[serde(default)]
    pub caller_auth: Option<CallerAuthConfig>,

    /// Journald configuration (optional, requires `journald` feature)
    #[cfg(feature = "journald")]
    #[serde(default)]
    pub journald: Option<JournaldConfig>,

    /// Account management configuration (optional)
    #[cfg(feature = "accounts")]
    #[serde(default)]
    pub accounts: Option<crate::accounts::AccountsConfig>,

    /// Background worker configuration (optional)
    #[serde(default)]
    pub background_worker: Option<crate::agents::BackgroundWorkerConfig>,

    /// Custom configuration extensions
    ///
    /// Any fields in config.toml that don't match the above framework fields
    /// will be deserialized into this field. Use `()` (unit type) for no custom config.
    #[serde(flatten)]
    pub custom: T,
}

/// Service-level configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    /// Service name
    pub name: String,

    /// IP address to bind the listener to.
    ///
    /// Defaults to `0.0.0.0` (all interfaces) for backward compatibility.
    /// Set to `127.0.0.1` or `::1` to expose a loopback-only surface.
    #[serde(default = "default_bind")]
    pub bind: IpAddr,

    /// Port to listen on
    #[serde(default = "default_port")]
    pub port: u16,

    /// Log level (trace, debug, info, warn, error)
    #[serde(default = "default_log_level")]
    pub log_level: String,

    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Environment (dev, staging, production)
    #[serde(default = "default_environment")]
    pub environment: String,

    /// Whether the request context trusts forwarded-for headers when resolving
    /// the client IP.
    ///
    /// Governs the [`RequestContext`](crate::middleware::request_context::RequestContext)
    /// resolution every downstream consumer shares — including audit-event
    /// source capture. When `true`, `X-Forwarded-For` (first value) and
    /// `X-Real-IP` are consulted before the direct TCP/TLS peer address. Only
    /// enable behind a proxy you trust to set these headers — a direct client
    /// can otherwise spoof the IP recorded in its audit trail.
    ///
    /// Defaults to `false` (do not trust) to be safe by default. Rate limiting
    /// has its own independent flag ([`RateLimitConfig::trust_forwarded_headers`])
    /// so the governor's trust posture can differ from the audit record's.
    #[serde(default = "default_false")]
    pub trust_forwarded_headers: bool,
}

/// Token authentication configuration
///
/// Supports PASETO (default) and JWT (requires `jwt` feature).
/// Uses tagged enum for config file format discrimination.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "format", rename_all = "lowercase")]
pub enum TokenConfig {
    /// PASETO token configuration (default)
    Paseto(PasetoConfig),
    /// JWT token configuration (requires `jwt` feature)
    #[cfg(feature = "jwt")]
    Jwt(JwtConfig),
}

impl Default for TokenConfig {
    fn default() -> Self {
        TokenConfig::Paseto(PasetoConfig::default())
    }
}

/// PASETO token configuration
///
/// Supports V4 Local (symmetric encryption) and V4 Public (asymmetric signatures).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasetoConfig {
    /// PASETO version (currently only "v4" supported)
    #[serde(default = "default_paseto_version")]
    pub version: String,

    /// Token purpose: "local" (symmetric) or "public" (asymmetric)
    #[serde(default = "default_paseto_purpose")]
    pub purpose: String,

    /// Path to key file
    /// - For "local": 32-byte symmetric key
    /// - For "public": Ed25519 public key (32 bytes)
    pub key_path: PathBuf,

    /// Issuer to validate (optional)
    #[serde(default)]
    pub issuer: Option<String>,

    /// Audience to validate (optional)
    #[serde(default)]
    pub audience: Option<String>,

    /// Path prefixes that bypass token authentication.
    ///
    /// Requests whose path starts with any of these prefixes will be passed
    /// through without requiring a bearer token. Use this for session-based
    /// frontend routes (e.g. `/admin/`, `/forge/`) that coexist with
    /// token-protected API routes.
    ///
    /// Infrastructure paths (`/health`, `/ready`, `/swagger-ui`, `/api-docs`)
    /// are always skipped regardless of this setting.
    #[serde(default)]
    pub public_paths: Vec<String>,
}

impl Default for PasetoConfig {
    fn default() -> Self {
        Self {
            version: default_paseto_version(),
            purpose: default_paseto_purpose(),
            key_path: PathBuf::from("./keys/paseto.key"),
            issuer: None,
            audience: None,
            public_paths: Vec::new(),
        }
    }
}

fn default_paseto_version() -> String {
    "v4".to_string()
}

fn default_paseto_purpose() -> String {
    "local".to_string()
}

/// JWT configuration (requires `jwt` feature)
#[cfg(feature = "jwt")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtConfig {
    /// Path to public key for JWT verification
    pub public_key_path: PathBuf,

    /// JWT algorithm (RS256, ES256, HS256)
    #[serde(default = "default_jwt_algorithm")]
    pub algorithm: String,

    /// JWT issuer to validate
    #[serde(default)]
    pub issuer: Option<String>,

    /// JWT audience to validate
    #[serde(default)]
    pub audience: Option<String>,

    /// Path prefixes that bypass token authentication.
    ///
    /// Requests whose path starts with any of these prefixes will be passed
    /// through without requiring a bearer token. Use this for session-based
    /// frontend routes (e.g. `/admin/`, `/forge/`) that coexist with
    /// token-protected API routes.
    ///
    /// Infrastructure paths (`/health`, `/ready`, `/swagger-ui`, `/api-docs`)
    /// are always skipped regardless of this setting.
    #[serde(default)]
    pub public_paths: Vec<String>,
}

/// Rate limiting configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Requests per minute per user (global default)
    #[serde(default = "default_per_user_rpm")]
    pub per_user_rpm: u32,

    /// Requests per minute per client (global default)
    #[serde(default = "default_per_client_rpm")]
    pub per_client_rpm: u32,

    /// Rate limit window in seconds
    #[serde(default = "default_window_secs")]
    pub window_secs: u64,

    /// Per-route rate limit overrides
    ///
    /// Routes can be specified as:
    /// - Exact paths: `/api/v1/users`
    /// - Method-prefixed: `POST /api/v1/uploads`
    /// - With wildcards: `/api/v1/users/*`, `/api/*/admin`
    /// - With ID placeholders: `/api/v1/users/{id}`
    ///
    /// Paths with UUIDs or numeric IDs are automatically normalized to `{id}`.
    ///
    /// # Example
    /// ```toml
    /// [rate_limit.routes."/api/v1/heavy-endpoint"]
    /// requests_per_minute = 10
    /// burst_size = 2
    ///
    /// [rate_limit.routes."POST /api/v1/uploads"]
    /// requests_per_minute = 5
    /// per_user = true
    /// ```
    #[serde(default)]
    pub routes: std::collections::HashMap<String, RouteRateLimitConfig>,

    /// Whether to auto-apply the governor rate-limit middleware in `ServiceBuilder`.
    ///
    /// When `true` (default) and the `governor` feature is enabled, the middleware
    /// is attached to the outer router during `ServiceBuilder::build()`. The layer
    /// runs *before* axum strips any nested route prefix, so route-rate-limit keys
    /// match the full request path (e.g. `"POST /api/v1/uploads"` works as documented).
    ///
    /// Set to `false` to disable auto-apply and wire the middleware manually.
    #[serde(default = "default_true")]
    pub auto_apply: bool,

    /// Whether to trust forwarded-for headers when resolving the client IP.
    ///
    /// When `true`, the middleware reads `X-Forwarded-For` (first value) and
    /// `X-Real-IP` before falling back to the direct TCP peer address. Only
    /// enable behind a proxy you trust to set these headers — direct clients
    /// can otherwise spoof their IP.
    ///
    /// Defaults to `false` (do not trust) to be safe by default.
    #[serde(default = "default_false")]
    pub trust_forwarded_headers: bool,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            per_user_rpm: default_per_user_rpm(),
            per_client_rpm: default_per_client_rpm(),
            window_secs: default_window_secs(),
            routes: std::collections::HashMap::new(),
            auto_apply: true,
            trust_forwarded_headers: false,
        }
    }
}

/// Per-route rate limit configuration
///
/// Configures rate limiting for a specific route or route pattern.
/// When a request matches a route pattern, these settings override the global defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteRateLimitConfig {
    /// Maximum requests per minute for this route
    pub requests_per_minute: u32,

    /// Burst size for local (governor) rate limiting
    ///
    /// Allows temporary spikes above the base rate.
    /// Only used with governor-based rate limiting.
    #[serde(default = "default_route_burst_size")]
    pub burst_size: u32,

    /// Whether the limit is per-user (true) or global for the route (false)
    ///
    /// - `true`: Each user gets their own rate limit bucket for this route
    /// - `false`: All users share a single rate limit bucket for this route
    ///
    /// Per-user tracking requires JWT authentication. Unauthenticated requests
    /// fall back to IP-based tracking when `per_user` is true.
    #[serde(default = "default_true")]
    pub per_user: bool,
}

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
}

/// Turso/libsql connection mode
#[cfg(feature = "turso")]
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
#[cfg(feature = "turso")]
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
#[cfg(feature = "surrealdb")]
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

/// Redis configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisConfig {
    /// Redis connection URL (redis://host:port or cluster URLs)
    pub url: String,

    /// Maximum number of connections in the pool
    #[serde(default = "default_redis_max_connections")]
    pub max_connections: usize,

    /// Connection timeout in seconds
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout_secs: u64,

    /// Maximum retry attempts for establishing Redis connection
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    /// Delay between retry attempts in seconds
    #[serde(default = "default_retry_delay")]
    pub retry_delay_secs: u64,

    /// Whether Redis is optional (service can start without it)
    #[serde(default = "default_false")]
    pub optional: bool,

    /// Whether to initialize connection lazily (in background)
    #[serde(default = "default_lazy_init")]
    pub lazy_init: bool,
}

/// NATS configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsConfig {
    /// NATS server URL
    pub url: String,

    /// Connection name
    #[serde(default)]
    pub name: Option<String>,

    /// Max reconnection attempts
    #[serde(default = "default_max_reconnects")]
    pub max_reconnects: usize,

    /// Maximum retry attempts for initial connection
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    /// Delay between retry attempts in seconds
    #[serde(default = "default_retry_delay")]
    pub retry_delay_secs: u64,

    /// Whether NATS is optional (service can start without it)
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

/// OpenTelemetry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtlpConfig {
    /// OTLP endpoint URL
    pub endpoint: String,

    /// Service name for tracing
    #[serde(default)]
    pub service_name: Option<String>,

    /// Enable tracing
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// gRPC server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrpcConfig {
    /// Enable gRPC server
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Use separate port for gRPC (if false, shares port with HTTP)
    #[serde(default = "default_false")]
    pub use_separate_port: bool,

    /// IP address to bind the gRPC listener to.
    ///
    /// When `None` (default), falls back to the service-level `[service] bind`.
    /// Only used when `use_separate_port` is true; the hybrid single-port mode
    /// shares the HTTP listener and its bind address.
    #[serde(default)]
    pub bind: Option<IpAddr>,

    /// Per-listener TLS configuration for the gRPC surface (requires `tls` feature).
    ///
    /// When present, this section is authoritative for the separate-port gRPC
    /// listener: `enabled = true` terminates TLS using this certificate/key
    /// independently of the HTTP listener, while `enabled = false` serves
    /// plaintext gRPC even when the shared `[tls]` is active (useful for a
    /// loopback-only gRPC surface). When `None`, the gRPC listener falls back
    /// to the shared `[tls]` configuration.
    #[cfg(feature = "tls")]
    #[serde(default)]
    pub tls: Option<TlsConfig>,

    /// gRPC port (only used if use_separate_port is true)
    #[serde(default = "default_grpc_port")]
    pub port: u16,

    /// Enable gRPC reflection service
    #[serde(default = "default_true")]
    pub reflection_enabled: bool,

    /// Enable gRPC health check service
    #[serde(default = "default_true")]
    pub health_check_enabled: bool,

    /// Maximum message size in MB
    #[serde(default = "default_grpc_max_message_mb")]
    pub max_message_size_mb: usize,

    /// Connection timeout in seconds
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout_secs: u64,

    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Protocol buffer runtime configuration
    #[serde(default)]
    pub proto: ProtoConfig,
}

impl Default for GrpcConfig {
    /// Every field reuses the serde default function that backs it, so a
    /// programmatically constructed `GrpcConfig` and one deserialized from a
    /// `[grpc]` section that omits those keys agree exactly.
    ///
    /// `enabled` is the one deliberate divergence. Deserialization defaults it
    /// to `true` because writing a `[grpc]` section at all is a statement of
    /// intent, whereas `GrpcConfig::default()` is only a starting point for
    /// building one up — it must not silently stand up a gRPC surface the
    /// caller never asked for. Set `enabled: true` explicitly to serve gRPC.
    fn default() -> Self {
        Self {
            enabled: false,
            use_separate_port: default_false(),
            bind: None,
            #[cfg(feature = "tls")]
            tls: None,
            port: default_grpc_port(),
            reflection_enabled: default_true(),
            health_check_enabled: default_true(),
            max_message_size_mb: default_grpc_max_message_mb(),
            connection_timeout_secs: default_connection_timeout(),
            timeout_secs: default_timeout(),
            proto: ProtoConfig::default(),
        }
    }
}

/// Protocol buffer runtime configuration
///
/// NOTE: This is RUNTIME configuration only. Proto compilation happens at build time.
/// See `acton_service::build_utils` for build-time proto compilation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtoConfig {
    /// Proto directory reference (for documentation/tooling only, not used during compilation)
    ///
    /// Build-time compilation uses `ACTON_PROTO_DIR` environment variable or `proto/` convention.
    /// This field can be used by runtime tooling (e.g., generating OpenAPI from protos).
    #[serde(default = "default_proto_dir")]
    pub dir: String,

    /// Service registry endpoint for dynamic service registration
    ///
    /// Example: "consul://localhost:8500" or "etcd://localhost:2379"
    #[serde(default)]
    pub service_registry: Option<String>,

    /// Service mesh integration endpoint
    ///
    /// Used for service mesh sidecar integration (Istio, Linkerd, etc.)
    #[serde(default)]
    pub service_mesh_endpoint: Option<String>,

    /// Enable proto validation (if using buf validate or similar)
    #[serde(default = "default_false")]
    pub validation_enabled: bool,

    /// Service metadata for discovery and registration
    ///
    /// Key-value pairs for service mesh/registry metadata
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
}

impl Default for ProtoConfig {
    fn default() -> Self {
        Self {
            dir: default_proto_dir(),
            service_registry: None,
            service_mesh_endpoint: None,
            validation_enabled: false,
            metadata: std::collections::HashMap::new(),
        }
    }
}

impl GrpcConfig {
    /// Get the effective port (either separate port or HTTP port)
    pub fn effective_port(&self, http_port: u16) -> u16 {
        if self.use_separate_port {
            self.port
        } else {
            http_port
        }
    }

    /// Resolve the effective bind address.
    ///
    /// Returns the gRPC-specific bind address when set, otherwise the supplied
    /// service-level bind address.
    pub fn effective_bind(&self, service_bind: IpAddr) -> IpAddr {
        self.bind.unwrap_or(service_bind)
    }

    /// Get max message size in bytes
    pub fn max_message_size_bytes(&self) -> usize {
        self.max_message_size_mb * 1024 * 1024
    }

    /// Get connection timeout as Duration
    pub fn connection_timeout(&self) -> Duration {
        Duration::from_secs(self.connection_timeout_secs)
    }

    /// Get request timeout as Duration
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }
}

/// Cedar authorization configuration
#[cfg(feature = "cedar-authz")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CedarConfig {
    /// Enable Cedar authorization
    #[serde(default = "default_false")]
    pub enabled: bool,

    /// Path to Cedar policy file
    pub policy_path: PathBuf,

    /// Enable policy hot-reload (watch file for changes)
    #[serde(default = "default_false")]
    pub hot_reload: bool,

    /// Hot-reload check interval in seconds
    #[serde(default = "default_cedar_hot_reload_interval")]
    pub hot_reload_interval_secs: u64,

    /// Enable policy caching (requires cache feature)
    #[serde(default = "default_true")]
    pub cache_enabled: bool,

    /// Policy cache TTL in seconds
    #[serde(default = "default_cedar_policy_cache_ttl")]
    pub cache_ttl_secs: u64,

    /// Fail open on policy evaluation errors
    /// - true: Allow requests when policy evaluation fails (permissive)
    /// - false: Deny requests when policy evaluation fails (strict)
    #[serde(default = "default_false")]
    pub fail_open: bool,
}

#[cfg(feature = "cedar-authz")]
impl CedarConfig {
    /// Get hot-reload interval as Duration
    pub fn hot_reload_interval(&self) -> Duration {
        Duration::from_secs(self.hot_reload_interval_secs)
    }

    /// Get cache TTL as Duration
    pub fn cache_ttl(&self) -> Duration {
        Duration::from_secs(self.cache_ttl_secs)
    }
}

/// GraphQL transport configuration (requires `graphql` feature).
///
/// Drives `ServiceBuilder::with_versioned_graphql`. When `enabled = false`
/// the schemas registered on the builder are ignored at mount time.
#[cfg(feature = "graphql")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphQLConfig {
    /// Enable the GraphQL transport. When false, registered schemas are not
    /// mounted onto the Axum router.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Maximum allowed query depth. Queries that exceed this depth are
    /// rejected before execution. `None` disables the limit.
    #[serde(default)]
    pub max_query_depth: Option<usize>,

    /// Maximum allowed query complexity. Implementation uses
    /// `async_graphql::SchemaBuilder::limit_complexity`. `None` disables the
    /// limit.
    #[serde(default)]
    pub max_query_complexity: Option<usize>,

    /// Serve GraphiQL on `GET /api/v{n}/graphql`. Disable in production if
    /// the schema is sensitive.
    #[serde(default = "default_true")]
    pub graphiql_enabled: bool,

    /// Allow schema introspection (`__schema`, `__type`). Disable in
    /// production to harden the schema against probing.
    #[serde(default = "default_true")]
    pub introspection_enabled: bool,
}

#[cfg(feature = "graphql")]
impl Default for GraphQLConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_query_depth: None,
            max_query_complexity: None,
            graphiql_enabled: true,
            introspection_enabled: true,
        }
    }
}

/// TLS configuration (requires `tls` feature)
///
/// When enabled, the server listens for HTTPS connections using rustls.
/// Certificates are loaded at startup from PEM files.
#[cfg(feature = "tls")]
#[derive(Debug, Clone, Serialize, Deserialize)]
// Reject unknown keys in `[tls]` and `[grpc.tls]`. A typo like
// `reload_interval_sec` (missing the `s`) would otherwise be silently ignored
// and quietly disarm certificate rotation; deny_unknown_fields turns it into a
// loud parse error at startup instead. Safe here because `TlsConfig` is embedded
// as a plain `Option<TlsConfig>`, never `#[serde(flatten)]`ed (which is the one
// case that would misbehave with this attribute).
#[serde(deny_unknown_fields)]
pub struct TlsConfig {
    /// Enable TLS (default: true when section is present)
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Path to PEM-encoded certificate chain
    pub cert_path: PathBuf,

    /// Path to PEM-encoded private key
    pub key_path: PathBuf,

    /// Path to a PEM-encoded CA bundle used to verify client certificates
    /// (mutual TLS). When present, the server requests a client certificate
    /// during the handshake and validates it against these roots. When absent
    /// (the default), no client certificate is requested.
    #[serde(default)]
    pub client_ca_path: Option<PathBuf>,

    /// When `true`, a client certificate is requested but not required:
    /// connections without one still complete, and a presented certificate is
    /// still verified against `client_ca_path`. When `false` (the default), a
    /// certificate that validates against `client_ca_path` is mandatory and
    /// the handshake fails without one. Ignored when `client_ca_path` is absent.
    #[serde(default = "default_false")]
    pub client_auth_optional: bool,

    /// Poll the credential files this often, in seconds, and reload them when
    /// their contents change. `None` (the default) disables polling.
    ///
    /// Change is detected by hashing the file contents, not by comparing
    /// modification times: `cp -p` and most certificate-management tools
    /// preserve mtimes, so an mtime check would silently miss real rotations.
    /// A tick whose files are missing, unreadable or half-written is logged and
    /// retried on the next tick rather than being treated as a rotation, so an
    /// in-progress write heals itself without operator involvement.
    ///
    /// Only meaningful for credentials this service loaded from disk. A
    /// caller-injected [`crate::tls::TlsConfigSource`] built from an
    /// already-loaded `ServerConfig` has no files to reread; configuring an
    /// interval for one is reported at `WARN` and ignored.
    ///
    /// An interval of `0` is rejected at build time rather than spinning.
    #[serde(default)]
    pub reload_interval_secs: Option<u64>,

    /// Reload the credential files when the process receives `SIGHUP`
    /// (default: `false`).
    ///
    /// Enabling this on *either* the `[tls]` or `[grpc.tls]` section installs a
    /// single handler that reloads **every** reloadable source, HTTP and gRPC
    /// alike. One signal reloading only half the listeners would be a confusing
    /// state to reason about during an incident, so the signal is deliberately
    /// all-or-nothing.
    ///
    /// Unix only. On other platforms a configured value is reported at `WARN`
    /// during startup and otherwise ignored.
    #[serde(default = "default_false")]
    pub reload_on_sighup: bool,

    /// Maximum time to wait for a TLS handshake to complete, in seconds, before
    /// the connection is dropped. Caps pre-handshake stalls from unauthenticated
    /// peers (a peer that connects but never completes the handshake). `None`
    /// (the default) uses the built-in default of
    /// [`DEFAULT_HANDSHAKE_TIMEOUT`](crate::tls::DEFAULT_HANDSHAKE_TIMEOUT)
    /// seconds.
    ///
    /// A value of `0` is rejected at build time: it would fail every handshake
    /// instantly.
    #[serde(default)]
    pub handshake_timeout_secs: Option<u64>,
}

/// Caller authorization for mutual-TLS deployments (requires `tls` feature)
///
/// `[tls].client_ca_path` decides whose certificates are *accepted*; this
/// section decides which of those callers may *proceed*. Without it, every
/// principal the configured CA has ever issued to is admitted identically.
///
/// ```toml
/// [caller_auth]
/// mode = "mtls"                 # bearer | mtls | mtls-or-bearer
/// allowlist = [
///   "spiffe://cluster.local/ns/prod/sa/ingest",
///   "reporter.internal",
/// ]
/// ```
///
/// Entries are matched byte-exactly against the DNS and URI `subjectAltName`
/// values of the caller's leaf certificate. An entry containing `://` is
/// treated as a URI SAN, anything else as a DNS SAN; there is no wildcard or
/// suffix matching.
///
/// Combinations that would look like protection without being it are refused
/// at startup rather than accepted: a certificate mode with no
/// `client_ca_path` on the listener it guards, an allowlist under
/// `mode = "bearer"` where nothing would consult it, and an empty allowlist.
///
/// See [`crate::caller_auth`] for the model and the layer that applies it.
#[cfg(feature = "tls")]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
// Reject unknown keys: a typo like `allow_list` would otherwise be silently
// ignored, leaving a service that looks allowlisted and admits everyone.
#[serde(deny_unknown_fields)]
pub struct CallerAuthConfig {
    /// What a caller must present. Defaults to
    /// [`CallerAuthMode::Bearer`](crate::caller_auth::CallerAuthMode::Bearer),
    /// which leaves caller authorization inert.
    #[serde(default)]
    pub mode: crate::caller_auth::CallerAuthMode,

    /// The callers permitted to proceed, by `subjectAltName`.
    ///
    /// Required by, and only meaningful for, the certificate modes.
    #[serde(default)]
    pub allowlist: Vec<String>,

    /// Path prefixes exempt from caller authorization, in addition to the
    /// built-in infrastructure paths (`/health`, `/ready`, `/swagger-ui` and
    /// `/api-docs` on HTTP; the gRPC health and reflection services on gRPC).
    #[serde(default)]
    pub public_paths: Vec<String>,
}

#[cfg(feature = "tls")]
impl CallerAuthConfig {
    /// Build the policy this section describes.
    ///
    /// Pure: derives the policy solely from `self`. Cross-section checks that
    /// need the TLS configuration live in
    /// [`crate::caller_auth::validate_listener`].
    ///
    /// # Errors
    ///
    /// Returns [`CallerAuthConfigError`](crate::caller_auth::CallerAuthConfigError)
    /// when the section cannot describe a policy that does what it appears to:
    /// an allowlist under `mode = "bearer"`, a certificate mode with an empty
    /// allowlist, or an entry that cannot name a caller.
    pub fn to_policy(
        &self,
    ) -> std::result::Result<
        crate::caller_auth::CallerAuthPolicy,
        crate::caller_auth::CallerAuthConfigError,
    > {
        use crate::caller_auth::{CallerAllowlist, CallerAuthConfigError, CallerAuthMode};

        let policy = match self.mode {
            CallerAuthMode::Bearer => {
                if !self.allowlist.is_empty() {
                    return Err(CallerAuthConfigError::AllowlistWithoutCertificateMode);
                }
                crate::caller_auth::CallerAuthPolicy::bearer()
            }
            CallerAuthMode::Mtls => crate::caller_auth::CallerAuthPolicy::mtls(
                CallerAllowlist::from_entries(&self.allowlist)?,
            ),
            CallerAuthMode::MtlsOrBearer => crate::caller_auth::CallerAuthPolicy::mtls_or_bearer(
                CallerAllowlist::from_entries(&self.allowlist)?,
            ),
        };

        Ok(policy.with_public_paths(self.public_paths.clone()))
    }
}

/// Client-side mutual-TLS identity (requires `tls` feature)
///
/// Describes the certificate this service presents *as a client* when it calls
/// another mutual-TLS service, and the trust anchors it uses to verify that
/// peer. It is the outbound mirror of [`TlsConfig`], which describes the
/// certificate this service presents as a server.
///
/// Deliberately not a field of [`Config`]. A service commonly calls several
/// peers, and those calls may legitimately use different identities or trust
/// different roots, so a single framework-level slot would be the wrong shape.
/// Deserialize one of these per peer from wherever the peer is configured, and
/// build a [`crate::client_tls::ClientIdentitySource`] from each.
#[cfg(feature = "tls")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientIdentityConfig {
    /// Enable the client identity (default: true when the section is present).
    ///
    /// This flag is advisory: the loaders in [`crate::client_tls`] do not
    /// consult it, because a caller that asked for a client identity and got a
    /// plain client back would silently lose its authentication. Branch on it
    /// yourself before choosing to build an identity-bearing client at all.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Path to this service's PEM-encoded client certificate chain, leaf first.
    pub cert_path: PathBuf,

    /// Path to this service's PEM-encoded client private key.
    pub key_path: PathBuf,

    /// Path to a PEM-encoded CA bundle used to verify the *peer's* server
    /// certificate.
    ///
    /// Set this when calling a peer whose server certificate is issued by a
    /// private CA that the public web PKI does not chain to, which is the
    /// normal case for internal mutual-TLS meshes. When absent (the default),
    /// the peer is verified against the built-in web PKI roots alone.
    #[serde(default)]
    pub root_ca_path: Option<PathBuf>,

    /// Whether [`root_ca_path`](Self::root_ca_path) *replaces* the built-in web
    /// PKI roots rather than adding to them.
    ///
    /// Defaults to `false`: the private CA is added alongside the public roots,
    /// matching `reqwest`'s `add_root_certificate` semantics. That default
    /// exists because mixed peer sets are ordinary — one client often calls
    /// both an internal mesh service and a publicly-signed endpoint — and a
    /// replace-by-default would break the public calls the moment a private CA
    /// was configured.
    ///
    /// # Security tradeoff
    ///
    /// The default is the permissive one. With `false`, any certificate that
    /// chains to *any* public root is accepted for the peer, so a mis-issued
    /// public certificate for the peer's hostname would be trusted even though
    /// the peer is only ever supposed to present a private-CA certificate. Set
    /// this to `true` for a client that talks exclusively to a private mesh:
    /// it pins trust to your own CA and removes the entire public root set from
    /// the attack surface. Do not set it for a client that also calls
    /// publicly-signed endpoints, because those calls will then fail to verify.
    ///
    /// Ignored when `root_ca_path` is absent: dropping the built-in roots
    /// without supplying replacements would leave nothing to verify against.
    #[serde(default = "default_false")]
    pub exclusive_roots: bool,

    /// Seconds to allow for the TCP connect plus the TLS handshake when opening
    /// a connection to a peer.
    ///
    /// Bounds the phase before any request is sent. A peer that completes the
    /// TCP connect and then stalls the handshake would otherwise hold the
    /// attempt open indefinitely — the OS bounds the connect phase loosely and
    /// nothing bounds the handshake at all.
    ///
    /// Unset resolves to
    /// [`DEFAULT_CLIENT_CONNECT_TIMEOUT`](crate::client_tls::DEFAULT_CLIENT_CONNECT_TIMEOUT).
    /// The default is deliberately generous, so a legitimately slow link is not
    /// clipped; it exists to stop an indefinite stall, not to enforce latency.
    /// A per-RPC deadline is still the caller's to set.
    ///
    /// This is the client-side counterpart to the listener's
    /// `handshake_timeout_secs`.
    #[serde(default)]
    pub connect_timeout_secs: Option<u64>,
}

/// Journald logging configuration (requires `journald` feature)
///
/// When enabled, tracing events are written directly to the systemd journal
/// with native structured fields instead of embedding JSON strings.
#[cfg(feature = "journald")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournaldConfig {
    /// Enable journald output (default: true when section is present)
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Syslog identifier for `journalctl -t <identifier>`
    /// Defaults to the service name
    #[serde(default)]
    pub syslog_identifier: Option<String>,

    /// Field prefix for user-defined fields (default: "F" per tracing-journald)
    /// Set to empty string to disable prefixing
    #[serde(default)]
    pub field_prefix: Option<String>,

    /// Disable the JSON fmt layer when journald is active
    /// Prevents double output on systemd systems where stdout goes to journal
    #[serde(default = "default_false")]
    pub disable_fmt_layer: bool,
}

/// Security headers configuration
///
/// Controls HTTP security headers (HSTS, X-Content-Type-Options, etc.).
/// No feature gate required -- uses existing `tower-http` `set-header`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityHeadersConfig {
    /// Enable security headers middleware (default: true)
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Send Strict-Transport-Security header (only when TLS is active)
    #[serde(default = "default_true")]
    pub hsts: bool,

    /// HSTS max-age in seconds (default: 63072000 = 2 years, OWASP recommendation)
    #[serde(default = "default_hsts_max_age")]
    pub hsts_max_age_secs: u64,

    /// Include subdomains in HSTS
    #[serde(default = "default_false")]
    pub hsts_include_subdomains: bool,

    /// Add HSTS preload flag
    #[serde(default = "default_false")]
    pub hsts_preload: bool,

    /// Send X-Content-Type-Options: nosniff
    #[serde(default = "default_true")]
    pub x_content_type_options: bool,

    /// X-Frame-Options value (default: "DENY")
    #[serde(default = "default_x_frame_options")]
    pub x_frame_options: String,

    /// Send X-XSS-Protection: 0 (modern recommendation: disable browser XSS filter)
    #[serde(default = "default_true")]
    pub x_xss_protection: bool,

    /// Referrer-Policy value (default: "strict-origin-when-cross-origin")
    #[serde(default = "default_referrer_policy")]
    pub referrer_policy: String,

    /// Permissions-Policy header value (optional, user-configured)
    #[serde(default)]
    pub permissions_policy: Option<String>,
}

impl Default for SecurityHeadersConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            hsts: true,
            hsts_max_age_secs: default_hsts_max_age(),
            hsts_include_subdomains: false,
            hsts_preload: false,
            x_content_type_options: true,
            x_frame_options: default_x_frame_options(),
            x_xss_protection: true,
            referrer_policy: default_referrer_policy(),
            permissions_policy: None,
        }
    }
}

/// Middleware configuration (all optional, feature-gated)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiddlewareConfig {
    /// Request tracking configuration (request IDs, header propagation)
    #[serde(default)]
    pub request_tracking: RequestTrackingConfig,

    /// Resilience configuration (circuit breaker, retry, bulkhead)
    #[serde(default)]
    pub resilience: Option<ResilienceConfig>,

    /// HTTP metrics configuration (OpenTelemetry)
    #[serde(default)]
    pub metrics: Option<MetricsConfig>,

    /// Local rate limiting configuration (governor)
    #[serde(default)]
    pub governor: Option<LocalRateLimitConfig>,

    /// Request body size limit in MB
    #[serde(default = "default_body_limit_mb")]
    pub body_limit_mb: usize,

    /// Enable panic recovery middleware
    #[serde(default = "default_true")]
    pub catch_panic: bool,

    /// Enable compression
    #[serde(default = "default_true")]
    pub compression: bool,

    /// CORS configuration
    #[serde(default = "default_cors_mode")]
    pub cors_mode: String,

    /// Security headers configuration (HSTS, X-Content-Type-Options, etc.)
    #[serde(default)]
    pub security_headers: SecurityHeadersConfig,
}

impl Default for MiddlewareConfig {
    fn default() -> Self {
        Self {
            request_tracking: RequestTrackingConfig::default(),
            resilience: None,
            metrics: None,
            governor: None,
            body_limit_mb: default_body_limit_mb(),
            catch_panic: true,
            compression: true,
            cors_mode: default_cors_mode(),
            security_headers: SecurityHeadersConfig::default(),
        }
    }
}

/// Request tracking configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestTrackingConfig {
    /// Enable request ID generation
    #[serde(default = "default_true")]
    pub request_id_enabled: bool,

    /// Request ID header name
    #[serde(default = "default_request_id_header")]
    pub request_id_header: String,

    /// Enable header propagation
    #[serde(default = "default_true")]
    pub propagate_headers: bool,

    /// Enable sensitive header masking in logs
    #[serde(default = "default_true")]
    pub mask_sensitive_headers: bool,
}

impl Default for RequestTrackingConfig {
    fn default() -> Self {
        Self {
            request_id_enabled: true,
            request_id_header: default_request_id_header(),
            propagate_headers: true,
            mask_sensitive_headers: true,
        }
    }
}

/// Resilience configuration (circuit breaker, retry, bulkhead)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResilienceConfig {
    /// Enable circuit breaker
    #[serde(default = "default_true")]
    pub circuit_breaker_enabled: bool,

    /// Failure threshold before circuit opens (0.0-1.0)
    #[serde(default = "default_circuit_breaker_threshold")]
    pub circuit_breaker_threshold: f64,

    /// Minimum requests before calculating failure rate
    #[serde(default = "default_circuit_breaker_min_requests")]
    pub circuit_breaker_min_requests: u64,

    /// Duration to wait before attempting to close circuit (seconds)
    #[serde(default = "default_circuit_breaker_wait_secs")]
    pub circuit_breaker_wait_secs: u64,

    /// Enable bulkhead (concurrency limiting)
    #[serde(default = "default_true")]
    pub bulkhead_enabled: bool,

    /// Maximum concurrent requests
    #[serde(default = "default_bulkhead_max_concurrent")]
    pub bulkhead_max_concurrent: usize,

    /// Maximum time a request waits for a bulkhead slot (milliseconds)
    #[serde(default = "default_bulkhead_max_wait_ms")]
    pub bulkhead_max_wait_ms: u64,
}

impl ResilienceConfig {
    /// Convert to Duration types for runtime use
    pub fn circuit_breaker_wait_duration(&self) -> Duration {
        Duration::from_secs(self.circuit_breaker_wait_secs)
    }

    /// Maximum time a request waits for a bulkhead slot.
    pub fn bulkhead_max_wait(&self) -> Duration {
        Duration::from_millis(self.bulkhead_max_wait_ms)
    }
}

/// HTTP metrics configuration (OpenTelemetry)
///
/// Every key here reaches the instrumentation. `deny_unknown_fields` is what
/// keeps that true: this table once accepted `include_path`, `include_method`,
/// `include_status` and `service_name`, parsed them, and built the layer
/// without them, so a config could ask for something and be told nothing. A key
/// that cannot be honoured must fail at startup, not be quietly dropped.
///
/// The three `include_*` keys are gone rather than implemented. The metrics
/// layer emits `http.request.method`, `http.route` and
/// `http.response.status_code` because the OpenTelemetry semantic conventions
/// require them; suppressing them would produce a metric that is no longer the
/// metric its name claims. `service_name` was never a key of this table -- the
/// resource-level `service.name` comes from `[service] name`, which is where a
/// service is named once for traces, metrics and logs alike.
///
/// The one nested table, `exporter`, is a real key that reaches a real
/// listener; see [`MetricsExporterConfig`].
///
/// `#[non_exhaustive]`, so future keys on this table are additive: construct
/// it with [`MetricsConfig::new`] and the `with_*` setters.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct MetricsConfig {
    /// Enable metrics collection
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Explicit histogram bucket boundaries for HTTP request duration, in
    /// milliseconds.
    ///
    /// The instrument itself is in seconds, per semantic convention; these are
    /// divided by 1000 on the way in. Boundaries must be finite, positive and
    /// strictly increasing -- anything else is reported and ignored in favour
    /// of the default, because a mis-bucketed metric must not take a service
    /// down at boot.
    #[serde(default = "default_latency_buckets")]
    pub latency_buckets_ms: Vec<f64>,

    /// Optional plaintext exporter listener. Absent means no second socket.
    ///
    /// The one nested table this section permits; its own keys are as strict
    /// as this table's.
    #[serde(default)]
    pub exporter: Option<MetricsExporterConfig>,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            latency_buckets_ms: default_latency_buckets(),
            exporter: None,
        }
    }
}

impl MetricsConfig {
    /// Create a metrics configuration carrying the defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether the HTTP metrics layer is installed.
    #[must_use]
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set the latency histogram boundaries, in milliseconds.
    #[must_use]
    pub fn with_latency_buckets_ms(mut self, buckets_ms: Vec<f64>) -> Self {
        self.latency_buckets_ms = buckets_ms;
        self
    }

    /// Install a plaintext exporter listener serving only `GET /metrics`.
    #[must_use]
    pub fn with_exporter(mut self, exporter: MetricsExporterConfig) -> Self {
        self.exporter = Some(exporter);
        self
    }

    /// The configured boundaries in seconds, the unit the instrument declares.
    ///
    /// Returns `None` when the list is empty or not strictly increasing through
    /// finite, positive values; the caller falls back to the instrumentation
    /// library's own defaults and says so.
    pub fn latency_buckets_seconds(&self) -> Option<Vec<f64>> {
        let usable = !self.latency_buckets_ms.is_empty()
            && self
                .latency_buckets_ms
                .iter()
                .all(|ms| ms.is_finite() && *ms > 0.0)
            && self
                .latency_buckets_ms
                .windows(2)
                .all(|pair| pair[0] < pair[1]);

        usable.then(|| {
            self.latency_buckets_ms
                .iter()
                .map(|ms| ms / 1000.0)
                .collect()
        })
    }
}

/// A dedicated, plaintext listener that serves only `GET /metrics`.
///
/// Absent table means absent listener: nothing changes for a deployment that
/// does not write `[middleware.metrics.exporter]`. When present, the service
/// binds one additional TCP socket at `bind:port` and serves the same
/// Prometheus document as the main listener's `/metrics` route, rendered from
/// the same registry. Every other path is 404.
///
/// This listener speaks HTTP only -- no TLS, no auth, no other middleware --
/// which is the point: managed platform collectors (Fly.io `[[metrics]]`, GKE
/// managed collection, most `PodMonitor`/`ServiceMonitor` defaults) scrape
/// plain HTTP against a declared port and offer no TLS knobs, so a service
/// whose main listener is TLS cannot be scraped through it. Bind this to a
/// private scrape network, never an internet-facing interface.
///
/// Both keys are required. A plaintext socket is a security-relevant act, so
/// there is no default address to open unasked.
///
/// `#[non_exhaustive]` from birth, so future keys on this table are additive:
/// construct it with [`MetricsExporterConfig::new`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct MetricsExporterConfig {
    /// IP address the exporter listener binds. Required; there is no default.
    ///
    /// `::` is dual-stack on Linux by OS default and v6-only on some BSDs.
    pub bind: IpAddr,

    /// TCP port the exporter listener binds. Required; there is no default.
    ///
    /// Must not be `0` and must not collide with the HTTP or gRPC listener.
    pub port: u16,
}

impl MetricsExporterConfig {
    /// Create an exporter configuration for the given address.
    pub fn new(bind: IpAddr, port: u16) -> Self {
        Self { bind, port }
    }

    /// The address the exporter listener binds.
    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.bind, self.port)
    }
}

/// Local rate limiting configuration (governor-based)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalRateLimitConfig {
    /// Enable local rate limiting
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Maximum requests per period
    #[serde(default = "default_governor_requests")]
    pub requests_per_period: u32,

    /// Time period in seconds
    #[serde(default = "default_governor_period_secs")]
    pub period_secs: u64,

    /// Burst size (allow temporary spikes)
    #[serde(default = "default_governor_burst")]
    pub burst_size: u32,
}

impl LocalRateLimitConfig {
    pub fn period(&self) -> Duration {
        Duration::from_secs(self.period_secs)
    }
}

// Default value functions
fn default_bind() -> IpAddr {
    IpAddr::V4(Ipv4Addr::UNSPECIFIED) // 0.0.0.0 (all interfaces)
}

fn default_port() -> u16 {
    8080
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_timeout() -> u64 {
    30
}

fn default_environment() -> String {
    "dev".to_string()
}

#[cfg(feature = "jwt")]
fn default_jwt_algorithm() -> String {
    "RS256".to_string()
}

fn default_per_user_rpm() -> u32 {
    200
}

fn default_per_client_rpm() -> u32 {
    1000
}

fn default_window_secs() -> u64 {
    60
}

fn default_route_burst_size() -> u32 {
    10 // 10% burst allowance by default
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

fn default_redis_max_connections() -> usize {
    20
}

fn default_max_reconnects() -> usize {
    10
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
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

#[cfg(feature = "surrealdb")]
fn default_surrealdb_namespace() -> String {
    "default".to_string()
}

#[cfg(feature = "surrealdb")]
fn default_surrealdb_database() -> String {
    "default".to_string()
}

// Security headers default functions
fn default_hsts_max_age() -> u64 {
    63_072_000 // 2 years (OWASP recommendation)
}

fn default_x_frame_options() -> String {
    "DENY".to_string()
}

fn default_referrer_policy() -> String {
    "strict-origin-when-cross-origin".to_string()
}

// Middleware default functions
fn default_body_limit_mb() -> usize {
    10 // 10 MB
}

fn default_cors_mode() -> String {
    "restrictive".to_string()
}

fn default_request_id_header() -> String {
    "x-request-id".to_string()
}

// Resilience default functions
fn default_circuit_breaker_threshold() -> f64 {
    0.5 // 50% failure rate
}

fn default_circuit_breaker_min_requests() -> u64 {
    10
}

fn default_circuit_breaker_wait_secs() -> u64 {
    30
}

fn default_bulkhead_max_concurrent() -> usize {
    100
}

fn default_bulkhead_max_wait_ms() -> u64 {
    5000 // 5 seconds
}

// Metrics default functions
/// The seconds-histogram boundaries this service already applies, in
/// milliseconds.
///
/// Deliberately the same set as `observability::SECONDS_HISTOGRAM_BOUNDARIES`,
/// which a view applies to every seconds-valued histogram. Before this table
/// was honoured, that view is what the HTTP duration histogram actually got;
/// keeping the default identical means turning the key on changes nothing until
/// an operator sets it, and no dashboard moves underneath one who does not. A
/// test pins the two lists together.
fn default_latency_buckets() -> Vec<f64> {
    vec![
        1.0, 2.5, 5.0, 10.0, 25.0, 50.0, 75.0, 100.0, 250.0, 500.0, 750.0, 1000.0, 2500.0, 5000.0,
        7500.0, 10000.0,
    ]
}

// Governor default functions
fn default_governor_requests() -> u32 {
    100
}

fn default_governor_period_secs() -> u64 {
    60
}

fn default_governor_burst() -> u32 {
    10
}

// gRPC default functions
fn default_grpc_port() -> u16 {
    9090
}

fn default_grpc_max_message_mb() -> usize {
    4 // 4 MB
}

fn default_proto_dir() -> String {
    "proto".to_string()
}

// Cedar default functions
#[cfg(feature = "cedar-authz")]
fn default_cedar_hot_reload_interval() -> u64 {
    60 // Check every 60 seconds
}

#[cfg(feature = "cedar-authz")]
fn default_cedar_policy_cache_ttl() -> u64 {
    300 // Cache for 5 minutes
}

#[cfg(unix)]
fn platform_user_config_root(
    xdg_config_home: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    xdg_config_home
        .filter(|path| path.is_absolute())
        .or_else(|| home.map(|path| path.join(".config")))
}

#[cfg(windows)]
fn platform_user_config_root(
    app_data: Option<PathBuf>,
    profile: Option<PathBuf>,
) -> Option<PathBuf> {
    app_data.or_else(|| profile.map(|path| path.join("AppData").join("Roaming")))
}

#[cfg(unix)]
fn user_config_root() -> Option<PathBuf> {
    platform_user_config_root(
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

#[cfg(windows)]
fn user_config_root() -> Option<PathBuf> {
    platform_user_config_root(
        std::env::var_os("APPDATA").map(PathBuf::from),
        std::env::var_os("USERPROFILE").map(PathBuf::from),
    )
}

#[cfg(not(any(unix, windows)))]
fn user_config_root() -> Option<PathBuf> {
    None
}

fn user_config_path(service_name: &str) -> Option<PathBuf> {
    user_config_root().map(|root| {
        root.join("acton-service")
            .join(service_name)
            .join("config.toml")
    })
}

#[cfg(unix)]
fn system_config_path(service_name: &str) -> Option<PathBuf> {
    Some(
        PathBuf::from("/etc/acton-service")
            .join(service_name)
            .join("config.toml"),
    )
}

#[cfg(windows)]
fn system_config_path(service_name: &str) -> Option<PathBuf> {
    std::env::var_os("PROGRAMDATA").map(|root| {
        PathBuf::from(root)
            .join("acton-service")
            .join(service_name)
            .join("config.toml")
    })
}

#[cfg(not(any(unix, windows)))]
fn system_config_path(_service_name: &str) -> Option<PathBuf> {
    None
}

impl<T> Config<T>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    /// Load configuration from all sources
    ///
    /// Searches for config files in this order (first found is used):
    /// 1. Current working directory: ./config.toml
    /// 2. Platform user config directory (`XDG_CONFIG_HOME` or `APPDATA`)
    /// 3. Platform system config directory (`/etc` or `PROGRAMDATA`)
    ///
    /// Environment variables (ACTON_ prefix) override all file-based configs.
    ///
    /// Both framework config and custom config (type T) are loaded from the same config.toml.
    pub fn load() -> Result<Self> {
        // Try to infer service name from binary name or use default
        let service_name = std::env::current_exe()
            .ok()
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "acton-service".to_string());

        Self::load_for_service(&service_name)
    }

    /// Load configuration for a specific service name
    ///
    /// This is the recommended way to load config in production.
    pub fn load_for_service(service_name: &str) -> Result<Self> {
        let config_paths = Self::find_config_paths(service_name);

        // Log which config paths we're checking
        tracing::debug!("Searching for config files in order:");
        for path in &config_paths {
            tracing::debug!("  - {}", path.display());
        }

        let mut figment = Figment::new()
            // Start with defaults
            .merge(Serialized::defaults(Config::<T>::default()));

        // Merge config files in reverse order (lowest priority first)
        // so that higher priority files override lower ones
        for path in config_paths.iter().rev() {
            if path.exists() {
                tracing::info!("Loading configuration from: {}", path.display());
                figment = figment.merge(Toml::file(path));
            }
        }

        // Environment variables have highest priority
        figment = figment.merge(Env::prefixed("ACTON_").split("_"));

        let config = figment.extract()?;
        Ok(config)
    }

    /// Load configuration from a specific file
    ///
    /// This bypasses XDG directories and loads directly from the given path.
    /// Useful for testing or non-standard deployments.
    pub fn load_from(path: &str) -> Result<Self> {
        let config = Figment::new()
            // Start with defaults
            .merge(Serialized::defaults(Config::<T>::default()))
            // Load from config file (if exists)
            .merge(Toml::file(path))
            // Override with environment variables
            .merge(Env::prefixed("ACTON_").split("_"))
            .extract()?;

        Ok(config)
    }

    /// Find all possible config file paths for a service
    ///
    /// Returns paths in priority order (highest first):
    /// 1. Current working directory
    /// 2. User config directory
    /// 3. System directory
    fn find_config_paths(service_name: &str) -> Vec<PathBuf> {
        let mut paths = Vec::new();

        // 1. Current working directory (highest priority for dev/testing)
        paths.push(PathBuf::from("config.toml"));

        // 2. Platform user config directory. Only include an existing file so
        // loading configuration remains side-effect free.
        if let Some(path) = user_config_path(service_name).filter(|path| path.is_file()) {
            paths.push(path);
        }

        // 3. Platform system-wide directory.
        if let Some(path) = system_config_path(service_name) {
            paths.push(path);
        }

        paths
    }

    /// Get the recommended config path for a service
    ///
    /// This is where the config file should be placed in production.
    /// On Unix this follows `XDG_CONFIG_HOME` and falls back to
    /// `~/.config`. On Windows it follows `APPDATA` and falls back to
    /// `%USERPROFILE%\AppData\Roaming`.
    pub fn recommended_path(service_name: &str) -> PathBuf {
        user_config_path(service_name).unwrap_or_else(|| {
            PathBuf::from("acton-service")
                .join(service_name)
                .join("config.toml")
        })
    }

    /// Create the config directory structure for a service
    ///
    /// Creates the platform user configuration directory if it doesn't exist.
    pub fn create_config_dir(service_name: &str) -> Result<PathBuf> {
        let config_path = Self::recommended_path(service_name);
        let config_dir = config_path
            .parent()
            .ok_or_else(|| crate::error::Error::Internal("Invalid config path".to_string()))?;

        std::fs::create_dir_all(config_dir).map_err(|e| {
            crate::error::Error::Internal(format!(
                "Failed to create config directory {}: {e}",
                config_dir.display()
            ))
        })?;

        Ok(config_dir.to_path_buf())
    }

    /// Get database URL
    pub fn database_url(&self) -> Option<&str> {
        self.database.as_ref().map(|db| db.url.as_str())
    }

    /// Get Redis URL
    pub fn redis_url(&self) -> Option<&str> {
        self.redis.as_ref().map(|r| r.url.as_str())
    }

    /// Get NATS URL
    pub fn nats_url(&self) -> Option<&str> {
        self.nats.as_ref().map(|n| n.url.as_str())
    }

    /// Get Turso remote URL
    #[cfg(feature = "turso")]
    pub fn turso_url(&self) -> Option<&str> {
        self.turso.as_ref().and_then(|t| t.url.as_deref())
    }

    /// Get SurrealDB URL
    #[cfg(feature = "surrealdb")]
    pub fn surrealdb_url(&self) -> Option<&str> {
        self.surrealdb.as_ref().map(|s| s.url.as_str())
    }

    /// Enable permissive CORS for local development
    ///
    /// ⚠️  **WARNING: DO NOT USE IN PRODUCTION** ⚠️
    ///
    /// This enables permissive CORS that allows:
    /// - All origins (*)
    /// - All methods (GET, POST, PUT, DELETE, etc.)
    /// - All headers
    /// - Credentials from any origin
    ///
    /// This configuration is appropriate ONLY for:
    /// - Local development environments
    /// - Testing with frontend dev servers (e.g., webpack-dev-server, vite)
    /// - Prototyping where security is not a concern
    ///
    /// For production, you should:
    /// - Use the default restrictive CORS (secure by default)
    /// - Configure specific allowed origins in your config file
    /// - Set ACTON_MIDDLEWARE_CORS_MODE=restrictive
    ///
    /// # Example
    /// ```no_run
    /// use acton_service::prelude::Config;
    ///
    /// let mut config = Config::<()>::load().unwrap();
    /// config.with_development_cors(); // Only for local development!
    /// ```
    pub fn with_development_cors(&mut self) -> &mut Self {
        tracing::warn!(
            "⚠️  CORS set to permissive mode - DO NOT USE IN PRODUCTION! \
             This allows any origin to access your API. \
             Use only for local development."
        );
        self.middleware.cors_mode = "permissive".to_string();
        self
    }
}

impl<T> Default for Config<T>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    fn default() -> Self {
        Self {
            service: ServiceConfig {
                name: "acton-service".to_string(),
                bind: default_bind(),
                port: default_port(),
                log_level: default_log_level(),
                timeout_secs: default_timeout(),
                environment: default_environment(),
                trust_forwarded_headers: false,
            },
            token: None,
            rate_limit: RateLimitConfig::default(),
            middleware: MiddlewareConfig::default(),
            database: None,
            #[cfg(feature = "turso")]
            turso: None,
            #[cfg(feature = "surrealdb")]
            surrealdb: None,
            redis: None,
            nats: None,
            #[cfg(feature = "clickhouse")]
            clickhouse: None,
            otlp: None,
            grpc: None,
            #[cfg(feature = "websocket")]
            websocket: None,
            #[cfg(feature = "cedar-authz")]
            cedar: None,
            #[cfg(feature = "graphql")]
            graphql: None,
            #[cfg(feature = "session")]
            session: None,
            #[cfg(feature = "audit")]
            audit: None,
            #[cfg(feature = "auth")]
            auth: None,
            #[cfg(feature = "login-lockout")]
            lockout: None,
            #[cfg(feature = "tls")]
            tls: None,
            #[cfg(feature = "tls")]
            caller_auth: None,
            #[cfg(feature = "journald")]
            journald: None,
            #[cfg(feature = "accounts")]
            accounts: None,
            background_worker: None,
            custom: T::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[cfg(unix)]
    #[test]
    fn user_config_root_prefers_an_absolute_xdg_path() {
        let root = platform_user_config_root(
            Some(PathBuf::from("/var/lib/example-config")),
            Some(PathBuf::from("/home/example")),
        );

        assert_eq!(root, Some(PathBuf::from("/var/lib/example-config")));
    }

    #[cfg(unix)]
    #[test]
    fn user_config_root_ignores_a_relative_xdg_path() {
        let root = platform_user_config_root(
            Some(PathBuf::from("relative/config")),
            Some(PathBuf::from("/home/example")),
        );

        assert_eq!(root, Some(PathBuf::from("/home/example/.config")));
    }

    #[cfg(windows)]
    #[test]
    fn user_config_root_prefers_app_data() {
        let root = platform_user_config_root(
            Some(PathBuf::from(r"C:\Users\example\AppData\Roaming")),
            Some(PathBuf::from(r"C:\Users\example")),
        );

        assert_eq!(
            root,
            Some(PathBuf::from(r"C:\Users\example\AppData\Roaming"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn user_config_root_falls_back_to_the_user_profile() {
        let root = platform_user_config_root(None, Some(PathBuf::from(r"C:\Users\example")));

        assert_eq!(
            root,
            Some(PathBuf::from(r"C:\Users\example\AppData\Roaming"))
        );
    }

    #[test]
    fn test_default_config() {
        let config = Config::<()>::default();
        assert_eq!(config.service.port, 8080);
        assert_eq!(config.service.log_level, "info");
        assert_eq!(config.rate_limit.per_user_rpm, 200);
    }

    #[cfg(feature = "tls")]
    #[test]
    fn tls_config_client_auth_fields_default_to_no_mtls() {
        let json = r#"{
            "enabled": true,
            "cert_path": "/etc/tls/cert.pem",
            "key_path": "/etc/tls/key.pem"
        }"#;

        let tls: TlsConfig = serde_json::from_str(json).expect("minimal TLS config must parse");

        assert!(
            tls.client_ca_path.is_none(),
            "an absent client CA must default to no mutual TLS"
        );
        assert!(
            !tls.client_auth_optional,
            "client auth must default to required when a CA is configured"
        );
    }

    #[cfg(feature = "tls")]
    #[test]
    fn caller_auth_config_parses_modes_and_allowlist() {
        let json = r#"{
            "mode": "mtls-or-bearer",
            "allowlist": ["spiffe://cluster.local/ns/prod/sa/ingest", "reporter.internal"],
            "public_paths": ["/metrics"]
        }"#;

        let caller_auth: CallerAuthConfig =
            serde_json::from_str(json).expect("caller auth config must parse");

        assert_eq!(
            caller_auth.mode,
            crate::caller_auth::CallerAuthMode::MtlsOrBearer
        );

        let policy = caller_auth.to_policy().expect("valid policy");
        let allowlist = policy.allowlist().expect("certificate mode carries a list");
        assert!(allowlist.contains(
            &crate::caller_auth::CallerSan::uri("spiffe://cluster.local/ns/prod/sa/ingest")
                .expect("valid")
        ));
        assert!(allowlist
            .contains(&crate::caller_auth::CallerSan::dns("reporter.internal").expect("valid")));
        assert_eq!(policy.public_paths(), ["/metrics"]);
    }

    #[cfg(feature = "tls")]
    #[test]
    fn caller_auth_config_defaults_to_inert_bearer_mode() {
        let caller_auth: CallerAuthConfig =
            serde_json::from_str("{}").expect("an empty section must parse");

        assert_eq!(caller_auth.mode, crate::caller_auth::CallerAuthMode::Bearer);
        let policy = caller_auth.to_policy().expect("valid policy");
        assert!(
            !policy.requires_client_ca(),
            "the default must not silently start demanding certificates"
        );
    }

    #[test]
    fn metrics_config_refuses_keys_it_cannot_honour() {
        // Each of these parsed happily and reached nothing. `include_*` asked
        // the layer to drop semantic-convention attributes it will always
        // record; `service_name` belongs to [service], which names the process
        // once for traces, metrics and logs together.
        for key in [
            r#""include_path": true"#,
            r#""include_method": true"#,
            r#""include_status": true"#,
            r#""service_name": "whatever""#,
        ] {
            let json = format!(r#"{{ "enabled": true, {key} }}"#);

            assert!(
                serde_json::from_str::<MetricsConfig>(&json).is_err(),
                "{key} must fail startup rather than be accepted and ignored"
            );
        }
    }

    #[test]
    fn metrics_config_defaults_to_the_buckets_already_in_effect() {
        let metrics: MetricsConfig =
            serde_json::from_str("{}").expect("every key in the table has a default");

        assert!(metrics.enabled);
        assert_eq!(
            metrics.latency_buckets_ms.first().copied(),
            Some(1.0),
            "the default must start at 1ms, as the seconds view does"
        );
        assert_eq!(
            metrics.latency_buckets_seconds(),
            Some(vec![
                0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1.0, 2.5,
                5.0, 7.5, 10.0
            ]),
            "omitting the key must leave the boundaries exactly where they were \
             before this table was honoured"
        );
        assert!(
            metrics.exporter.is_none(),
            "an absent exporter table must mean an absent listener"
        );
    }

    #[test]
    fn metrics_exporter_table_round_trips_through_figment() {
        // Through Figment rather than serde_json because the real load path
        // seeds `Serialized::defaults(Config::default())` first, and the
        // exporter's `None` default must survive that merge.
        let toml = r#"
[middleware.metrics]
enabled = true

[middleware.metrics.exporter]
bind = "::"
port = 9091
"#;
        let config: Config<()> = Figment::new()
            .merge(Serialized::defaults(Config::<()>::default()))
            .merge(Toml::string(toml))
            .extract()
            .expect("a bind + port exporter table must deserialize");

        let exporter = config
            .middleware
            .metrics
            .as_ref()
            .and_then(|m| m.exporter.as_ref())
            .expect("the exporter table must reach the config");
        assert_eq!(
            exporter.socket_addr(),
            "[::]:9091".parse::<SocketAddr>().expect("valid addr")
        );
    }

    #[test]
    fn metrics_exporter_table_requires_bind() {
        // A defaulted bind would let a typo'd table open an interface unasked.
        // That the omission fails here proves serde enforces it, not a runtime
        // check that could drift.
        let json = r#"{ "port": 9091 }"#;

        assert!(
            serde_json::from_str::<MetricsExporterConfig>(json).is_err(),
            "an exporter table without `bind` must be a parse error"
        );
    }

    #[test]
    fn metrics_exporter_table_refuses_keys_it_cannot_honour() {
        // The parent table's rule is inherited by intent: a key that reaches
        // nothing must fail at startup, not be quietly dropped.
        let json = r#"{ "bind": "127.0.0.1", "port": 9091, "tls": true }"#;

        assert!(
            serde_json::from_str::<MetricsExporterConfig>(json).is_err(),
            "an unknown key in [middleware.metrics.exporter] must be a parse error"
        );
    }

    #[test]
    fn metrics_config_builder_sets_the_exporter() {
        let metrics = MetricsConfig::new().with_exporter(MetricsExporterConfig::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            9091,
        ));

        assert_eq!(
            metrics.exporter,
            Some(MetricsExporterConfig::new(
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                9091
            ))
        );
    }

    #[cfg(feature = "tls")]
    #[test]
    fn caller_auth_config_refuses_a_typo_in_a_key() {
        // `allow_list` instead of `allowlist` would otherwise deserialize to an
        // empty allowlist, leaving a service that looks protected.
        let json = r#"{ "mode": "mtls", "allow_list": ["reporter.internal"] }"#;

        assert!(
            serde_json::from_str::<CallerAuthConfig>(json).is_err(),
            "an unknown key in [caller_auth] must be a parse error"
        );
    }

    #[cfg(feature = "tls")]
    #[test]
    fn caller_auth_config_refuses_combinations_that_only_look_protective() {
        let dead_allowlist = CallerAuthConfig {
            mode: crate::caller_auth::CallerAuthMode::Bearer,
            allowlist: vec!["reporter.internal".to_string()],
            public_paths: Vec::new(),
        };
        assert_eq!(
            dead_allowlist
                .to_policy()
                .expect_err("an allowlist nothing consults must be refused"),
            crate::caller_auth::CallerAuthConfigError::AllowlistWithoutCertificateMode
        );

        let empty_allowlist = CallerAuthConfig {
            mode: crate::caller_auth::CallerAuthMode::Mtls,
            allowlist: Vec::new(),
            public_paths: Vec::new(),
        };
        assert_eq!(
            empty_allowlist
                .to_policy()
                .expect_err("a certificate mode with no allowed callers must be refused"),
            crate::caller_auth::CallerAuthConfigError::EmptyAllowlist
        );
    }

    #[cfg(feature = "tls")]
    #[test]
    fn tls_config_parses_mutual_tls_fields() {
        let json = r#"{
            "enabled": true,
            "cert_path": "/etc/tls/cert.pem",
            "key_path": "/etc/tls/key.pem",
            "client_ca_path": "/etc/tls/client-ca.pem",
            "client_auth_optional": true
        }"#;

        let tls: TlsConfig = serde_json::from_str(json).expect("mutual TLS config must parse");

        assert_eq!(
            tls.client_ca_path.as_deref(),
            Some(std::path::Path::new("/etc/tls/client-ca.pem")),
            "the client CA path must round-trip from configuration"
        );
        assert!(
            tls.client_auth_optional,
            "the optional client-auth flag must round-trip from configuration"
        );
    }

    /// A typo in a `[tls]` key must fail to parse rather than be silently
    /// ignored: `reload_interval_sec` (missing the trailing `s`) would otherwise
    /// quietly disarm certificate rotation. `deny_unknown_fields` turns it into a
    /// startup error.
    #[cfg(feature = "tls")]
    #[test]
    fn tls_config_rejects_an_unknown_field() {
        let json = r#"{
            "enabled": true,
            "cert_path": "/etc/tls/cert.pem",
            "key_path": "/etc/tls/key.pem",
            "reload_interval_sec": 300
        }"#;

        let err = serde_json::from_str::<TlsConfig>(json)
            .expect_err("a misspelled TLS key must be rejected, not silently dropped");
        assert!(
            err.to_string().contains("reload_interval_sec"),
            "the parse error must name the offending key: {err}"
        );
    }

    #[test]
    fn test_default_config_with_unit_type() {
        let config = Config::<()>::default();
        assert_eq!(config.service.port, 8080);
        assert_eq!(config.service.name, "acton-service");
        // config.custom is () - no assertion needed for unit type
    }

    #[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
    struct CustomConfig {
        api_key: String,
        timeout_ms: u32,
        feature_flags: HashMap<String, bool>,
    }

    #[test]
    fn test_config_with_custom_type() {
        let custom = CustomConfig {
            api_key: "test-key-123".to_string(),
            timeout_ms: 5000,
            feature_flags: {
                let mut map = HashMap::new();
                map.insert("new_ui".to_string(), true);
                map.insert("beta_features".to_string(), false);
                map
            },
        };

        let config = Config {
            service: ServiceConfig {
                name: "test-service".to_string(),
                bind: default_bind(),
                port: 9090,
                log_level: "debug".to_string(),
                timeout_secs: 30,
                environment: "test".to_string(),
                trust_forwarded_headers: false,
            },
            token: Some(TokenConfig::Paseto(PasetoConfig {
                version: "v4".to_string(),
                purpose: "local".to_string(),
                key_path: PathBuf::from("./test-key.key"),
                issuer: Some("test-issuer".to_string()),
                audience: None,
                public_paths: Vec::new(),
            })),
            rate_limit: RateLimitConfig {
                per_user_rpm: 100,
                per_client_rpm: 500,
                window_secs: 60,
                routes: std::collections::HashMap::new(),
                auto_apply: true,
                trust_forwarded_headers: false,
            },
            middleware: MiddlewareConfig::default(),
            database: None,
            #[cfg(feature = "turso")]
            turso: None,
            #[cfg(feature = "surrealdb")]
            surrealdb: None,
            redis: None,
            nats: None,
            #[cfg(feature = "clickhouse")]
            clickhouse: None,
            otlp: None,
            grpc: None,
            #[cfg(feature = "websocket")]
            websocket: None,
            #[cfg(feature = "cedar-authz")]
            cedar: None,
            #[cfg(feature = "graphql")]
            graphql: None,
            #[cfg(feature = "session")]
            session: None,
            #[cfg(feature = "audit")]
            audit: None,
            #[cfg(feature = "auth")]
            auth: None,
            #[cfg(feature = "login-lockout")]
            lockout: None,
            #[cfg(feature = "tls")]
            tls: None,
            #[cfg(feature = "tls")]
            caller_auth: None,
            #[cfg(feature = "journald")]
            journald: None,
            #[cfg(feature = "accounts")]
            accounts: None,
            background_worker: None,
            custom,
        };

        assert_eq!(config.service.name, "test-service");
        assert_eq!(config.custom.api_key, "test-key-123");
        assert_eq!(config.custom.timeout_ms, 5000);
        assert_eq!(config.custom.feature_flags.get("new_ui"), Some(&true));
    }

    #[test]
    fn test_config_serialization_with_custom() {
        let custom = CustomConfig {
            api_key: "secret-key".to_string(),
            timeout_ms: 3000,
            feature_flags: HashMap::new(),
        };

        let config = Config {
            service: ServiceConfig {
                name: "test".to_string(),
                bind: default_bind(),
                port: 8080,
                log_level: "info".to_string(),
                timeout_secs: 30,
                environment: "dev".to_string(),
                trust_forwarded_headers: false,
            },
            token: None,
            rate_limit: RateLimitConfig {
                per_user_rpm: 200,
                per_client_rpm: 1000,
                window_secs: 60,
                routes: std::collections::HashMap::new(),
                auto_apply: true,
                trust_forwarded_headers: false,
            },
            middleware: MiddlewareConfig::default(),
            database: None,
            #[cfg(feature = "turso")]
            turso: None,
            #[cfg(feature = "surrealdb")]
            surrealdb: None,
            redis: None,
            nats: None,
            #[cfg(feature = "clickhouse")]
            clickhouse: None,
            otlp: None,
            grpc: None,
            #[cfg(feature = "websocket")]
            websocket: None,
            #[cfg(feature = "cedar-authz")]
            cedar: None,
            #[cfg(feature = "graphql")]
            graphql: None,
            #[cfg(feature = "session")]
            session: None,
            #[cfg(feature = "audit")]
            audit: None,
            #[cfg(feature = "auth")]
            auth: None,
            #[cfg(feature = "login-lockout")]
            lockout: None,
            #[cfg(feature = "tls")]
            tls: None,
            #[cfg(feature = "tls")]
            caller_auth: None,
            #[cfg(feature = "journald")]
            journald: None,
            #[cfg(feature = "accounts")]
            accounts: None,
            background_worker: None,
            custom: custom.clone(),
        };

        // Serialize to JSON
        let json = serde_json::to_string(&config).expect("Failed to serialize");

        // Deserialize back
        let deserialized: Config<CustomConfig> =
            serde_json::from_str(&json).expect("Failed to deserialize");

        assert_eq!(deserialized.custom, custom);
        assert_eq!(deserialized.service.name, "test");
    }

    #[test]
    fn test_config_deserialization_with_flatten() {
        // Simulate a JSON config with both framework and custom fields
        let json_str = r#"{
            "service": {
                "name": "my-service",
                "port": 9000,
                "log_level": "debug",
                "timeout_secs": 60,
                "environment": "production"
            },
            "token": {
                "format": "paseto",
                "version": "v4",
                "purpose": "local",
                "key_path": "./keys/paseto.key"
            },
            "rate_limit": {
                "per_user_rpm": 150,
                "per_client_rpm": 750,
                "window_secs": 60
            },
            "middleware": {
                "cors_mode": "restrictive",
                "body_limit_mb": 10,
                "compression_enabled": true
            },
            "api_key": "prod-api-key",
            "timeout_ms": 10000,
            "feature_flags": {
                "new_dashboard": true,
                "analytics": true
            }
        }"#;

        let config: Config<CustomConfig> =
            serde_json::from_str(json_str).expect("Failed to parse JSON");

        // Verify framework config
        assert_eq!(config.service.name, "my-service");
        assert_eq!(config.service.port, 9000);
        assert_eq!(config.service.log_level, "debug");

        // Verify custom config (flattened fields)
        assert_eq!(config.custom.api_key, "prod-api-key");
        assert_eq!(config.custom.timeout_ms, 10000);
        assert_eq!(
            config.custom.feature_flags.get("new_dashboard"),
            Some(&true)
        );
        assert_eq!(config.custom.feature_flags.get("analytics"), Some(&true));
    }

    #[test]
    fn test_token_config_parses_from_tagged_toml() {
        // TokenConfig is internally tagged on `format`; this is the wire
        // format documented in config.example.toml, the README, and the docs
        // site. Round-trip through Figment (the real load path) so docs and
        // code cannot silently drift apart again.
        let toml = r#"
[token]
format = "paseto"
version = "v4"
purpose = "local"
key_path = "./keys/paseto.key"
issuer = "my-service"
"#;
        let config: Config<()> = Figment::new()
            .merge(Serialized::defaults(Config::<()>::default()))
            .merge(Toml::string(toml))
            .extract()
            .expect("[token] with format = \"paseto\" must deserialize");

        match config.token {
            Some(TokenConfig::Paseto(paseto)) => {
                assert_eq!(paseto.version, "v4");
                assert_eq!(paseto.purpose, "local");
                assert_eq!(paseto.key_path, PathBuf::from("./keys/paseto.key"));
                assert_eq!(paseto.issuer.as_deref(), Some("my-service"));
            }
            other => panic!("expected PASETO token config, got {other:?}"),
        }
    }

    #[test]
    fn test_token_config_rejects_nested_table_form() {
        // `[token.paseto]` produces `{ token: { paseto: {...} } }`, which has
        // no `format` tag and must be rejected — it is not a silently-ignored
        // alternate spelling.
        let toml = r#"
[token.paseto]
version = "v4"
purpose = "local"
key_path = "./keys/paseto.key"
"#;
        let result: std::result::Result<Config<()>, _> = Figment::new()
            .merge(Serialized::defaults(Config::<()>::default()))
            .merge(Toml::string(toml))
            .extract();

        assert!(
            result.is_err(),
            "nested [token.paseto] must fail: the wire format is [token] with format = \"paseto\""
        );
    }

    #[test]
    fn test_config_example_toml_loads() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../config.example.toml");
        let config = Config::<()>::load_from(path)
            .expect("config.example.toml must load under default features");
        assert!(
            config.token.is_none(),
            "token sections in config.example.toml must stay commented (JWT needs the `jwt` feature)"
        );
    }

    #[test]
    fn test_default_bind_is_unspecified() {
        // Default must remain 0.0.0.0 for backward compatibility.
        let config = Config::<()>::default();
        assert_eq!(config.service.bind, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    }

    #[test]
    fn test_service_bind_parses_from_toml() {
        // Round-trip through Figment (the real load path) so a loopback bind
        // documented in config.example.toml actually resolves to 127.0.0.1.
        let toml = r#"
[service]
name = "loopback-service"
bind = "127.0.0.1"
"#;
        let config: Config<()> = Figment::new()
            .merge(Serialized::defaults(Config::<()>::default()))
            .merge(Toml::string(toml))
            .extract()
            .expect("[service] bind = \"127.0.0.1\" must deserialize");
        assert_eq!(config.service.bind, IpAddr::V4(Ipv4Addr::LOCALHOST));
    }

    #[test]
    fn test_service_bind_accepts_ipv6() {
        let toml = r#"
[service]
name = "v6-service"
bind = "::1"
"#;
        let config: Config<()> = Figment::new()
            .merge(Serialized::defaults(Config::<()>::default()))
            .merge(Toml::string(toml))
            .extract()
            .expect("[service] bind = \"::1\" must deserialize");
        assert_eq!(
            config.service.bind,
            IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)
        );
    }

    #[test]
    fn test_service_bind_rejects_garbage() {
        // Validation is free: an unparseable address must fail extraction
        // rather than silently falling back to a default.
        let toml = r#"
[service]
name = "bad-service"
bind = "not-an-ip"
"#;
        let result: std::result::Result<Config<()>, _> = Figment::new()
            .merge(Serialized::defaults(Config::<()>::default()))
            .merge(Toml::string(toml))
            .extract();
        assert!(result.is_err(), "an invalid bind address must be rejected");
    }

    #[test]
    fn test_grpc_effective_bind_falls_back_to_service() {
        let service_bind = IpAddr::V4(Ipv4Addr::LOCALHOST);

        // No gRPC-specific bind: fall back to the service bind.
        let unset: GrpcConfig =
            serde_json::from_str("{}").expect("empty gRPC config must deserialize via defaults");
        assert_eq!(unset.effective_bind(service_bind), service_bind);

        // Explicit gRPC bind wins over the service bind.
        let explicit: GrpcConfig = serde_json::from_str(r#"{ "bind": "0.0.0.0" }"#)
            .expect("gRPC config with bind must deserialize");
        assert_eq!(
            explicit.effective_bind(service_bind),
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        );
    }

    #[test]
    fn test_grpc_bind_parses_from_toml() {
        let toml = r#"
[service]
name = "svc"

[grpc]
use_separate_port = true
bind = "127.0.0.1"
port = 9090
"#;
        let config: Config<()> = Figment::new()
            .merge(Serialized::defaults(Config::<()>::default()))
            .merge(Toml::string(toml))
            .extract()
            .expect("[grpc] bind must deserialize");
        let grpc = config.grpc.expect("grpc section present");
        assert_eq!(grpc.bind, Some(IpAddr::V4(Ipv4Addr::LOCALHOST)));
        assert_eq!(grpc.port, 9090);
    }

    #[cfg(feature = "tls")]
    #[test]
    fn test_grpc_tls_parses_from_toml() {
        let toml = r#"
[service]
name = "svc"

[grpc]
use_separate_port = true
port = 9090

[grpc.tls]
enabled = true
cert_path = "./certs/grpc.pem"
key_path = "./certs/grpc.key"
"#;
        let config: Config<()> = Figment::new()
            .merge(Serialized::defaults(Config::<()>::default()))
            .merge(Toml::string(toml))
            .extract()
            .expect("[grpc.tls] must deserialize");
        let grpc = config.grpc.expect("grpc section present");
        let tls = grpc.tls.expect("grpc tls present");
        assert!(tls.enabled);
        assert_eq!(tls.cert_path, PathBuf::from("./certs/grpc.pem"));
        assert_eq!(tls.key_path, PathBuf::from("./certs/grpc.key"));
    }
}
