//! Every failed TLS handshake is counted on `tls.handshake_failures{kind}`,
//! and each kind is what a real peer of that shape produces.
//!
//! One test per kind, each driving a real peer against a real mutual-TLS
//! `TlsListener` and reading the counter back through the same Prometheus
//! handler a scrape uses. Assertions are on the delta of the test's own kind,
//! so the tests hold whether they run one per process (nextest) or as threads
//! sharing one registry (`cargo test`).

#![cfg(all(feature = "tls", feature = "prometheus-metrics"))]

use std::net::SocketAddr;
use std::sync::{Arc, Once};
use std::time::Duration;

use acton_service::tls::TlsListener;
use axum::serve::Listener as _;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
use tokio_rustls::rustls::server::WebPkiClientVerifier;
use tokio_rustls::rustls::{ClientConfig, RootCertStore, ServerConfig};
use tokio_rustls::TlsConnector;

/// Bounds every wait. A loopback handshake fails in milliseconds; the bound
/// is generous because it is only ever reached on failure, and a loaded CI
/// runner should not be what fails a test.
const SETTLE: Duration = Duration::from_secs(30);

/// The handshake timeout for the one test that waits it out: short, so it
/// does not dominate the suite.
const SHORT_HANDSHAKE_TIMEOUT: Duration = Duration::from_millis(300);

/// The handshake timeout for every other test. A certificate handshake on a
/// loaded machine can take longer than [`SHORT_HANDSHAKE_TIMEOUT`], and one
/// that did would be counted as `timeout` instead of the kind under test; no
/// test here waits this out.
const PATIENT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(60);

fn init_metrics() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let mut config = acton_service::config::Config::<()>::default();
        config.service.name = "tls-handshake-failure-kinds".to_string();
        acton_service::observability::init_meter_provider(&config)
            .expect("meter provider initializes");
    });
}

/// The `tls_handshake_failures_total` value for `kind` in the current scrape.
async fn failures(kind: &str) -> u64 {
    let response = acton_service::observability::metrics_handler().await;
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("scrape body");
    let body = String::from_utf8(body.to_vec()).expect("text exposition");
    let label = format!("kind=\"{kind}\"");
    body.lines()
        .filter(|line| line.starts_with("tls_handshake_failures_total{") && line.contains(&label))
        .filter_map(|line| line.rsplit(' ').next()?.parse::<u64>().ok())
        .sum()
}

/// Waits until the `kind` count reaches `target`.
async fn counted(kind: &str, target: u64) {
    let deadline = tokio::time::Instant::now() + SETTLE;
    loop {
        let now = failures(kind).await;
        if now >= target {
            assert_eq!(now, target, "exactly one {kind} failure is counted");
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no {kind} failure was counted (count {now}, want {target})"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

struct Identity {
    chain: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
}

struct Pki {
    ca: CertificateDer<'static>,
    server: Identity,
    client: Identity,
    /// Signed by a CA the server does not trust.
    rogue_client: Identity,
}

fn certificate_authority(name: &str) -> (CertificateDer<'static>, Issuer<'static, KeyPair>) {
    let key = KeyPair::generate().expect("ca key");
    let mut params = CertificateParams::new(Vec::new()).expect("ca params");
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, name);
    params.distinguished_name = dn;
    let cert = params.self_signed(&key).expect("self-signed ca");
    (cert.der().clone(), Issuer::new(params, key))
}

fn leaf(name: &str, issuer: &Issuer<'static, KeyPair>) -> Identity {
    let key = KeyPair::generate().expect("leaf key");
    let params = CertificateParams::new(vec![name.to_string()]).expect("leaf params");
    let cert = params.signed_by(&key, issuer).expect("leaf signed");
    Identity {
        chain: vec![cert.der().clone()],
        key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
    }
}

fn pki() -> Pki {
    let (ca, issuer) = certificate_authority("handshake-kinds CA");
    let (_, rogue) = certificate_authority("rogue CA");
    Pki {
        ca,
        server: leaf("localhost", &issuer),
        client: leaf("client", &issuer),
        rogue_client: leaf("client", &rogue),
    }
}

/// A mutual-TLS listener requiring a client certificate from the test CA,
/// accepting in the background. Returns its address and a receiver that
/// yields once per completed handshake.
async fn serve(pki: &Pki, handshake_timeout: Duration) -> (SocketAddr, UnboundedReceiver<()>) {
    acton_service::crypto::ensure_default_crypto_provider();
    init_metrics();

    let mut roots = RootCertStore::empty();
    roots.add(pki.ca.clone()).expect("trust the test CA");
    let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
        .build()
        .expect("client verifier");
    let config = ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(pki.server.chain.clone(), pki.server.key.clone_key())
        .expect("server config");

    let tcp = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = tcp.local_addr().expect("addr");
    let mut listener =
        TlsListener::new(tcp, Arc::new(config)).with_handshake_timeout(handshake_timeout);
    let (accepted, accepts) = unbounded_channel();
    tokio::spawn(async move {
        loop {
            let _connection = listener.accept().await;
            let _ = accepted.send(());
        }
    });
    (addr, accepts)
}

/// A TLS client trusting the test CA, presenting `identity` if given.
async fn tls_client(pki: &Pki, addr: SocketAddr, identity: Option<&Identity>) {
    let mut roots = RootCertStore::empty();
    roots.add(pki.ca.clone()).expect("trust the test CA");
    let builder = ClientConfig::builder().with_root_certificates(roots);
    let config = match identity {
        Some(id) => builder
            .with_client_auth_cert(id.chain.clone(), id.key.clone_key())
            .expect("client identity"),
        None => builder.with_no_client_auth(),
    };
    let tcp = TcpStream::connect(addr).await.expect("connect");
    let name = ServerName::try_from("localhost").expect("server name");
    // Under TLS 1.3 the client can finish its side before the server rejects
    // its certificate, so the refusal shows up on the first read, if at all.
    if let Ok(mut stream) = TlsConnector::from(Arc::new(config))
        .connect(name, tcp)
        .await
    {
        let mut buf = [0u8; 1];
        let _ = tokio::time::timeout(SETTLE, stream.read(&mut buf)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plaintext_http_on_the_tls_port_is_plaintext() {
    let pki = pki();
    let (addr, _accepts) = serve(&pki, PATIENT_HANDSHAKE_TIMEOUT).await;
    let before = failures("plaintext").await;

    let mut tcp = TcpStream::connect(addr).await.expect("connect");
    tcp.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .expect("write");
    let mut sink = Vec::new();
    let _ = tokio::time::timeout(SETTLE, tcp.read_to_end(&mut sink)).await;

    counted("plaintext", before + 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_without_a_certificate_is_no_client_cert() {
    let pki = pki();
    let (addr, _accepts) = serve(&pki, PATIENT_HANDSHAKE_TIMEOUT).await;
    let before = failures("no_client_cert").await;

    tls_client(&pki, addr, None).await;

    counted("no_client_cert", before + 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_certificate_from_an_untrusted_ca_is_bad_cert() {
    let pki = pki();
    let (addr, _accepts) = serve(&pki, PATIENT_HANDSHAKE_TIMEOUT).await;
    let before = failures("bad_cert").await;

    tls_client(&pki, addr, Some(&pki.rogue_client)).await;

    counted("bad_cert", before + 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_peer_that_never_sends_a_client_hello_is_timeout() {
    let pki = pki();
    let (addr, _accepts) = serve(&pki, SHORT_HANDSHAKE_TIMEOUT).await;
    let before = failures("timeout").await;

    // Held open, silent, past the handshake timeout.
    let _silent = TcpStream::connect(addr).await.expect("connect");

    counted("timeout", before + 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_peer_that_hangs_up_mid_handshake_is_eof() {
    let pki = pki();
    let (addr, _accepts) = serve(&pki, PATIENT_HANDSHAKE_TIMEOUT).await;
    let before = failures("eof").await;

    // Half a record header, then the write side closes.
    let mut tcp = TcpStream::connect(addr).await.expect("connect");
    tcp.write_all(&[0x16, 0x03]).await.expect("write");
    tcp.shutdown().await.expect("close the write side");

    counted("eof", before + 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_malformed_handshake_record_is_other() {
    let pki = pki();
    let (addr, _accepts) = serve(&pki, PATIENT_HANDSHAKE_TIMEOUT).await;
    let before = failures("other").await;

    // A well-formed handshake record carrying a message type that does not
    // exist: TLS on the wire, just not a ClientHello.
    let mut tcp = TcpStream::connect(addr).await.expect("connect");
    tcp.write_all(&[0x16, 0x03, 0x01, 0x00, 0x04, 0xfe, 0x00, 0x00, 0x00])
        .await
        .expect("write");
    let mut sink = Vec::new();
    let _ = tokio::time::timeout(SETTLE, tcp.read_to_end(&mut sink)).await;

    counted("other", before + 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_trusted_client_is_not_a_failure() {
    let pki = pki();
    let (addr, mut accepts) = serve(&pki, PATIENT_HANDSHAKE_TIMEOUT).await;
    let kinds = [
        "plaintext",
        "no_client_cert",
        "bad_cert",
        "timeout",
        "eof",
        "other",
    ];
    let mut before = Vec::new();
    for kind in kinds {
        before.push(failures(kind).await);
    }

    let mut roots = RootCertStore::empty();
    roots.add(pki.ca.clone()).expect("trust the test CA");
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_client_auth_cert(pki.client.chain.clone(), pki.client.key.clone_key())
        .expect("client identity");
    let tcp = TcpStream::connect(addr).await.expect("connect");
    let name = ServerName::try_from("localhost").expect("server name");
    let _stream = tokio::time::timeout(
        SETTLE,
        TlsConnector::from(Arc::new(config)).connect(name, tcp),
    )
    .await
    .expect("the handshake finishes within the bound")
    .expect("a trusted client completes the handshake");
    // The listener hands a connection out only once its side of the
    // handshake has finished, so every counter it could touch is settled.
    tokio::time::timeout(SETTLE, accepts.recv())
        .await
        .expect("the listener accepts the trusted client")
        .expect("the accept loop is running");

    for (kind, was) in kinds.iter().zip(before) {
        // Other tests may share this registry under `cargo test`, so only a
        // quiet registry can prove a negative; under nextest it always is.
        if was == 0 {
            assert_eq!(
                failures(kind).await,
                0,
                "a good handshake counted as {kind}"
            );
        }
    }
}
