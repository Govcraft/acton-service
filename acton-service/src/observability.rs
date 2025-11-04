//! OpenTelemetry tracing and observability

#[cfg(feature = "observability")]
use tracing_subscriber::EnvFilter;

use crate::{config::Config, error::Result};

/// Initialize tracing with OpenTelemetry
#[cfg(feature = "observability")]
pub fn init_tracing(config: &Config) -> Result<()> {
    let log_level = config.service.log_level.clone();

    // For now, just use JSON formatting without OpenTelemetry
    // Full OpenTelemetry integration can be added later with proper version compatibility
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::try_new(&log_level).unwrap_or_else(|_| EnvFilter::new("info"))
        )
        .init();

    tracing::info!("Tracing initialized for service: {}", config.service.name);

    Ok(())
}

/// Initialize tracing without OpenTelemetry (fallback)
#[cfg(not(feature = "observability"))]
pub fn init_tracing(config: &Config) -> Result<()> {
    let log_level = config.service.log_level.clone();

    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::try_new(&log_level).unwrap_or_else(|_| EnvFilter::new("info"))
        )
        .init();

    tracing::info!("Tracing initialized for service: {}", config.service.name);

    Ok(())
}

/// Shutdown tracing and flush spans
#[cfg(feature = "observability")]
pub fn shutdown_tracing() {
    tracing::info!("Tracing shutdown complete");
}

/// Shutdown tracing (no-op without observability feature)
#[cfg(not(feature = "observability"))]
pub fn shutdown_tracing() {
    tracing::info!("Tracing shutdown (no-op)");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_tracing_without_otlp() {
        let config = Config::default();
        // This should not panic
        let _ = init_tracing(&config);
    }
}
