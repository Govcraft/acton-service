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
    ///
    /// # TLS credential rotation
    ///
    /// When `[tls]` sets `reload_interval_secs` or `reload_on_sighup`, this
    /// path installs the same triggers the `ServiceBuilder` path does, over the
    /// same shared implementation, and the listener's credentials rotate in
    /// place without a restart.
    ///
    /// The one difference is that [`Server`] has no builder, so there is no
    /// equivalent of `ServiceBuilder::with_tls_reload` and no way to drive a
    /// reload from your own trigger here. Rotation on this path is
    /// config-driven only. A service that needs to reload from a Vault lease,
    /// a secret watch or an admin endpoint should use `ServiceBuilder`, which
    /// exposes both the hook and the credential handles.
    ///
    /// # Errors
    ///
    /// Returns an error before binding the listener if `[tls]` is enabled but
    /// its credentials cannot be loaded, or if `reload_interval_secs` is `0`.
    pub async fn serve(self, app: Router) -> Result<()> {
        let addr = SocketAddr::new(self.config.service.bind, self.config.service.port);

        tracing::info!("Starting {} on {}", self.config.service.name, addr);

        // Validate the rotation settings before anything binds, so a bad
        // interval is a refusal to start rather than a task that misbehaves
        // once traffic is arriving. Matches how the ServiceBuilder path reports
        // the same misconfiguration from `build()`.
        #[cfg(feature = "tls")]
        let tls_reload_interval = match self.config.tls.as_ref().filter(|t| t.enabled) {
            Some(tls_cfg) => crate::tls::validate_reload_interval(tls_cfg, "[tls]")?,
            None => None,
        };

        // Log middleware configuration
        self.log_middleware_config();

        // Determine TLS status for security headers
        #[cfg(feature = "tls")]
        let tls_enabled = self.config.tls.as_ref().map(|t| t.enabled).unwrap_or(false);
        #[cfg(not(feature = "tls"))]
        let tls_enabled = false;

        // Build middleware stack using ServiceBuilder for optimal composition
        // Note: Layers are applied in reverse order (bottom layer is innermost/first)
        let body_limit = self.config.middleware.body_limit_mb * 1024 * 1024;
        let cors_layer = self.build_cors_layer();

        let app = app
            // CORS (outermost layer) - configurable
            .layer(cors_layer);

        // Security headers (after CORS, before compression)
        let app = crate::middleware::security_headers::apply_security_headers(
            app,
            &self.config.middleware.security_headers,
            tls_enabled,
        );

        // Resilience (circuit breaker + bulkhead) from [middleware.resilience]
        #[cfg(feature = "resilience")]
        let app = match self.config.middleware.resilience {
            Some(ref resilience) => crate::middleware::resilience::apply_resilience(
                app,
                &crate::middleware::resilience::ResilienceConfig::from(resilience),
            ),
            None => app,
        };

        let app = app
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

        // Serve with graceful shutdown -- TLS or plain TCP
        //
        // The TLS listener exposes `TlsConnectInfo` (remote address plus any
        // verified client certificate) as connect-info.
        #[cfg(feature = "tls")]
        if let Some(ref tls_config) = self.config.tls {
            if tls_config.enabled {
                // A reloadable source rather than a fixed `ServerConfig`: the
                // listener rereads it per handshake, which is what lets the
                // triggers below rotate the certificate without rebinding.
                let source = crate::tls::TlsConfigSource::from_tls_config(tls_config)?;
                crate::tls::warn_if_reload_config_is_unusable(Some(&source), tls_config, "[tls]");

                // This path serves one listener, so the handle carries only the
                // HTTP slot and there is no gRPC interval to pass.
                let reload_handle = crate::tls::TlsReloadHandle::new(Some(source.clone()), None);
                // Held across the serve await; dropping it on return aborts the
                // trigger tasks so they cannot outlive the listener.
                let _tls_reload_tasks = crate::tls::install_reload_triggers(
                    &reload_handle,
                    tls_reload_interval,
                    None,
                    tls_config.reload_on_sighup,
                );

                let tls_listener = crate::tls::TlsListener::with_config_source(listener, source);
                tracing::info!("TLS enabled (HTTPS)");
                axum::serve(
                    tls_listener,
                    app.into_make_service_with_connect_info::<crate::tls::TlsConnectInfo>(),
                )
                .with_graceful_shutdown(shutdown_signal())
                .await?;
                tracing::info!("Server shutdown complete");
                return Ok(());
            }
        }

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
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
            // Report what was actually applied, not merely what was parsed.
            #[cfg(feature = "resilience")]
            {
                tracing::info!("  - Resilience applied:");
                tracing::info!(
                    "    - Circuit breaker: {}",
                    resilience.circuit_breaker_enabled
                );
                tracing::info!("    - Bulkhead: {}", resilience.bulkhead_enabled);
            }
            #[cfg(not(feature = "resilience"))]
            {
                let _ = resilience;
                tracing::warn!(
                    "  - Resilience: [middleware.resilience] is configured but the \
                     'resilience' feature is disabled -- no resilience middleware is active"
                );
            }
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

        // TLS status
        #[cfg(feature = "tls")]
        if let Some(ref tls_config) = self.config.tls {
            if tls_config.enabled {
                tracing::info!(
                    "  - TLS: enabled (cert: {})",
                    tls_config.cert_path.display()
                );
            } else {
                tracing::info!("  - TLS: disabled");
            }
        } else {
            tracing::info!("  - TLS: not configured");
        }
        #[cfg(not(feature = "tls"))]
        tracing::info!("  - TLS: feature not enabled");

        // Security headers
        let sh = &self.config.middleware.security_headers;
        if sh.enabled {
            tracing::info!("  - Security headers: enabled");
        } else {
            tracing::info!("  - Security headers: disabled");
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

    /// `Server` is public API and the example in the crate docs, so it must
    /// reject the same TLS misconfiguration the `ServiceBuilder` path does —
    /// and reject it *before* binding, not after traffic can arrive.
    ///
    /// Asserting from the failure side is what makes this testable at all: the
    /// success path would bind a real socket and serve forever. That the error
    /// arrives despite the cert paths not existing is itself the ordering
    /// proof — validation runs ahead of the credential load, which runs ahead
    /// of the bind.
    #[cfg(feature = "tls")]
    #[tokio::test]
    async fn serve_rejects_a_zero_reload_interval_before_binding() {
        use crate::config::TlsConfig;

        let config = Config {
            tls: Some(TlsConfig {
                enabled: true,
                cert_path: "/nonexistent/cert.pem".into(),
                key_path: "/nonexistent/key.pem".into(),
                client_ca_path: None,
                client_auth_optional: false,
                reload_interval_secs: Some(0),
                reload_on_sighup: false,
            }),
            ..Default::default()
        };

        let error = Server::new(config)
            .serve(Router::new())
            .await
            .expect_err("a zero poll interval must refuse to start the server");

        let message = error.to_string();
        assert!(
            message.contains("reload_interval_secs = 0"),
            "the error must name the offending setting, got: {message}"
        );
        assert!(
            message.contains("[tls]"),
            "the error must name the section to fix, got: {message}"
        );
    }
}
