//! The defect issue #129 describes, as a test: a service whose main listener
//! is TLS must still be scrapeable in plain HTTP through the exporter.
//!
//! This is the one test in the suite that fails before the exporter existed:
//! the full `ServiceBuilder::serve` path runs with `[tls]` enabled and a
//! `[middleware.metrics.exporter]` table, and a cleartext client -- the shape
//! of a Fly.io or `PodMonitor` collector, which has no TLS knobs -- scrapes
//! the exporter while the same bytes are refused by the main listener.
//!
//! TLS material follows `tls_alpn_http2.rs` (rcgen + tempfile); the client is
//! a raw `TcpStream` for the same reason as there: cleartext on the wire is
//! the property under test, and a client library would hide it.

#![cfg(all(feature = "prometheus-metrics", feature = "tls"))]

use std::io::Write as _;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use acton_service::config::{Config, MetricsConfig, MetricsExporterConfig, TlsConfig};
use acton_service::prelude::ServiceBuilder;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
/// Bounds every wait in this file; loopback answers in microseconds.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);
/// How long the spawned `serve` gets to bind both listeners.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

fn write_temp(contents: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().expect("temp file");
    file.write_all(contents.as_bytes()).expect("write temp");
    file.flush().expect("flush temp");
    file
}

/// Two distinct free loopback ports, reserved together so the second cannot
/// be handed the first's port back.
fn two_free_ports() -> (u16, u16) {
    let first = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve first port");
    let second = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve second port");
    (
        first.local_addr().expect("first addr").port(),
        second.local_addr().expect("second addr").port(),
    )
}

/// One cleartext HTTP/1.1 GET /metrics, response returned verbatim.
async fn raw_scrape(addr: SocketAddr) -> std::io::Result<String> {
    let mut stream = TcpStream::connect(addr).await?;
    let request = format!("GET /metrics HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await?;

    let mut response = Vec::new();
    tokio::time::timeout(REPLY_TIMEOUT, stream.read_to_end(&mut response))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "no reply"))??;
    Ok(String::from_utf8_lossy(&response).into_owned())
}

// Multi-thread flavor: the default config enables the audit agent, which
// `build()` refuses on a current-thread runtime.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tls_service_is_scrapeable_in_plaintext_through_the_exporter() {
    let certified =
        rcgen::generate_simple_self_signed(vec!["localhost".to_string(), "127.0.0.1".to_string()])
            .expect("self-signed cert generation");
    let cert_file = write_temp(&certified.cert.pem());
    let key_file = write_temp(&certified.signing_key.serialize_pem());

    let (http_port, exporter_port) = two_free_ports();

    let mut config = Config::<()>::default();
    config.service.name = "tls-exporter-e2e".to_string();
    config.service.bind = LOOPBACK;
    config.service.port = http_port;
    config.tls = Some(TlsConfig {
        enabled: true,
        cert_path: cert_file.path().to_path_buf(),
        key_path: key_file.path().to_path_buf(),
        client_ca_path: None,
        client_auth_optional: false,
        reload_interval_secs: None,
        reload_on_sighup: false,
        handshake_timeout_secs: None,
    });
    config.middleware.metrics = Some(
        MetricsConfig::new().with_exporter(MetricsExporterConfig::new(LOOPBACK, exporter_port)),
    );

    let service = ServiceBuilder::new().with_config(config).build();
    let server = tokio::spawn(service.serve());

    // The exporter binds before the service listeners, so once it scrapes the
    // whole startup ordering has held.
    let exporter_addr = SocketAddr::new(LOOPBACK, exporter_port);
    let deadline = tokio::time::Instant::now() + STARTUP_TIMEOUT;
    let scrape = loop {
        match raw_scrape(exporter_addr).await {
            Ok(response) => break response,
            Err(_) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Err(e) => panic!("the exporter never came up: {e}"),
        }
    };

    assert!(
        scrape.starts_with("HTTP/1.1 200"),
        "the plaintext scrape must succeed while the main listener is TLS, got: {scrape}"
    );
    assert!(
        scrape.contains("text/plain"),
        "the scrape must be the Prometheus text exposition, got: {scrape}"
    );

    // The same cleartext bytes against the main listener must NOT produce an
    // HTTP success: that listener speaks TLS, and `GET ` is not a TLS record.
    let main_addr = SocketAddr::new(LOOPBACK, http_port);
    let refused = raw_scrape(main_addr).await;
    let plaintext_worked = matches!(&refused, Ok(response) if response.starts_with("HTTP/1.1 200"));
    assert!(
        !plaintext_worked,
        "the main listener must stay TLS-only; a cleartext 200 means the posture leaked: {refused:?}"
    );

    server.abort();
}
