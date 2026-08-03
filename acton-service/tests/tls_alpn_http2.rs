//! The TLS listener must serve every protocol it advertises in ALPN.
//!
//! `tls::load_server_config` offers `[h2, http/1.1]`. A client that honours
//! ALPN — curl at its defaults, any browser, Go's `net/http` — takes the h2
//! offer and sends the HTTP/2 connection preface. If the server `axum::serve`
//! builds cannot answer that preface the failure is silent on both ends: the
//! handshake succeeds, then the connection is dropped before any span opens, so
//! the server logs nothing at any level and the operator sees only a
//! TLS-looking error client-side.
//!
//! Whether the listener speaks h2 is decided by `hyper-util`'s `http2` feature,
//! which reaches the auto server through axum's `http2` feature (see the
//! workspace `Cargo.toml`). That makes this file's coverage fragile in a
//! specific way: any dependency that turns `hyper-util/http2` on for the test
//! build — `reqwest/http2`, `tonic`, a plain `hyper` client with h2 — also
//! turns it on for the server under test, and these tests would then pass
//! whatever the workspace declared. The h2 client here is therefore hand-rolled
//! over `tokio-rustls` and this file adds no dependency of its own. Keep it
//! that way, and run it at least once with `--no-default-features --features
//! http,tls,crypto-aws-lc-rs` where nothing else supplies h2.
#![cfg(feature = "tls")]

use std::io::Write as _;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use acton_service::config::TlsConfig;
use acton_service::tls::{load_server_config, TlsListener};
use axum::routing::get;
use axum::Router;
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, ServerName};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// The HTTP/2 connection preface every h2 client sends first (RFC 9113 §3.4).
const H2_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
/// An empty SETTINGS frame: 24-bit length, type, flags, 31-bit stream id.
const H2_EMPTY_SETTINGS: &[u8] = &[0, 0, 0, 0x04, 0, 0, 0, 0, 0];
/// Frame type SETTINGS, which a real h2 server must send before anything else.
const H2_FRAME_TYPE_SETTINGS: u8 = 0x04;

/// Bound generously: this only has to outlast a loopback round trip, and a
/// server with no h2 support answers (or hangs up) immediately.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

fn write_temp(contents: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().expect("temp file");
    file.write_all(contents.as_bytes()).expect("write temp");
    file.flush().expect("flush temp");
    file
}

/// Serves `GET /health` over TLS on an ephemeral loopback port.
///
/// Returns the bound address and the certificate the client must trust. The
/// serving task is detached; it ends with the test process.
async fn serve_tls() -> (SocketAddr, String) {
    let certified =
        rcgen::generate_simple_self_signed(vec!["localhost".to_string(), "127.0.0.1".to_string()])
            .expect("self-signed cert generation");
    let cert_pem = certified.cert.pem();
    let cert_file = write_temp(&cert_pem);
    let key_file = write_temp(&certified.signing_key.serialize_pem());

    let tls_config = TlsConfig {
        enabled: true,
        cert_path: cert_file.path().to_path_buf(),
        key_path: key_file.path().to_path_buf(),
        client_ca_path: None,
        client_auth_optional: false,
        reload_interval_secs: None,
        reload_on_sighup: false,
        handshake_timeout_secs: None,
    };
    // Also installs the process-wide crypto provider the client below needs.
    let server_config = load_server_config(&tls_config).expect("server config builds");

    let tcp = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let addr = tcp.local_addr().expect("local addr");
    let listener = TlsListener::new(tcp, server_config);

    let app = Router::new().route("/health", get(|| async { "ok" }));
    tokio::spawn(async move {
        let _ = axum::serve(listener, app.into_make_service()).await;
    });

    (addr, cert_pem)
}

/// Completes a TLS handshake offering exactly `alpn`, and reports what the
/// listener selected alongside the connected stream.
async fn connect(
    addr: SocketAddr,
    cert_pem: &str,
    alpn: &[&[u8]],
) -> (tokio_rustls::client::TlsStream<TcpStream>, Option<Vec<u8>>) {
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(CertificateDer::from_pem_slice(cert_pem.as_bytes()).expect("parse test cert"))
        .expect("trust test cert");

    let mut config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();

    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let tcp = TcpStream::connect(addr).await.expect("connect loopback");
    let stream = connector
        .connect(ServerName::try_from("localhost").expect("server name"), tcp)
        .await
        .expect("TLS handshake");

    let negotiated = stream.get_ref().1.alpn_protocol().map(<[u8]>::to_vec);
    (stream, negotiated)
}

/// The regression this file exists for.
///
/// The client offers `[h2, http/1.1]`, the listener answers `h2`, and the
/// listener must then behave like an HTTP/2 server: a real one replies to the
/// preface with its own SETTINGS frame. A listener built without h2 support
/// hands the preface to its HTTP/1.1 parser, which rejects it — the connection
/// closes or answers in HTTP/1.1 text, and either way no SETTINGS frame
/// arrives.
#[tokio::test]
async fn tls_listener_serves_http2_when_alpn_selects_it() {
    let (addr, cert_pem) = serve_tls().await;
    let (mut stream, negotiated) = connect(addr, &cert_pem, &[b"h2", b"http/1.1"]).await;

    assert_eq!(
        negotiated.as_deref(),
        Some(b"h2".as_ref()),
        "the listener must select h2 for a client that offers it"
    );

    stream.write_all(H2_PREFACE).await.expect("write preface");
    stream
        .write_all(H2_EMPTY_SETTINGS)
        .await
        .expect("write client SETTINGS");
    stream.flush().await.expect("flush");

    let mut header = [0u8; 9];
    let read = tokio::time::timeout(REPLY_TIMEOUT, stream.read_exact(&mut header)).await;

    match read {
        Err(_) => panic!(
            "the listener selected h2 and then never answered the HTTP/2 preface: \
             it is serving HTTP/1.1 only"
        ),
        Ok(Err(e)) => panic!(
            "the listener selected h2 and then dropped the connection ({e}): \
             it is serving HTTP/1.1 only"
        ),
        Ok(Ok(_)) => assert_eq!(
            header[3], H2_FRAME_TYPE_SETTINGS,
            "an HTTP/2 server answers the preface with a SETTINGS frame, \
             got frame type {:#04x} (bytes {header:?})",
            header[3]
        ),
    }
}

/// Serving h2 must not cost the clients that ask for HTTP/1.1.
#[tokio::test]
async fn tls_listener_still_serves_http1_clients() {
    let (addr, cert_pem) = serve_tls().await;
    let (mut stream, negotiated) = connect(addr, &cert_pem, &[b"http/1.1"]).await;

    assert_eq!(
        negotiated.as_deref(),
        Some(b"http/1.1".as_ref()),
        "the listener must still select http/1.1 for a client that offers only that"
    );

    stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("write request");
    stream.flush().await.expect("flush");

    let mut response = Vec::new();
    tokio::time::timeout(REPLY_TIMEOUT, stream.read_to_end(&mut response))
        .await
        .expect("the listener must answer an http/1.1 request")
        .expect("read response");

    let text = String::from_utf8_lossy(&response);
    assert!(
        text.starts_with("HTTP/1.1 200 OK"),
        "expected a 200 over HTTP/1.1, got: {text}"
    );
    assert!(
        text.ends_with("ok"),
        "expected the handler body, got: {text}"
    );
}
