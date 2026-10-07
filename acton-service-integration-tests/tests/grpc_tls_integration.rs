//! Live gRPC TLS regressions exercise the public facade and generated clients.
#![cfg(all(feature = "grpc", feature = "tls"))]

use acton_service::client_tls::ClientIdentitySource;
use acton_service::config::ClientIdentityConfig;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A self-signed certificate plus its PEM-encoded private key, usable both
/// as a CA trust anchor and as a client identity in tests.
struct TestCert {
    cert_pem: String,
    key_pem: String,
}

fn generate_cert(name: &str) -> TestCert {
    let certified = rcgen::generate_simple_self_signed(vec![name.to_string()])
        .expect("self-signed cert generation");
    TestCert {
        cert_pem: certified.cert.pem(),
        key_pem: certified.signing_key.serialize_pem(),
    }
}

/// Write cert and key PEM into named files under a directory the test owns,
/// so their contents can be rewritten in place to simulate a rotation.
fn write_identity(dir: &Path, cert: &TestCert) -> ClientIdentityConfig {
    let cert_path = dir.join("client.pem");
    let key_path = dir.join("client.key");
    std::fs::write(&cert_path, &cert.cert_pem).expect("write cert");
    std::fs::write(&key_path, &cert.key_pem).expect("write key");
    config_for(cert_path, key_path)
}

fn config_for(cert_path: PathBuf, key_path: PathBuf) -> ClientIdentityConfig {
    ClientIdentityConfig {
        enabled: true,
        cert_path,
        key_path,
        root_ca_path: None,
        exclusive_roots: false,
        connect_timeout_secs: None,
    }
}

/// A CA certificate and a leaf signed by it, both PEM-encoded.
struct TestChain {
    ca_pem: String,
    leaf_pem: String,
    leaf_key_pem: String,
}

/// Issue a CA and a server certificate for `127.0.0.1` under it.
///
/// A real two-level chain rather than a self-signed leaf, so the client's
/// peer verification exercises `build_root_store` and webpki path building
/// the way a deployment would.
fn generate_server_chain() -> TestChain {
    use rcgen::{
        BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, Issuer, KeyPair,
        KeyUsagePurpose,
    };

    let ca_key = KeyPair::generate().expect("ca key");
    let mut ca_params = CertificateParams::new(Vec::new()).expect("ca params");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let mut ca_dn = DistinguishedName::new();
    ca_dn.push(DnType::CommonName, "acton-service test CA");
    ca_params.distinguished_name = ca_dn;
    let ca_cert = ca_params.self_signed(&ca_key).expect("self-signed ca");

    let leaf_key = KeyPair::generate().expect("leaf key");
    let mut leaf_params =
        CertificateParams::new(vec!["127.0.0.1".to_string()]).expect("leaf params");
    let mut leaf_dn = DistinguishedName::new();
    leaf_dn.push(DnType::CommonName, "acton-service test server");
    leaf_params.distinguished_name = leaf_dn;

    let issuer = Issuer::new(ca_params, ca_key);
    let leaf_cert = leaf_params
        .signed_by(&leaf_key, &issuer)
        .expect("leaf signed by ca");

    TestChain {
        ca_pem: ca_cert.pem(),
        leaf_pem: leaf_cert.pem(),
        leaf_key_pem: leaf_key.serialize_pem(),
    }
}

// --- End-to-end gRPC-over-TLS regressions (#97, #98) ------------------
//
// These drive a real generated `tonic` client through the actual
// `Endpoint`/`Channel` machinery against a live TLS listener whose
// credentials come from `load_server_config`. Earlier connector-only tests
// never touched tonic's `Endpoint::connect_*` wrapper (#97) nor the
// listener's ALPN (#98), which is exactly where both bugs lived.

mod ping_proto {
    tonic::include_proto!("ping.v1");
}

#[derive(Clone, Default)]
struct PingImpl;

#[tonic::async_trait]
impl ping_proto::ping_service_server::PingService for PingImpl {
    async fn ping(
        &self,
        request: tonic::Request<ping_proto::PingRequest>,
    ) -> std::result::Result<tonic::Response<ping_proto::PongResponse>, tonic::Status> {
        Ok(tonic::Response::new(ping_proto::PongResponse {
            message: format!("pong: {}", request.into_inner().message),
            timestamp: 0,
        }))
    }
}

/// Serve the ping gRPC service over a TLS listener built from `server_config`
/// (the output of `load_server_config`). Returns the bound address and the
/// server task, which the caller aborts when done.
async fn spawn_grpc_tls_server(
    server_config: Arc<tokio_rustls::rustls::ServerConfig>,
) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = tcp.local_addr().expect("local addr");
    let listener = acton_service::tls::TlsListener::new(tcp, server_config);

    let routes = acton_service::grpc::server::GrpcServicesBuilder::new()
        .add_service(ping_proto::ping_service_server::PingServiceServer::new(
            PingImpl,
        ))
        .build::<()>(None);
    let app = routes.into_axum_router();

    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (addr, handle)
}

/// Write a CA-signed server leaf to disk and load it through
/// `load_server_config`, so the listener's config, ALPN included, is
/// exactly what production builds.
fn server_config_from_chain(
    dir: &Path,
    chain: &TestChain,
) -> Arc<tokio_rustls::rustls::ServerConfig> {
    let cert_path = dir.join("server.pem");
    let key_path = dir.join("server.key");
    std::fs::write(&cert_path, &chain.leaf_pem).expect("write server leaf");
    std::fs::write(&key_path, &chain.leaf_key_pem).expect("write server key");

    let tls_config = acton_service::config::TlsConfig {
        enabled: true,
        cert_path,
        key_path,
        client_ca_path: None,
        client_auth_optional: false,
        reload_interval_secs: None,
        reload_on_sighup: false,
        handshake_timeout_secs: None,
    };
    acton_service::tls::load_server_config(&tls_config).expect("server config loads")
}

/// Build a client identity that trusts only the test CA.
fn client_identity_trusting(dir: &Path, ca_pem: &str) -> ClientIdentityConfig {
    let client_cert = generate_cert("test-client");
    let mut identity = write_identity(dir, &client_cert);
    let ca_path = dir.join("server-ca.pem");
    std::fs::write(&ca_path, ca_pem).expect("write ca");
    identity.root_ca_path = Some(ca_path);
    identity.exclusive_roots = true;
    identity
}

/// #97: a real generated client, connected through
/// [`ClientIdentitySource::grpc_channel`], must complete an RPC. Before the
/// fix this fails with `HttpsUriWithoutTlsSupport` on the first call, because
/// tonic's `_tls-any` connector wrapper rejects the `https` URI before the
/// rotating connector ever runs.
#[tokio::test]
async fn grpc_channel_completes_a_real_rpc_over_a_tls_listener() {
    acton_service::crypto::ensure_default_crypto_provider();
    let dir = tempfile::tempdir().expect("temp dir");
    let chain = generate_server_chain();

    let server_config = server_config_from_chain(dir.path(), &chain);
    let (addr, server) = spawn_grpc_tls_server(server_config).await;

    let identity = client_identity_trusting(dir.path(), &chain.ca_pem);
    let source = ClientIdentitySource::from_config(&identity).expect("client identity source");

    let endpoint =
        tonic::transport::Endpoint::from_shared(format!("https://127.0.0.1:{}", addr.port()))
            .expect("valid endpoint");
    let channel = source
        .grpc_channel(endpoint)
        .expect("grpc_channel must build a channel for an https endpoint");

    let mut client = ping_proto::ping_service_client::PingServiceClient::new(channel);
    let response = client
        .ping(ping_proto::PingRequest {
            message: "hello".to_string(),
        })
        .await
        .expect("the RPC must succeed over the rotating TLS channel");

    assert_eq!(response.into_inner().message, "pong: hello");
    server.abort();
}

/// #98: a strict `tonic` client, one built from the crate's own
/// `tonic_client_tls_config`, which does not set `assume_http2`, must
/// negotiate HTTP/2 against the listener. Before the fix the listener
/// advertises no ALPN, so the eager `connect()` fails with `H2NotNegotiated`.
#[tokio::test]
async fn a_strict_tonic_client_negotiates_h2_via_server_alpn() {
    acton_service::crypto::ensure_default_crypto_provider();
    let dir = tempfile::tempdir().expect("temp dir");
    let chain = generate_server_chain();

    let server_config = server_config_from_chain(dir.path(), &chain);
    let (addr, server) = spawn_grpc_tls_server(server_config).await;

    let identity = client_identity_trusting(dir.path(), &chain.ca_pem);
    let source = ClientIdentitySource::from_config(&identity).expect("client identity source");
    let tls = source
        .tonic_client_tls_config_snapshot()
        .expect("tonic client TLS config");

    // Eager connect: tonic verifies the negotiated ALPN during the handshake,
    // so this is where a listener without ALPN fails.
    let channel =
        tonic::transport::Endpoint::from_shared(format!("https://127.0.0.1:{}", addr.port()))
            .expect("valid endpoint")
            .tls_config(tls)
            .expect("endpoint accepts the tonic TLS config")
            .connect()
            .await
            .expect("a strict tonic client must negotiate h2 once the server advertises ALPN");

    let mut client = ping_proto::ping_service_client::PingServiceClient::new(channel);
    let response = client
        .ping(ping_proto::PingRequest {
            message: "strict".to_string(),
        })
        .await
        .expect("the RPC over the strict tonic TLS channel must succeed");

    assert_eq!(response.into_inner().message, "pong: strict");
    server.abort();
}
