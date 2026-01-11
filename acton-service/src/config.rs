//! Configuration management using Figment
//!
//! Configuration is loaded from multiple sources with the following precedence (highest to lowest):
//! 1. Environment variables (prefix: ACTON_)
//! 2. Current working directory: ./config.toml
//! 3. XDG config directory: ~/.config/acton-service/{service_name}/config.toml
//! 4. System directory: /etc/acton-service/{service_name}/config.toml
//! 5. Default values

use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::path::{Path, PathBuf};
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

    /// Redis configuration (optional)
    #[serde(default)]
    pub redis: Option<RedisConfig>,

    /// NATS configuration (optional)
    #[serde(default)]
    pub nats: Option<NatsConfig>,

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
}

impl Default for PasetoConfig {
    fn default() -> Self {
        Self {
            version: default_paseto_version(),
            purpose: default_paseto_purpose(),
            key_path: PathBuf::from("./keys/paseto.key"),
            issuer: None,
            audience: None,
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

    /// Enable retry logic
    #[serde(default = "default_true")]
    pub retry_enabled: bool,

    /// Maximum number of retry attempts
    #[serde(default = "default_retry_max_attempts")]
    pub retry_max_attempts: usize,

    /// Base delay for exponential backoff (milliseconds)
    #[serde(default = "default_retry_base_delay_ms")]
    pub retry_base_delay_ms: u64,

    /// Maximum delay for exponential backoff (milliseconds)
    #[serde(default = "default_retry_max_delay_ms")]
    pub retry_max_delay_ms: u64,

    /// Enable bulkhead (concurrency limiting)
    #[serde(default = "default_true")]
    pub bulkhead_enabled: bool,

    /// Maximum concurrent requests
    #[serde(default = "default_bulkhead_max_concurrent")]
    pub bulkhead_max_concurrent: usize,

    /// Maximum queued requests
    #[serde(default = "default_bulkhead_max_queued")]
    pub bulkhead_max_queued: usize,
}

impl ResilienceConfig {
    /// Convert to Duration types for runtime use
    pub fn circuit_breaker_wait_duration(&self) -> Duration {
        Duration::from_secs(self.circuit_breaker_wait_secs)
    }

    pub fn retry_base_delay(&self) -> Duration {
        Duration::from_millis(self.retry_base_delay_ms)
    }

    pub fn retry_max_delay(&self) -> Duration {
        Duration::from_millis(self.retry_max_delay_ms)
    }
}

/// HTTP metrics configuration (OpenTelemetry)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    /// Enable metrics collection
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Include request path in metrics
    #[serde(default = "default_true")]
    pub include_path: bool,

    /// Include request method in metrics
    #[serde(default = "default_true")]
    pub include_method: bool,

    /// Include status code in metrics
    #[serde(default = "default_true")]
    pub include_status: bool,

    /// Histogram buckets for latency (in milliseconds)
    #[serde(default = "default_latency_buckets")]
    pub latency_buckets_ms: Vec<f64>,
}

impl MetricsConfig {
    pub fn latency_buckets_as_duration(&self) -> Vec<Duration> {
        self.latency_buckets_ms
            .iter()
            .map(|&ms| Duration::from_millis(ms as u64))
            .collect()
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

fn default_retry_max_attempts() -> usize {
    3
}

fn default_retry_base_delay_ms() -> u64 {
    100
}

fn default_retry_max_delay_ms() -> u64 {
    10000 // 10 seconds
}

fn default_bulkhead_max_concurrent() -> usize {
    100
}

fn default_bulkhead_max_queued() -> usize {
    200
}

// Metrics default functions
fn default_latency_buckets() -> Vec<f64> {
    vec![5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 10000.0]
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

impl<T> Config<T>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    /// Load configuration from all sources
    ///
    /// Searches for config files in this order (first found is used):
    /// 1. Current working directory: ./config.toml
    /// 2. XDG config directory: ~/.config/acton-service/{service_name}/config.toml
    /// 3. System directory: /etc/acton-service/{service_name}/config.toml
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
    /// 2. XDG config directory
    /// 3. System directory
    fn find_config_paths(service_name: &str) -> Vec<PathBuf> {
        let mut paths = Vec::new();

        // 1. Current working directory (highest priority for dev/testing)
        paths.push(PathBuf::from("config.toml"));

        // 2. XDG config directory (~/.config/acton-service/{service_name}/config.toml)
        // Use find_config_file instead of place_config_file to avoid creating directories
        let xdg_dirs = xdg::BaseDirectories::with_prefix("acton-service");
        let config_file_path = Path::new(service_name).join("config.toml");
        if let Some(path) = xdg_dirs.find_config_file(&config_file_path) {
            paths.push(path);
        }

        // 3. System-wide directory (/etc/acton-service/{service_name}/config.toml)
        paths.push(PathBuf::from("/etc/acton-service").join(service_name).join("config.toml"));

        paths
    }

    /// Get the recommended config path for a service
    ///
    /// This is where the config file should be placed in production.
    /// Returns: ~/.config/acton-service/{service_name}/config.toml
    pub fn recommended_path(service_name: &str) -> PathBuf {
        let xdg_dirs = xdg::BaseDirectories::with_prefix("acton-service");
        let config_file_path = Path::new(service_name).join("config.toml");

        // place_config_file creates parent directories if needed
        xdg_dirs.place_config_file(&config_file_path)
            .unwrap_or_else(|_| {
                // Fallback to manual path construction if place_config_file fails
                PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| String::from("~")))
                    .join(".config/acton-service")
                    .join(service_name)
                    .join("config.toml")
            })
    }

    /// Create the config directory structure for a service
    ///
    /// Creates ~/.config/acton-service/{service_name}/ if it doesn't exist
    pub fn create_config_dir(service_name: &str) -> Result<PathBuf> {
        let xdg_dirs = xdg::BaseDirectories::with_prefix("acton-service");
        let config_file_path = Path::new(service_name).join("config.toml");

        // place_config_file creates all necessary parent directories
        let config_path = xdg_dirs.place_config_file(&config_file_path)
            .map_err(|e| crate::error::Error::Internal(format!("Failed to create config directory: {}", e)))?;

        // Return the directory path, not the file path
        Ok(config_path.parent()
            .ok_or_else(|| crate::error::Error::Internal("Invalid config path".to_string()))?
            .to_path_buf())
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
                port: default_port(),
                log_level: default_log_level(),
                timeout_secs: default_timeout(),
                environment: default_environment(),
            },
            token: None,
            rate_limit: RateLimitConfig {
                per_user_rpm: default_per_user_rpm(),
                per_client_rpm: default_per_client_rpm(),
                window_secs: default_window_secs(),
                routes: std::collections::HashMap::new(),
            },
            middleware: MiddlewareConfig::default(),
            database: None,
            #[cfg(feature = "turso")]
            turso: None,
            redis: None,
            nats: None,
            otlp: None,
            grpc: None,
            #[cfg(feature = "websocket")]
            websocket: None,
            #[cfg(feature = "cedar-authz")]
            cedar: None,
            custom: T::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_default_config() {
        let config = Config::<()>::default();
        assert_eq!(config.service.port, 8080);
        assert_eq!(config.service.log_level, "info");
        assert_eq!(config.rate_limit.per_user_rpm, 200);
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
                port: 9090,
                log_level: "debug".to_string(),
                timeout_secs: 30,
                environment: "test".to_string(),
            },
            token: Some(TokenConfig::Paseto(PasetoConfig {
                version: "v4".to_string(),
                purpose: "local".to_string(),
                key_path: PathBuf::from("./test-key.key"),
                issuer: Some("test-issuer".to_string()),
                audience: None,
            })),
            rate_limit: RateLimitConfig {
                per_user_rpm: 100,
                per_client_rpm: 500,
                window_secs: 60,
                routes: std::collections::HashMap::new(),
            },
            middleware: MiddlewareConfig::default(),
            database: None,
            #[cfg(feature = "turso")]
            turso: None,
            redis: None,
            nats: None,
            otlp: None,
            grpc: None,
            #[cfg(feature = "websocket")]
            websocket: None,
            #[cfg(feature = "cedar-authz")]
            cedar: None,
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
                port: 8080,
                log_level: "info".to_string(),
                timeout_secs: 30,
                environment: "dev".to_string(),
            },
            token: None,
            rate_limit: RateLimitConfig {
                per_user_rpm: 200,
                per_client_rpm: 1000,
                window_secs: 60,
                routes: std::collections::HashMap::new(),
            },
            middleware: MiddlewareConfig::default(),
            database: None,
            #[cfg(feature = "turso")]
            turso: None,
            redis: None,
            nats: None,
            otlp: None,
            grpc: None,
            #[cfg(feature = "websocket")]
            websocket: None,
            #[cfg(feature = "cedar-authz")]
            cedar: None,
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
        assert_eq!(config.custom.feature_flags.get("new_dashboard"), Some(&true));
        assert_eq!(config.custom.feature_flags.get("analytics"), Some(&true));
    }
}
