//! HTTP server with graceful shutdown

use axum::Router;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::signal;
use tower_http::{
    catch_panic::CatchPanicLayer,
    compression::CompressionLayer,
    cors::CorsLayer,
    limit::RequestBodyLimitLayer,
    timeout::TimeoutLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};

use crate::{
    config::Config,
    error::Result,
    middleware::{request_id_layer, request_id_propagation_layer, sensitive_headers_layer},
};

/// Server instance
pub struct Server {
    config: Config,
}

impl Server {
    /// Create a new server instance
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Run the server with the given router
    pub async fn serve(self, app: Router) -> Result<()> {
        let addr = SocketAddr::from(([0, 0, 0, 0], self.config.service.port));

        tracing::info!("Starting {} on {}", self.config.service.name, addr);

        // Log middleware configuration
        self.log_middleware_config();

        // Build middleware stack using ServiceBuilder for optimal composition
        // Note: Layers are applied in reverse order (bottom layer is innermost/first)
        let body_limit = self.config.middleware.body_limit_mb * 1024 * 1024;
        let cors_layer = self.build_cors_layer();

        let app = app
            // CORS (outermost layer) - configurable
            .layer(cors_layer)
            // Compression - always enabled (minimal overhead)
            .layer(CompressionLayer::new())
            // Request timeout
            .layer(TimeoutLayer::with_status_code(
                http::StatusCode::REQUEST_TIMEOUT,
                Duration::from_secs(self.config.service.timeout_secs),
            ))
            // Request body size limit - configurable via config
            .layer(RequestBodyLimitLayer::new(body_limit))
            // Tracing (always enabled)
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(DefaultMakeSpan::new().include_headers(true))
                    .on_response(DefaultOnResponse::new().include_headers(true)),
            )
            // Request tracking layers - always enabled for distributed tracing
            .layer(sensitive_headers_layer())
            .layer(request_id_propagation_layer())
            .layer(request_id_layer())
            // Panic recovery (innermost layer) - always enabled for stability
            .layer(CatchPanicLayer::new());

        // Create TCP listener
        let listener = TcpListener::bind(&addr).await?;

        tracing::info!("Server listening on {}", addr);

        // Serve with graceful shutdown
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await?;

        tracing::info!("Server shutdown complete");

        Ok(())
    }

    /// Log middleware configuration for debugging
    fn log_middleware_config(&self) {
        tracing::info!("Middleware configuration:");
        tracing::info!("  - Panic recovery: enabled");
        tracing::info!("  - Request ID tracking: enabled");
        tracing::info!("  - Sensitive header masking: enabled");
        tracing::info!(
            "  - Request body limit: {} MB",
            self.config.middleware.body_limit_mb
        );
        tracing::info!("  - Compression: enabled");
        tracing::info!("  - CORS mode: {}", self.config.middleware.cors_mode);
        tracing::info!(
            "  - Request timeout: {} seconds",
            self.config.service.timeout_secs
        );

        // Log optional advanced middleware
        if let Some(ref resilience) = self.config.middleware.resilience {
            tracing::info!("  - Resilience configured:");
            tracing::info!(
                "    - Circuit breaker: {}",
                resilience.circuit_breaker_enabled
            );
            tracing::info!("    - Retry: {}", resilience.retry_enabled);
            tracing::info!("    - Bulkhead: {}", resilience.bulkhead_enabled);
        } else {
            tracing::info!("  - Resilience: not configured");
        }

        if let Some(ref metrics) = self.config.middleware.metrics {
            tracing::info!("  - HTTP metrics: enabled");
            tracing::info!("    - Include path: {}", metrics.include_path);
            tracing::info!("    - Include method: {}", metrics.include_method);
            tracing::info!("    - Include status: {}", metrics.include_status);
        } else {
            tracing::info!("  - HTTP metrics: not configured");
        }

        if let Some(ref governor) = self.config.middleware.governor {
            tracing::info!(
                "  - Local rate limiting: {} req / {} sec (burst: {})",
                governor.requests_per_period,
                governor.period_secs,
                governor.burst_size
            );
        } else {
            tracing::info!("  - Local rate limiting: not configured");
        }
    }

    /// Get the configuration
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Build CORS layer based on configuration
    fn build_cors_layer(&self) -> CorsLayer {
        match self.config.middleware.cors_mode.as_str() {
            "permissive" => {
                tracing::debug!("Enabling permissive CORS");
                CorsLayer::permissive()
            }
            "restrictive" => {
                tracing::debug!("Enabling restrictive CORS (default deny)");
                CorsLayer::new()
            }
            "disabled" => {
                tracing::debug!("CORS disabled (using restrictive)");
                CorsLayer::new()
            }
            _ => {
                tracing::warn!(
                    "Unknown CORS mode: {}, defaulting to permissive",
                    self.config.middleware.cors_mode
                );
                CorsLayer::permissive()
            }
        }
    }
}

/// Wait for shutdown signal (SIGTERM or SIGINT)
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received SIGINT (Ctrl+C), starting graceful shutdown");
        },
        _ = terminate => {
            tracing::info!("Received SIGTERM, starting graceful shutdown");
        },
    }

    tracing::info!("Shutdown signal received, draining requests...");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_creation() {
        let config = Config::default();
        let server = Server::new(config.clone());
        assert_eq!(server.config().service.port, config.service.port);
    }
}
