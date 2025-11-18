//! Example: Using Custom Configuration Extensions
//!
//! This example demonstrates how to extend the framework's Config with your own
//! custom configuration fields. Custom fields are automatically loaded from the
//! same config.toml file using #[serde(flatten)].
//!
//! Run with: cargo run --example custom-config

use acton_service::prelude::*;
use axum::{extract::State, routing::get, Json};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Custom configuration that extends the framework config
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct MyCustomConfig {
    /// API key for external service
    external_api_key: String,

    /// Feature flags
    feature_flags: HashMap<String, bool>,

    /// Custom timeout in milliseconds
    custom_timeout_ms: u32,

    /// Custom retry settings
    #[serde(default)]
    retry_config: RetryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RetryConfig {
    max_attempts: u32,
    backoff_ms: u32,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            backoff_ms: 1000,
        }
    }
}

/// Response showing both framework and custom config
#[derive(Serialize)]
struct ConfigInfoResponse {
    service_name: String,
    service_port: u16,
    environment: String,
    // Custom fields
    external_api_configured: bool,
    enabled_features: Vec<String>,
    retry_attempts: u32,
}

/// Handler that accesses both framework and custom configuration
async fn config_info(State(state): State<AppState<MyCustomConfig>>) -> Json<ConfigInfoResponse> {
    let config = state.config();

    // Access framework config
    let service_name = config.service.name.clone();
    let service_port = config.service.port;
    let environment = config.service.environment.clone();

    // Access custom config
    let external_api_configured = !config.custom.external_api_key.is_empty();
    let enabled_features: Vec<String> = config
        .custom
        .feature_flags
        .iter()
        .filter_map(|(k, v)| if *v { Some(k.clone()) } else { None })
        .collect();
    let retry_attempts = config.custom.retry_config.max_attempts;

    Json(ConfigInfoResponse {
        service_name,
        service_port,
        environment,
        external_api_configured,
        enabled_features,
        retry_attempts,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load custom config explicitly
    // In production, you can use Config::<MyCustomConfig>::load() to load from
    // config.toml automatically with XDG directory support
    let config = Config::<MyCustomConfig> {
        service: ServiceConfig {
            name: "my-custom-service".to_string(),
            port: 8080,
            log_level: "info".to_string(),
            timeout_secs: 30,
            environment: "dev".to_string(),
        },
        jwt: JwtConfig::default(),
        rate_limit: RateLimitConfig::default(),
        middleware: MiddlewareConfig::default(),
        database: None,
        redis: None,
        nats: None,
        otlp: None,
        grpc: None,
        #[cfg(feature = "cedar-authz")]
        cedar: None,
        custom: MyCustomConfig {
            external_api_key: "my-secret-key-123".to_string(),
            custom_timeout_ms: 5000,
            feature_flags: {
                let mut flags = HashMap::new();
                flags.insert("new_dashboard".to_string(), true);
                flags.insert("analytics".to_string(), false);
                flags
            },
            retry_config: RetryConfig {
                max_attempts: 5,
                backoff_ms: 2000,
            },
        },
    };

    // Build AppState with custom config
    let state = AppState::new(config);

    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/config-info", get(config_info))
        })
        .build_routes_with_state(state.clone());

    println!("🚀 Starting service with custom configuration extensions...");
    println!("🔗 Try: http://localhost:8080/api/v1/config-info");

    ServiceBuilder::<MyCustomConfig>::new()
        .with_state(state)
        .with_routes(routes)
        .build()
        .serve()
        .await
}
