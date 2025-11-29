//! Example: Using Custom Configuration Extensions
//!
//! This example demonstrates how to extend the framework's Config with your own
//! custom configuration fields. Custom fields are automatically loaded from the
//! same config.toml file using #[serde(flatten)].
//!
//! Run with: cargo run --example custom-config

use acton_service::prelude::*;
use axum::{routing::get, Json};
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

/// Response showing configuration info
#[derive(Serialize)]
struct ConfigInfoResponse {
    message: String,
    service_name: String,
}

/// Handler that returns basic info
async fn config_info() -> Json<ConfigInfoResponse> {
    Json(ConfigInfoResponse {
        message: "Custom config loaded successfully!".to_string(),
        service_name: "my-custom-service".to_string(),
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    // Demonstrate loading custom config type
    // In production, use Config::<MyCustomConfig>::load() to load from config.toml
    let mut custom_config = Config::<MyCustomConfig>::default();
    custom_config.service.name = "my-custom-service".to_string();
    custom_config.custom = MyCustomConfig {
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

    // Print custom config to demonstrate it's loaded
    println!("Custom Config Loaded:");
    println!("  - Service name: {}", custom_config.service.name);
    println!("  - External API key: {}", custom_config.custom.external_api_key);
    println!("  - Enabled features: {:?}", custom_config.custom.feature_flags);
    println!("  - Retry attempts: {}", custom_config.custom.retry_config.max_attempts);

    // Build versioned routes (uses default () type)
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/config-info", get(config_info))
        })
        .build_routes();

    println!("\nStarting service with custom configuration extensions...");
    println!("Try: http://localhost:8080/api/v1/config-info");

    // For services with custom config that need to access it in handlers,
    // you would typically:
    // 1. Store config in an Extension layer
    // 2. Or use a global static with once_cell
    // 3. Or pass specific values through Extension

    // This example uses the default ServiceBuilder since the routes don't need state
    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await
}
