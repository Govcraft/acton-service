//! The exporter listener must speak plain HTTP, serve exactly one route, and
//! stop with the service.
//!
//! The client is a raw `TcpStream` writing literal HTTP/1.1 bytes, following
//! `tls_alpn_http2.rs`: writing the request in cleartext and reading a parseable
//! response is what *proves* the listener is not TLS -- an HTTP client library
//! would hide exactly the property under test. This file adds no dependency of
//! its own.
//!
//! One test per file-process concern: the meter provider is a process-wide
//! `OnceCell` (see `metrics_config_reaches_the_scrape.rs`), and nextest's
//! process-per-test isolation is what makes initializing it per test workable.

#![cfg(feature = "prometheus-metrics")]

use std::net::SocketAddr;
use std::time::Duration;

use acton_service::metrics_exporter::MetricsExporter;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Bounds a loopback round trip; a healthy exporter answers immediately.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

async fn start_exporter() -> MetricsExporter {
    MetricsExporter::start("127.0.0.1:0".parse().expect("loopback addr"))
        .await
        .expect("the exporter binds an ephemeral loopback port")
}

/// One cleartext HTTP/1.1 request, response returned verbatim.
async fn raw_get(addr: SocketAddr, path: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.expect("connect exporter");
    let request = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write request");

    let mut response = Vec::new();
    tokio::time::timeout(REPLY_TIMEOUT, stream.read_to_end(&mut response))
        .await
        .expect("the exporter must answer within the bound")
        .expect("read response");
    String::from_utf8_lossy(&response).into_owned()
}

#[tokio::test]
async fn a_plaintext_scrape_returns_the_prometheus_document() {
    let mut config = acton_service::config::Config::<()>::default();
    config.service.name = "exporter-plaintext-e2e".to_string();
    acton_service::observability::init_meter_provider(&config).expect("meter provider initializes");

    // One recorded measurement, so the assertion below is about a document
    // with content rather than a technically-200 empty registry.
    acton_service::observability::get_meter()
        .expect("the meter provider was just initialized")
        .u64_counter("exporter_e2e_probe")
        .build()
        .add(1, &[]);

    let exporter = start_exporter().await;
    let response = raw_get(exporter.local_addr(), "/metrics").await;

    assert!(
        response.starts_with("HTTP/1.1 200"),
        "a cleartext scrape must succeed, got: {response}"
    );
    assert!(
        response.contains("text/plain"),
        "the response must carry the Prometheus text exposition content type, got: {response}"
    );
    let body = response.split("\r\n\r\n").nth(1).unwrap_or("");
    assert!(
        body.contains("exporter_e2e_probe"),
        "the scrape must render the registry the service records into, got: {body}"
    );

    exporter.shutdown().await;
}

#[tokio::test]
async fn every_other_path_is_404() {
    let exporter = start_exporter().await;

    let response = raw_get(exporter.local_addr(), "/health").await;

    assert!(
        response.starts_with("HTTP/1.1 404"),
        "the exporter must not carry the application's routes, got: {response}"
    );

    exporter.shutdown().await;
}

#[tokio::test]
async fn shutdown_closes_the_listener() {
    let exporter = start_exporter().await;
    let addr = exporter.local_addr();

    exporter.shutdown().await;

    assert!(
        TcpStream::connect(addr).await.is_err(),
        "a drained exporter must refuse new connections"
    );
}

#[tokio::test]
async fn dropping_the_handle_also_closes_the_listener() {
    let exporter = start_exporter().await;
    let addr = exporter.local_addr();

    // The abort-on-drop backstop for a cancelled serve future. Abort lands
    // asynchronously, so poll rather than assert the very next instant.
    drop(exporter);

    for _ in 0..200 {
        if TcpStream::connect(addr).await.is_err() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the listener must close after its handle is dropped");
}
