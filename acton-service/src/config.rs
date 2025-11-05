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
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::Result;

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Service configuration
    pub service: ServiceConfig,

    /// JWT configuration
    pub jwt: JwtConfig,

    /// Rate limiting configuration
    pub rate_limit: RateLimitConfig,

    /// Middleware configuration
    #[serde(default)]
    pub middleware: MiddlewareConfig,

    /// Database configuration (optional)
    #[serde(default)]
    pub database: Option<DatabaseConfig>,

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

/// JWT configuration
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
    /// Requests per minute per user
    #[serde(default = "default_per_user_rpm")]
    pub per_user_rpm: u32,

    /// Requests per minute per client
    #[serde(default = "default_per_client_rpm")]
    pub per_client_rpm: u32,

    /// Rate limit window in seconds
    #[serde(default = "default_window_secs")]
    pub window_secs: u64,
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
    "permissive".to_string()
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

impl Config {
    /// Load configuration from all sources
    ///
    /// Searches for config files in this order (first found is used):
    /// 1. Current working directory: ./config.toml
    /// 2. XDG config directory: ~/.config/acton-service/{service_name}/config.toml
    /// 3. System directory: /etc/acton-service/{service_name}/config.toml
    ///
    /// Environment variables (ACTON_ prefix) override all file-based configs.
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
            .merge(Serialized::defaults(Config::default()));

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
            .merge(Serialized::defaults(Config::default()))
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
        let xdg_dirs = xdg::BaseDirectories::with_prefix("acton-service");
        let config_file_path = Path::new(service_name).join("config.toml");
        if let Ok(path) = xdg_dirs.place_config_file(&config_file_path) {
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            service: ServiceConfig {
                name: "acton-service".to_string(),
                port: default_port(),
                log_level: default_log_level(),
                timeout_secs: default_timeout(),
                environment: default_environment(),
            },
            jwt: JwtConfig {
                public_key_path: PathBuf::from("./keys/jwt-public.pem"),
                algorithm: default_jwt_algorithm(),
                issuer: None,
                audience: None,
            },
            rate_limit: RateLimitConfig {
                per_user_rpm: default_per_user_rpm(),
                per_client_rpm: default_per_client_rpm(),
                window_secs: default_window_secs(),
            },
            middleware: MiddlewareConfig::default(),
            database: None,
            redis: None,
            nats: None,
            otlp: None,
            grpc: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.service.port, 8080);
        assert_eq!(config.service.log_level, "info");
        assert_eq!(config.rate_limit.per_user_rpm, 200);
    }
}
