//! Example: Using Custom Configuration Extensions
//!
//! This example demonstrates how to extend the framework's Config with your own
//! custom configuration fields. Custom fields are automatically loaded from the
//! same config.toml file using #[serde(flatten)].
//!
//! Run with: cargo run --example custom-config

use acton_service::prelude::*;
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Custom configuration that extends the framework config
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct MyCustomConfig {
    /// API key for external service
    #[serde(default)]
    external_api_key: String,

    /// Feature flags
    #[serde(default)]
    feature_flags: HashMap<String, bool>,

    /// Custom timeout in milliseconds
    #[serde(default)]
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
    // Load config using defaults and then customize
    // In production, use Config::<MyCustomConfig>::load() to load from config.toml
    let mut config = Config::<MyCustomConfig>::default();

    // Customize the service settings
    config.service.name = "my-custom-service".to_string();
    config.service.port = 8080;
    config.service.environment = "dev".to_string();

    // Set custom configuration
    config.custom = MyCustomConfig {
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
    };

    // Build AppState with custom config
    let state = AppState::new(config.clone());

    // Create routes using standard axum Router with the correct state type
    let app = Router::new()
        .route("/api/v1/config-info", get(config_info))
        .route("/health", get(health))
        .route("/ready", get(readiness))
        .with_state(state);

    println!("Starting service with custom configuration extensions...");
    println!("Try: http://localhost:8080/api/v1/config-info");

    // Run the server directly using axum
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", config.service.port))
        .await
        .map_err(|e| Error::Internal(format!("Failed to bind: {}", e)))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| Error::Internal(format!("Server error: {}", e)))?;

    Ok(())
}
