//! A dedicated, plaintext listener that serves only `GET /metrics`.
//!
//! The main listener's `/metrics` route inherits whatever that listener is:
//! TLS, auth, CORS, compression, body limits. That is correct for an
//! application surface and wrong for a scrape target, because the collectors
//! that matter in practice — Fly.io's `[[metrics]]` block, GKE managed
//! collection, the defaults of most `PodMonitor`/`ServiceMonitor` resources —
//! speak plain HTTP to a declared port and expose no TLS knobs at all. A
//! service that terminates TLS on its main listener therefore cannot be
//! scraped through it, and the usual workaround is to stop terminating TLS.
//!
//! This module offers the other answer: an opt-in second socket carrying the
//! same bytes, from the same registry, through the same
//! [`crate::observability::metrics_handler`]. It is configured by
//! `[middleware.metrics.exporter]` and is absent unless that table is written.
//!
//! ```toml
//! [middleware.metrics.exporter]
//! bind = "::"
//! port = 9090
//! ```
//!
//! Both keys are required, the listener carries no TLS and no authentication,
//! and every path other than `GET /metrics` is a 404. Bind it to a private
//! scrape network.

use std::net::SocketAddr;

use crate::config::MetricsConfig;
use crate::error::{Error, Result};

/// The section name used in every diagnostic this module emits.
const SECTION: &str = "[middleware.metrics.exporter]";

/// Resolve the exporter listener address from `[middleware.metrics]`.
///
/// Returns `Ok(None)` when no exporter is configured, which is the common case
/// and not a problem. Returns the validated address otherwise.
///
/// Pure: derives its verdict solely from its arguments; performs no I/O and
/// reads no globals. Callers run it before binding anything, so a rejected
/// configuration refuses to start rather than surfacing as a scrape that
/// silently never arrives.
///
/// # Errors
///
/// - The table is configured but the `prometheus-metrics` feature is not
///   compiled in, so there is no registry to serve and the socket would answer
///   nothing useful.
/// - `port = 0`, which binds an OS-assigned ephemeral port that no scrape
///   configuration can name.
/// - The port collides with the HTTP listener or the separate-port gRPC
///   listener. The check is on the port alone, deliberately: `::` and `0.0.0.0`
///   overlap on the same port under Linux's dual-stack default, so "a different
///   address" is not a reliable escape, and no legitimate deployment wants the
///   exporter racing another listener for a bind.
pub(crate) fn resolve_exporter_addr(
    metrics: Option<&MetricsConfig>,
    http_addr: SocketAddr,
    grpc_addr: Option<SocketAddr>,
) -> Result<Option<SocketAddr>> {
    let Some(exporter) = metrics.and_then(|m| m.exporter.as_ref()) else {
        return Ok(None);
    };

    #[cfg(not(feature = "prometheus-metrics"))]
    {
        let _ = (exporter, http_addr, grpc_addr);
        Err(Error::Internal(format!(
            "{SECTION} is configured, but this binary was built without the \
             `prometheus-metrics` feature. There is no Prometheus registry to serve, so the \
             exporter listener would answer every scrape with 503 and the configuration would \
             be silently untrue. To fix, rebuild with `--features prometheus-metrics` (or \
             `--features full`), or remove the {SECTION} table."
        )))
    }

    #[cfg(feature = "prometheus-metrics")]
    {
        if exporter.port == 0 {
            return Err(Error::Internal(format!(
                "{SECTION} sets `port = 0`. Port 0 asks the operating system for an arbitrary \
                 ephemeral port, which no scrape configuration can name and which changes on \
                 every restart. Set an explicit port."
            )));
        }

        let addr = exporter.socket_addr();

        if exporter.port == http_addr.port() {
            return Err(Error::Internal(format!(
                "{SECTION} sets `port = {}`, the same port the HTTP listener binds ({http_addr}). \
                 Two listeners cannot share a port, and a differing bind address is not an \
                 escape: `::` and `0.0.0.0` overlap on the same port under Linux's dual-stack \
                 default. Give the exporter a port of its own, or remove the {SECTION} table and \
                 scrape `/metrics` on the main listener.",
                exporter.port
            )));
        }

        if let Some(grpc_addr) = grpc_addr {
            if exporter.port == grpc_addr.port() {
                return Err(Error::Internal(format!(
                    "{SECTION} sets `port = {}`, the same port the separate-port gRPC listener \
                     binds ({grpc_addr}). Two listeners cannot share a port. Give the exporter a \
                     port of its own, or change `[grpc] port`.",
                    exporter.port
                )));
            }
        }

        Ok(Some(addr))
    }
}

/// Warn when the exporter will serve a document with no HTTP instruments in it.
///
/// `[middleware.metrics] enabled = false` suppresses the HTTP metrics *layer*,
/// not the registry: `init_meter_provider` still installs the Prometheus reader,
/// and API-version counters plus anything an application records through
/// `get_meter()` still land there. So the combination is legitimate rather than
/// contradictory, and must not refuse to start — but an operator who configured
/// an exporter and then finds no `http_server_*` families should be told why
/// once, at startup, instead of going looking.
///
/// Pure: derives its verdict solely from its argument.
pub(crate) fn exporter_without_http_instruments(metrics: Option<&MetricsConfig>) -> bool {
    metrics.is_some_and(|m| m.exporter.is_some() && !m.enabled)
}

/// Emit the [`exporter_without_http_instruments`] warning if it applies.
pub(crate) fn warn_if_http_instruments_are_absent(metrics: Option<&MetricsConfig>) {
    if exporter_without_http_instruments(metrics) {
        tracing::warn!(
            "{SECTION} is configured while `[middleware.metrics] enabled = false`. The exporter \
             will serve, but the HTTP request instruments are not installed, so the scrape will \
             carry only API-version and application metrics."
        );
    }
}

#[cfg(feature = "prometheus-metrics")]
pub use runtime::{exporter_router, MetricsExporter};

#[cfg(feature = "prometheus-metrics")]
mod runtime {
    use super::SECTION;
    use crate::error::{Error, Result};
    use std::net::SocketAddr;
    use std::time::Duration;
    use tokio::sync::oneshot;
    use tokio::task::JoinHandle;

    /// How long [`MetricsExporter::shutdown`] waits for in-flight scrapes.
    ///
    /// A scrape is one in-memory encode of the registry, so it completes in
    /// microseconds. Anything approaching this bound is a stuck peer holding
    /// the connection open, not a slow render, and shutdown must not wait on
    /// it.
    const DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

    /// The exporter's entire route table: `GET /metrics`, nothing else.
    ///
    /// Deliberately unlayered. No CORS, no compression, no tracing, no timeout,
    /// no body limit, no connect-info: every one of those exists to serve an
    /// application surface, and none of them helps a scraper. Unmatched paths
    /// get axum's 404 and a non-`GET` `/metrics` gets 405.
    ///
    /// The handler is [`crate::observability::metrics_handler`] itself — the
    /// same function the main listener's route uses — so the two sockets cannot
    /// drift into serving different documents.
    pub fn exporter_router() -> axum::Router {
        axum::Router::new().route(
            "/metrics",
            axum::routing::get(crate::observability::metrics_handler),
        )
    }

    /// A bound, running exporter listener.
    ///
    /// Created by [`MetricsExporter::start`], which binds before returning, and
    /// stopped by [`MetricsExporter::shutdown`]. Dropping the handle without
    /// calling `shutdown` aborts the serving task, so a cancelled `serve`
    /// future cannot leave the socket open.
    pub struct MetricsExporter {
        addr: SocketAddr,
        stop: Option<oneshot::Sender<()>>,
        task: Option<JoinHandle<std::io::Result<()>>>,
    }

    impl MetricsExporter {
        /// Bind `addr` and start serving.
        ///
        /// Binding happens here rather than inside the spawned task so that a
        /// port already in use is a refusal to start, reported to the caller,
        /// rather than a background task that logs an error into a process
        /// which then runs on believing it is observable.
        ///
        /// # Errors
        ///
        /// Returns [`Error::Internal`] if the address cannot be bound. The
        /// message names the address and the config section, which the bare
        /// `std::io::Error` ("Address already in use (os error 98)") does not.
        pub async fn start(addr: SocketAddr) -> Result<Self> {
            let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
                Error::Internal(format!(
                    "the Prometheus exporter listener could not bind {addr}, configured by \
                     {SECTION}: {e}"
                ))
            })?;

            // `local_addr` rather than `addr`: they differ when the caller
            // asked for port 0, which `resolve_exporter_addr` rejects for
            // configuration but which tests legitimately use.
            let addr = listener.local_addr().unwrap_or(addr);

            if crate::observability::PROMETHEUS_REGISTRY.get().is_none() {
                tracing::warn!(
                    %addr,
                    "Prometheus exporter is starting but no registry has been initialised; \
                     scrapes will return 503 until `observability::init_meter_provider` runs. \
                     The `ServiceBuilder` path calls it during `build()`; a bare `Server` does \
                     not, and must call it itself."
                );
            }

            let (stop, stopped) = oneshot::channel();
            let task = tokio::spawn(async move {
                axum::serve(listener, exporter_router().into_make_service())
                    .with_graceful_shutdown(async move {
                        let _ = stopped.await;
                    })
                    .await
            });

            tracing::info!(
                %addr,
                "Prometheus exporter listening (plaintext, GET /metrics only)"
            );

            Ok(Self {
                addr,
                stop: Some(stop),
                task: Some(task),
            })
        }

        /// The address the listener actually bound.
        pub fn local_addr(&self) -> SocketAddr {
            self.addr
        }

        /// Stop serving and wait for in-flight scrapes, bounded by
        /// `DRAIN_TIMEOUT`.
        ///
        /// Callers drain the exporter *after* the service's own listeners have
        /// finished, so the final scrape can still observe the drain.
        pub async fn shutdown(mut self) {
            // Taken, so `Drop` sees `None` and does not abort a task that
            // finished on its own terms.
            let Some(task) = self.task.take() else {
                return;
            };
            drop(self.stop.take());

            match tokio::time::timeout(DRAIN_TIMEOUT, task).await {
                Ok(Ok(Ok(()))) => tracing::info!("Prometheus exporter shutdown complete"),
                Ok(Ok(Err(e))) => tracing::warn!(error = %e, "Prometheus exporter stopped with an error"),
                Ok(Err(e)) => tracing::warn!(error = %e, "Prometheus exporter task did not join cleanly"),
                Err(_) => tracing::warn!(
                    timeout_secs = DRAIN_TIMEOUT.as_secs(),
                    "Prometheus exporter did not drain in time; abandoning it"
                ),
            }
        }
    }

    impl Drop for MetricsExporter {
        fn drop(&mut self) {
            // Only reached when `shutdown` was not called — a cancelled
            // `serve` future, or a caller that dropped the handle. Abort rather
            // than signal: there is no runtime guarantee left to await on.
            if let Some(task) = self.task.take() {
                task.abort();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MetricsExporterConfig;
    use std::net::{IpAddr, Ipv4Addr};
    #[cfg(feature = "prometheus-metrics")]
    use std::net::Ipv6Addr;

    fn http_addr(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
    }

    fn metrics_with_exporter(bind: IpAddr, port: u16) -> MetricsConfig {
        MetricsConfig::new().with_exporter(MetricsExporterConfig::new(bind, port))
    }

    /// No `[middleware.metrics]` at all is the default posture and must stay
    /// silent: the exporter is opt-in twice over.
    #[test]
    fn absent_metrics_section_resolves_to_no_exporter() {
        let resolved = resolve_exporter_addr(None, http_addr(8080), None)
            .expect("an absent section is not an error");
        assert_eq!(resolved, None);
    }

    /// The metrics table without the sub-table is the pre-existing config of
    /// every deployment on 0.36 and earlier. It must keep meaning "one socket".
    #[test]
    fn metrics_section_without_exporter_resolves_to_no_exporter() {
        let metrics = MetricsConfig::new();
        let resolved = resolve_exporter_addr(Some(&metrics), http_addr(8080), None)
            .expect("an absent sub-table is not an error");
        assert_eq!(resolved, None);
    }

    #[cfg(feature = "prometheus-metrics")]
    #[test]
    fn configured_exporter_resolves_to_exactly_the_requested_address() {
        let metrics = metrics_with_exporter(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 9090);
        let resolved = resolve_exporter_addr(Some(&metrics), http_addr(8080), None)
            .expect("a valid exporter resolves");
        assert_eq!(
            resolved,
            Some(SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 9090)),
            "the resolved address must be what was configured, not a normalised form"
        );
    }

    /// Port 0 binds an ephemeral port that changes every restart, so no scrape
    /// job could ever name it. Accepting it would produce a listener that is
    /// running and unreachable, the hardest shape of failure to diagnose.
    #[cfg(feature = "prometheus-metrics")]
    #[test]
    fn port_zero_is_refused() {
        let metrics = metrics_with_exporter(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
        let error = resolve_exporter_addr(Some(&metrics), http_addr(8080), None)
            .expect_err("port 0 must be refused");

        let message = error.to_string();
        assert!(
            message.contains("port = 0"),
            "the error must name the offending setting, got: {message}"
        );
        assert!(
            message.contains(SECTION),
            "the error must name the section to fix, got: {message}"
        );
    }

    /// The dual-stack case the port-only rule exists for: a different bind
    /// address does not make the port free.
    #[cfg(feature = "prometheus-metrics")]
    #[test]
    fn colliding_with_the_http_port_is_refused_even_on_a_different_bind() {
        let metrics = metrics_with_exporter(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 8080);
        let error = resolve_exporter_addr(Some(&metrics), http_addr(8080), None)
            .expect_err("sharing the HTTP port must be refused");

        let message = error.to_string();
        assert!(
            message.contains(SECTION),
            "the error must name the exporter section, got: {message}"
        );
        assert!(
            message.contains("HTTP listener"),
            "the error must name the listener it collides with, got: {message}"
        );
    }

    #[cfg(feature = "prometheus-metrics")]
    #[test]
    fn colliding_with_the_separate_port_grpc_listener_is_refused() {
        let metrics = metrics_with_exporter(IpAddr::V4(Ipv4Addr::LOCALHOST), 50051);
        let error =
            resolve_exporter_addr(Some(&metrics), http_addr(8080), Some(http_addr(50051)))
                .expect_err("sharing the gRPC port must be refused");

        let message = error.to_string();
        assert!(
            message.contains("gRPC"),
            "the error must name the gRPC listener, got: {message}"
        );
    }

    /// A gRPC listener on its own port must not make an unrelated exporter port
    /// look like a collision.
    #[cfg(feature = "prometheus-metrics")]
    #[test]
    fn a_distinct_grpc_port_is_not_a_collision() {
        let metrics = metrics_with_exporter(IpAddr::V4(Ipv4Addr::LOCALHOST), 9090);
        let resolved =
            resolve_exporter_addr(Some(&metrics), http_addr(8080), Some(http_addr(50051)))
                .expect("distinct ports resolve");
        assert_eq!(resolved, Some(http_addr(9090)));
    }

    /// Without the feature there is no registry, so the socket would serve 503
    /// forever. The refusal names the feature, because "why is my scrape 503"
    /// is otherwise unanswerable from the config alone.
    #[cfg(not(feature = "prometheus-metrics"))]
    #[test]
    fn an_exporter_without_the_feature_is_refused() {
        let metrics = metrics_with_exporter(IpAddr::V4(Ipv4Addr::LOCALHOST), 9090);
        let error = resolve_exporter_addr(Some(&metrics), http_addr(8080), None)
            .expect_err("an exporter without the feature must be refused");

        let message = error.to_string();
        assert!(
            message.contains("prometheus-metrics"),
            "the error must name the missing feature, got: {message}"
        );
    }

    /// `enabled = false` plus an exporter is legal, not fatal — but it is worth
    /// one warning, and this is the predicate that decides it.
    #[test]
    fn exporter_with_the_http_layer_disabled_warrants_a_warning() {
        let metrics = metrics_with_exporter(IpAddr::V4(Ipv4Addr::LOCALHOST), 9090)
            .with_enabled(false);
        assert!(exporter_without_http_instruments(Some(&metrics)));
    }

    #[test]
    fn exporter_with_the_http_layer_enabled_warrants_no_warning() {
        let metrics = metrics_with_exporter(IpAddr::V4(Ipv4Addr::LOCALHOST), 9090);
        assert!(!exporter_without_http_instruments(Some(&metrics)));
    }

    #[test]
    fn a_disabled_metrics_layer_without_an_exporter_warrants_no_warning() {
        let metrics = MetricsConfig::new().with_enabled(false);
        assert!(!exporter_without_http_instruments(Some(&metrics)));
    }

    /// The route table is the security boundary: anything reachable here is
    /// reachable without TLS and without auth, so assert what is *not* there.
    #[cfg(feature = "prometheus-metrics")]
    #[tokio::test]
    async fn the_exporter_router_serves_metrics_and_nothing_else() {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt as _;

        let status_for = |method: &'static str, path: &'static str| async move {
            exporter_router()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .body(Body::empty())
                        .expect("request builds"),
                )
                .await
                .expect("router responds")
                .status()
        };

        assert_ne!(
            status_for("GET", "/metrics").await,
            StatusCode::NOT_FOUND,
            "GET /metrics must be routed"
        );
        assert_eq!(
            status_for("GET", "/healthz").await,
            StatusCode::NOT_FOUND,
            "the exporter must not carry the application's routes"
        );
        assert_eq!(
            status_for("POST", "/metrics").await,
            StatusCode::METHOD_NOT_ALLOWED,
            "the exporter is read-only"
        );
    }
}
