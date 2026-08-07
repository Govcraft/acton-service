//! A configured exporter in a build without `prometheus-metrics` must refuse
//! to start, and must refuse *before* binding anything.
//!
//! Without the feature there is no registry, so the socket would answer 503
//! forever -- a listener that is running and useless, the hardest shape of
//! failure to diagnose from the outside. The refusal happens at config
//! resolution, which these tests prove by occupying the service port first:
//! if the code bound before validating, the error would be "address in use"
//! instead of the one naming the feature.
//!
//! This file only executes in a build without `prometheus-metrics`; the
//! `minimal` CI leg is what runs it.

#![cfg(not(feature = "prometheus-metrics"))]

use acton_service::config::{Config, MetricsConfig, MetricsExporterConfig};
use acton_service::prelude::{Server, ServiceBuilder};
use axum::Router;
use std::net::{IpAddr, Ipv4Addr};

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

fn config_with_exporter(service_port: u16) -> Config<()> {
    let mut config = Config::<()>::default();
    config.service.bind = LOOPBACK;
    config.service.port = service_port;
    config.middleware.metrics =
        Some(MetricsConfig::new().with_exporter(MetricsExporterConfig::new(LOOPBACK, 9091)));
    config
}

// Multi-thread flavor: in builds that carry the `audit` feature, the default
// config enables the audit agent, whose own current-thread-runtime guard would
// otherwise be recorded first and mask the refusal under test.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn try_build_refuses_an_exporter_this_build_cannot_serve() {
    let error = ServiceBuilder::new()
        .with_config(config_with_exporter(8080))
        .try_build()
        .err()
        .expect("an exporter table without the feature must be a startup error");

    let message = error.to_string();
    assert!(
        message.contains("prometheus-metrics"),
        "the error must name the missing feature, got: {message}"
    );
    assert!(
        message.contains("[middleware.metrics.exporter]"),
        "the error must name the section to fix, got: {message}"
    );
}

#[tokio::test]
async fn server_refuses_the_exporter_before_binding() {
    // Held open across the call: if `serve` bound its listener before
    // validating the exporter table, this occupation would surface as
    // "address in use" rather than the refusal under test.
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").expect("occupy a port");
    let port = occupied.local_addr().expect("occupied addr").port();

    let error = Server::new(config_with_exporter(port))
        .serve(Router::new())
        .await
        .expect_err("an exporter table without the feature must refuse to start");

    let message = error.to_string();
    assert!(
        message.contains("prometheus-metrics"),
        "the refusal must precede the bind and name the feature, got: {message}"
    );
}
