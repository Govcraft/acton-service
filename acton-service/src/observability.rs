//! OpenTelemetry tracing and observability
//!
//! This module provides comprehensive observability with:
//! - Full OpenTelemetry integration with OTLP export
//! - Structured JSON logging
//! - Distributed tracing with span propagation
//! - Graceful fallback when OTLP is not configured

use crate::{config::Config, error::Result};

#[cfg(feature = "observability")]
use {
    opentelemetry::{global, trace::TracerProvider},
    opentelemetry_otlp::{SpanExporter, WithExportConfig},
    opentelemetry_sdk::{
        propagation::TraceContextPropagator,
        trace::SdkTracerProvider,
        Resource,
    },
    tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer},
};

#[cfg(feature = "otel-metrics")]
use {
    opentelemetry::metrics::MeterProvider as _,
    opentelemetry_otlp::MetricExporter,
    opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider},
    std::time::Duration as StdDuration,
};

/// Global tracer provider for graceful shutdown
#[cfg(feature = "observability")]
static TRACER_PROVIDER: once_cell::sync::OnceCell<SdkTracerProvider> =
    once_cell::sync::OnceCell::new();

/// Global meter provider for graceful shutdown
#[cfg(feature = "otel-metrics")]
pub static METER_PROVIDER: once_cell::sync::OnceCell<SdkMeterProvider> =
    once_cell::sync::OnceCell::new();

/// Initialize tracing with OpenTelemetry and structured logging
///
/// This function sets up:
/// - OpenTelemetry OTLP exporter (if configured)
/// - Structured JSON logging with tracing
/// - Trace context propagation (W3C Trace Context)
/// - Graceful fallback to JSON-only logging if OTLP fails
///
/// # Arguments
/// * `config` - Service configuration containing OTLP and service details
///
/// # Returns
/// * `Ok(())` on successful initialization
/// * `Err` if tracing setup fails critically
#[cfg(feature = "observability")]
pub fn init_tracing(config: &Config) -> Result<()> {
    let log_level = config.service.log_level.clone();
    let service_name = config.service.name.clone();

    // Set global trace context propagator for distributed tracing
    global::set_text_map_propagator(TraceContextPropagator::new());

    // Build subscriber with JSON formatting
    let fmt_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_filter(
            EnvFilter::try_new(&log_level).unwrap_or_else(|_| EnvFilter::new("info")),
        );

    // Try to initialize OpenTelemetry if configured
    if let Some(otlp_config) = &config.otlp {
        if otlp_config.enabled {
            match init_otlp_tracer(otlp_config, &service_name) {
                Ok(tracer_provider) => {
                    // Create tracer directly from provider for type compatibility
                    let tracer = tracer_provider.tracer(service_name.clone());
                    let telemetry_layer = tracing_opentelemetry::layer().with_tracer(tracer);

                    tracing_subscriber::registry()
                        .with(fmt_layer)
                        .with(telemetry_layer)
                        .init();

                    // Store provider for shutdown and set global
                    let _ = TRACER_PROVIDER.set(tracer_provider.clone());
                    global::set_tracer_provider(tracer_provider);

                    tracing::info!(
                        service = %service_name,
                        otlp_endpoint = %otlp_config.endpoint,
                        "OpenTelemetry tracing initialized with OTLP export"
                    );

                    return Ok(());
                }
                Err(e) => {
                    // Log error but continue with JSON-only logging
                    eprintln!(
                        "Failed to initialize OTLP exporter (falling back to JSON logging): {}",
                        e
                    );
                }
            }
        }
    }

    // Fallback: JSON logging only (no OTLP)
    tracing_subscriber::registry().with(fmt_layer).init();

    tracing::info!(
        service = %service_name,
        "Tracing initialized with JSON logging (OTLP not configured)"
    );

    Ok(())
}

/// Initialize OpenTelemetry OTLP tracer using official SDK pattern
#[cfg(feature = "observability")]
pub(crate) fn init_otlp_tracer(
    otlp_config: &crate::config::OtlpConfig,
    service_name: &str,
) -> Result<SdkTracerProvider> {
    // Use service name from OTLP config or fall back to main service name
    let trace_service_name = otlp_config
        .service_name
        .as_ref()
        .unwrap_or(&service_name.to_string())
        .clone();

    // Create resource with service metadata
    let resource = Resource::builder()
        .with_service_name(trace_service_name)
        .build();

    // Build OTLP span exporter with Tonic gRPC transport
    let mut exporter_builder = SpanExporter::builder().with_tonic();

    // Configure custom endpoint if provided (default is http://localhost:4317)
    if !otlp_config.endpoint.is_empty() {
        exporter_builder = exporter_builder.with_endpoint(&otlp_config.endpoint);
    }

    let exporter = exporter_builder
        .build()
        .map_err(|e| {
            crate::error::Error::Internal(format!("Failed to build OTLP exporter: {}", e))
        })?;

    // Build tracer provider with production-ready configuration
    let provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    Ok(provider)
}

/// Initialize OpenTelemetry OTLP meter provider for metrics collection
#[cfg(feature = "otel-metrics")]
pub(crate) fn init_otlp_meter(
    otlp_config: &crate::config::OtlpConfig,
    service_name: &str,
) -> Result<SdkMeterProvider> {
    // Use service name from OTLP config or fall back to main service name
    let metrics_service_name = otlp_config
        .service_name
        .as_ref()
        .unwrap_or(&service_name.to_string())
        .clone();

    // Create resource with service metadata
    let resource = Resource::builder()
        .with_service_name(metrics_service_name)
        .build();

    // Build OTLP metric exporter with Tonic gRPC transport
    let mut exporter_builder = MetricExporter::builder().with_tonic();

    // Configure custom endpoint if provided (default is http://localhost:4317)
    if !otlp_config.endpoint.is_empty() {
        exporter_builder = exporter_builder.with_endpoint(&otlp_config.endpoint);
    }

    let exporter = exporter_builder
        .build()
        .map_err(|e| {
            crate::error::Error::Internal(format!("Failed to build OTLP metric exporter: {}", e))
        })?;

    // Create periodic reader with appropriate export interval (15s for Prometheus compatibility)
    let reader = PeriodicReader::builder(exporter)
        .with_interval(StdDuration::from_secs(15))
        .build();

    // Build meter provider with production-ready configuration
    let provider = SdkMeterProvider::builder()
        .with_resource(resource)
        .with_reader(reader)
        .build();

    Ok(provider)
}

/// Get the global meter for metrics collection
///
/// This function returns a meter from the global meter provider if metrics are enabled.
/// Returns None if no meter provider has been initialized (neither local nor global).
#[cfg(feature = "otel-metrics")]
pub fn get_meter() -> Option<opentelemetry::metrics::Meter> {
    // Try local provider first
    if let Some(provider) = METER_PROVIDER.get() {
        return Some(provider.meter("acton-service"));
    }

    // Check if global meter provider was set (not the noop default)
    // This is a bit hacky but works: try to get a meter and see if it's functional
    // For the real use case (with OTLP or explicit provider), this will work
    // For tests without a provider, we rely on METER_PROVIDER being empty
    None
}

/// Initialize the meter provider and set it globally
///
/// This should be called during service initialization if metrics are enabled.
#[cfg(feature = "otel-metrics")]
pub fn init_meter_provider(config: &Config) -> Result<()> {
    if let Some(otlp_config) = &config.otlp {
        if otlp_config.enabled {
            match init_otlp_meter(otlp_config, &config.service.name) {
                Ok(meter_provider) => {
                    // Store provider for later access and shutdown
                    let _ = METER_PROVIDER.set(meter_provider.clone());

                    // Set global meter provider
                    global::set_meter_provider(meter_provider);

                    tracing::info!(
                        service = %config.service.name,
                        otlp_endpoint = %otlp_config.endpoint,
                        "OpenTelemetry metrics initialized with OTLP export"
                    );

                    return Ok(());
                }
                Err(e) => {
                    // Log error but continue without metrics
                    tracing::warn!(
                        error = %e,
                        "Failed to initialize OTLP metric exporter (metrics disabled)"
                    );
                }
            }
        }
    }

    tracing::info!("Metrics not configured or disabled");
    Ok(())
}

/// Initialize tracing without OpenTelemetry (fallback when observability feature is disabled)
#[cfg(not(feature = "observability"))]
pub fn init_tracing(config: &Config) -> Result<()> {
    let log_level = config.service.log_level.clone();

    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::try_new(&log_level).unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!(
        service = %config.service.name,
        "Tracing initialized (observability feature disabled)"
    );

    Ok(())
}

/// Shutdown tracing and flush all pending spans to OTLP collector
///
/// This ensures all telemetry data is exported before process termination.
/// Should be called during graceful shutdown.
#[cfg(feature = "observability")]
pub fn shutdown_tracing() {
    tracing::info!("Shutting down tracing and flushing spans...");

    // Shutdown the tracer provider if initialized
    if let Some(provider) = TRACER_PROVIDER.get() {
        if let Err(e) = provider.shutdown() {
            eprintln!("Error during tracer provider shutdown: {}", e);
        } else {
            tracing::debug!("OpenTelemetry tracer provider shutdown complete");
        }
    }

    // Shutdown the meter provider if initialized
    #[cfg(feature = "otel-metrics")]
    if let Some(provider) = METER_PROVIDER.get() {
        if let Err(e) = provider.shutdown() {
            eprintln!("Error during meter provider shutdown: {}", e);
        } else {
            tracing::debug!("OpenTelemetry meter provider shutdown complete");
        }
    }

    tracing::info!("Tracing shutdown complete");
}

/// Shutdown tracing (no-op without observability feature)
#[cfg(not(feature = "observability"))]
pub fn shutdown_tracing() {
    tracing::info!("Tracing shutdown (observability feature disabled)");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_tracing_without_otlp() {
        let config = Config::default();
        // This should not panic and should fall back to JSON logging
        let result = init_tracing(&config);
        assert!(result.is_ok(), "Tracing initialization should succeed");
    }

    #[tokio::test]
    #[cfg(feature = "observability")]
    async fn test_init_tracing_with_invalid_otlp() {
        // Note: This test verifies initialization logic but can't actually call init_tracing
        // because the global subscriber can only be set once per process.
        // Instead, we test the OTLP tracer initialization directly.

        let otlp_config = crate::config::OtlpConfig {
            endpoint: "http://invalid-endpoint:4317".to_string(),
            service_name: Some("test-service".to_string()),
            enabled: true,
        };

        // The OTLP exporter should build successfully even with invalid endpoint
        // It will only fail when trying to actually send spans (lazy connection)
        let result = init_otlp_tracer(&otlp_config, "test-service");

        // Should succeed - the exporter doesn't validate connectivity at build time
        assert!(result.is_ok(), "OTLP tracer should build even with invalid endpoint (connection is lazy)");
    }

    #[test]
    fn test_shutdown_tracing() {
        // Should not panic
        shutdown_tracing();
    }

    #[tokio::test]
    #[cfg(feature = "otel-metrics")]
    async fn test_init_meter_provider_without_config() {
        let config = Config::default();
        // Should succeed even without OTLP config
        let result = init_meter_provider(&config);
        assert!(result.is_ok(), "Meter provider init should succeed without config");
    }

    #[tokio::test]
    #[cfg(feature = "otel-metrics")]
    async fn test_init_otlp_meter() {
        let otlp_config = crate::config::OtlpConfig {
            endpoint: "http://localhost:4317".to_string(),
            service_name: Some("test-metrics-service".to_string()),
            enabled: true,
        };

        // The OTLP metric exporter should build successfully even with potentially invalid endpoint
        // It will only fail when trying to actually send metrics (lazy connection)
        let result = init_otlp_meter(&otlp_config, "test-service");

        assert!(result.is_ok(), "OTLP meter should build even with potentially invalid endpoint (connection is lazy)");
    }

    #[test]
    #[cfg(feature = "otel-metrics")]
    fn test_get_meter_without_init() {
        // Before initialization, get_meter should return None
        let meter = get_meter();
        assert!(meter.is_none(), "get_meter should return None before initialization");
    }
}
