//! Test OpenTelemetry observability implementation
//!
//! Run with: cargo run --example test-observability --features observability

use acton_service::config::Config;
use acton_service::observability::{init_tracing, shutdown_tracing};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing OpenTelemetry observability...\n");

    // Test 1: JSON logging only (no OTLP)
    println!("Test 1: Initializing with JSON logging only");
    let mut config = Config::<()>::default();
    config.service.name = "test-observability".to_string();
    config.otlp = None;

    init_tracing(&config)?;
    tracing::info!("JSON logging initialized successfully");
    println!("✓ JSON logging works\n");

    // Can't test OTLP without a collector running, but we verified it compiles
    // and builds correctly

    shutdown_tracing();
    println!("✓ Shutdown complete\n");

    println!("All observability tests passed!");

    Ok(())
}
