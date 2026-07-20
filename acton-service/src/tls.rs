//! TLS support using rustls
//!
//! Provides a [`TlsListener`] that wraps a TCP listener with TLS termination,
//! implementing [`axum::serve::Listener`] for seamless integration with axum's server.
//!
//! Credentials reach the listener through a [`TlsConfigSource`], a handle whose
//! contents can be replaced while the listener is running so certificates can be
//! rotated without dropping the socket.

use std::io;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use arc_swap::ArcSwap;
use rustls_pki_types::CertificateDer;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::rustls::server::danger::ClientCertVerifier;
use tokio_rustls::rustls::server::WebPkiClientVerifier;
use tokio_rustls::rustls::{RootCertStore, ServerConfig};
use tokio_rustls::server::TlsStream;
use tokio_rustls::TlsAcceptor;

use crate::config::TlsConfig;
use crate::error::Result;

/// The credentials a TLS listener serves, replaceable while it runs.
///
/// A source holds the current [`ServerConfig`] behind an [`ArcSwap`], so
/// [`load`](Self::load) is a cheap atomic read on the accept path and
/// [`reload`](Self::reload) can install fresh credentials without rebinding the
/// socket. Connections already established keep the configuration they
/// handshook with; every subsequent handshake uses the newest one.
///
/// Cloning is cheap and every clone observes the same credentials: hand clones
/// to the listener and keep one to drive rotation.
///
/// # Failure behaviour
///
/// A source built from a [`TlsConfig`] rereads those files on every reload. A
/// failed reload is **fail-closed**: the last-good configuration stays
/// installed, the error is logged at `ERROR` level and returned to the caller.
/// A reload can never leave the listener without credentials or downgrade what
/// it serves.
///
/// A source built from an already-loaded [`ServerConfig`] has no files to
/// reread, so it is static and [`reload`](Self::reload) reports that rather
/// than silently doing nothing.
///
/// # Example
///
/// ```rust,ignore
/// let source = TlsConfigSource::from_tls_config(&tls_config)?;
/// let listener = TlsListener::with_config_source(tcp, source.clone());
///
/// // Later, after the certificate files have been rewritten on disk:
/// if let Err(e) = source.reload() {
///     tracing::warn!("certificate rotation failed, still serving previous cert: {e}");
/// }
/// ```
#[derive(Clone)]
pub struct TlsConfigSource {
    inner: Arc<TlsConfigSourceInner>,
}

struct TlsConfigSourceInner {
    /// The configuration every new handshake uses.
    current: ArcSwap<ServerConfig>,
    /// The file paths a reload rereads. `None` for a static source.
    origin: Option<TlsConfig>,
}

impl TlsConfigSource {
    /// Create a static source from an already-built server configuration.
    ///
    /// The result never changes: [`reload`](Self::reload) has no files to read
    /// and returns an error. Use [`from_tls_config`](Self::from_tls_config)
    /// when the credentials must be rotatable.
    #[must_use]
    pub fn from_server_config(server_config: Arc<ServerConfig>) -> Self {
        Self {
            inner: Arc::new(TlsConfigSourceInner {
                current: ArcSwap::new(server_config),
                origin: None,
            }),
        }
    }

    /// Create a reloadable source by loading credentials from disk.
    ///
    /// Reads the certificate chain, private key and any client-CA bundle named
    /// by `tls_config` through [`load_server_config`], and remembers those paths
    /// so [`reload`](Self::reload) can reread them. Returns the load error if
    /// the initial read fails, leaving no source to serve from.
    pub fn from_tls_config(tls_config: &TlsConfig) -> Result<Self> {
        let server_config = load_server_config(tls_config)?;
        Ok(Self {
            inner: Arc::new(TlsConfigSourceInner {
                current: ArcSwap::new(server_config),
                origin: Some(tls_config.clone()),
            }),
        })
    }

    /// The configuration new handshakes currently use.
    ///
    /// A cheap atomic load: this sits on the accept path and does no I/O.
    #[must_use]
    pub fn load(&self) -> Arc<ServerConfig> {
        self.inner.current.load_full()
    }

    /// Whether [`reload`](Self::reload) can do anything for this source.
    ///
    /// `true` for a source built from a [`TlsConfig`], `false` for one built
    /// from an already-loaded [`ServerConfig`].
    #[must_use]
    pub fn is_reloadable(&self) -> bool {
        self.inner.origin.is_some()
    }

    /// The configuration this source reloads from, if it is reloadable.
    #[must_use]
    pub fn origin(&self) -> Option<&TlsConfig> {
        self.inner.origin.as_ref()
    }

    /// Reread the credential files and install them for new handshakes.
    ///
    /// On success every subsequent handshake uses the new configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the source is static (nothing to reread), or when
    /// the files fail to load or parse. In both cases the previously installed
    /// configuration stays in place and keeps serving: a rotation that produced
    /// an unusable certificate must not take the listener down with it. Failures
    /// are also logged at `ERROR` level, because a service whose rotation has
    /// silently stopped working will keep working until the certificate expires
    /// and then fail all at once.
    pub fn reload(&self) -> Result<()> {
        let Some(ref origin) = self.inner.origin else {
            let err = crate::error::Error::Internal(
                "TLS credentials cannot be reloaded: this source was built from an \
                 already-loaded ServerConfig and has no files to reread"
                    .to_string(),
            );
            tracing::error!("{}", err);
            return Err(err);
        };

        match load_server_config(origin) {
            Ok(server_config) => {
                self.inner.current.store(server_config);
                tracing::info!(
                    cert_path = %origin.cert_path.display(),
                    key_path = %origin.key_path.display(),
                    "TLS credentials reloaded; new handshakes use the new certificate"
                );
                Ok(())
            }
            Err(e) => {
                tracing::error!(
                    cert_path = %origin.cert_path.display(),
                    key_path = %origin.key_path.display(),
                    error = %e,
                    "TLS credential reload failed; continuing to serve the previous \
                     certificate. New credentials will not take effect until a reload \
                     succeeds."
                );
                Err(e)
            }
        }
    }
}

impl std::fmt::Debug for TlsConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `ServerConfig` carries key material, so describe the source by where
        // it came from rather than by what it holds.
        f.debug_struct("TlsConfigSource")
            .field("reloadable", &self.is_reloadable())
            .field("origin", &self.inner.origin)
            .finish()
    }
}

impl From<Arc<ServerConfig>> for TlsConfigSource {
    fn from(server_config: Arc<ServerConfig>) -> Self {
        Self::from_server_config(server_config)
    }
}

/// A TLS-enabled listener wrapping a [`TcpListener`], terminating TLS with the
/// credentials held by a [`TlsConfigSource`].
///
/// Implements [`axum::serve::Listener`] so it can be used as a drop-in
/// replacement for `TcpListener` when calling `axum::serve()`.
///
/// Each accepted connection reads the source's current configuration, so
/// credentials swapped in by [`TlsConfigSource::reload`] apply to the next
/// handshake without restarting the listener.
pub struct TlsListener {
    tcp: TcpListener,
    config_source: TlsConfigSource,
}

impl TlsListener {
    /// Create a TLS listener serving one fixed server configuration.
    ///
    /// The credentials cannot be rotated. Use
    /// [`with_config_source`](Self::with_config_source) with a source built by
    /// [`TlsConfigSource::from_tls_config`] when they must be.
    pub fn new(tcp: TcpListener, server_config: Arc<ServerConfig>) -> Self {
        Self::with_config_source(tcp, TlsConfigSource::from_server_config(server_config))
    }

    /// Create a TLS listener that reads its credentials from `config_source`.
    ///
    /// Every handshake uses whatever the source holds at that moment, so a
    /// [`reload`](TlsConfigSource::reload) on a clone of the source rotates this
    /// listener's certificate in place.
    pub fn with_config_source(tcp: TcpListener, config_source: TlsConfigSource) -> Self {
        Self { tcp, config_source }
    }

    /// The credential source this listener hands to each new handshake.
    #[must_use]
    pub fn config_source(&self) -> &TlsConfigSource {
        &self.config_source
    }
}

impl axum::serve::Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    fn accept(&mut self) -> impl std::future::Future<Output = (Self::Io, Self::Addr)> + Send {
        let config_source = self.config_source.clone();
        let tcp = &mut self.tcp;

        async move {
            loop {
                // Accept a TCP connection using the tokio TcpListener method (not
                // the axum Listener trait method, which handles errors internally).
                let (stream, addr) = match TcpListener::accept(tcp).await {
                    Ok((stream, addr)) => (stream, addr),
                    Err(e) => {
                        tracing::error!("TCP accept error: {}", e);
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        continue;
                    }
                };

                // Read the credentials per connection so a reload that happened
                // since the last accept takes effect. Both the load and the
                // acceptor construction are `Arc` clones, so this costs nothing
                // measurable against the handshake that follows.
                let acceptor = TlsAcceptor::from(config_source.load());

                // Perform TLS handshake. On failure, log and try the next connection.
                match acceptor.accept(stream).await {
                    Ok(tls_stream) => return (tls_stream, addr),
                    Err(e) => {
                        tracing::warn!("TLS handshake failed from {}: {}", addr, e);
                        continue;
                    }
                }
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.tcp.local_addr()
    }
}

/// Load a [`RootCertStore`] of client-CA certificates from a PEM bundle.
///
/// Every PEM-encoded certificate in the file is treated as a trust anchor for
/// verifying client certificates during a mutual-TLS handshake. Returns an
/// error if the file cannot be opened, cannot be parsed, or contains no
/// certificates (an empty trust store would silently reject every client).
pub fn load_client_ca_roots(path: &Path) -> Result<RootCertStore> {
    load_root_store(path, "client CA")
}

/// Load a [`RootCertStore`] of trust anchors from a PEM bundle.
///
/// `role` names what the bundle is for and appears verbatim in every error
/// message, so the same loader can serve both the server's client-CA bundle and
/// a client's peer-CA bundle without either producing a misleading diagnostic.
/// Returns an error if the file cannot be opened, cannot be parsed, or contains
/// no certificates: an empty trust store would silently reject every peer, so
/// it must be a load failure rather than a permissive default.
pub(crate) fn load_root_store(path: &Path, role: &str) -> Result<RootCertStore> {
    use rustls_pki_types::pem::PemObject;

    let ca_certs: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(path)
        .map_err(|e| {
            crate::error::Error::Internal(format!(
                "Failed to open {} file '{}': {}",
                role,
                path.display(),
                e
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            crate::error::Error::Internal(format!(
                "Failed to parse {} certificates from '{}': {}",
                role,
                path.display(),
                e
            ))
        })?;

    if ca_certs.is_empty() {
        return Err(crate::error::Error::Internal(format!(
            "The {} file '{}' contains no certificates",
            role,
            path.display()
        )));
    }

    let mut roots = RootCertStore::empty();
    for cert in ca_certs {
        roots.add(cert).map_err(|e| {
            crate::error::Error::Internal(format!(
                "Failed to add {} certificate from '{}' to trust store: {}",
                role,
                path.display(),
                e
            ))
        })?;
    }

    Ok(roots)
}

/// Build a rustls [`ClientCertVerifier`] from a set of trust anchors.
///
/// When `optional` is `true`, a client certificate is requested but not
/// required: connections without one still complete, and a presented
/// certificate is still verified. When `false`, a valid client certificate is
/// mandatory and the handshake fails without one.
pub fn build_client_verifier(
    roots: RootCertStore,
    optional: bool,
) -> Result<Arc<dyn ClientCertVerifier>> {
    // The crypto provider must be installed before the verifier builder runs,
    // for the same reason as `ServerConfig::builder()` below.
    crate::crypto::ensure_default_crypto_provider();

    let mut builder = WebPkiClientVerifier::builder(Arc::new(roots));
    if optional {
        builder = builder.allow_unauthenticated();
    }

    builder.build().map_err(|e| {
        crate::error::Error::Internal(format!(
            "Failed to build client certificate verifier: {}",
            e
        ))
    })
}

/// Load a rustls [`ServerConfig`] from PEM certificate and key files.
///
/// Reads the certificate chain and private key from disk and constructs a
/// server configuration. When [`TlsConfig::client_ca_path`] is set, the server
/// is configured for mutual TLS: client certificates are verified against that
/// CA bundle (required unless [`TlsConfig::client_auth_optional`] is set).
/// Otherwise no client authentication is requested.
pub fn load_server_config(tls_config: &TlsConfig) -> Result<Arc<ServerConfig>> {
    use rustls_pki_types::pem::PemObject;
    use rustls_pki_types::PrivateKeyDer;
    use tokio_rustls::rustls;

    // Read certificate chain
    let cert_chain: Vec<rustls::pki_types::CertificateDer<'static>> =
        CertificateDer::pem_file_iter(&tls_config.cert_path)
            .map_err(|e| {
                crate::error::Error::Internal(format!(
                    "Failed to open TLS cert file '{}': {}",
                    tls_config.cert_path.display(),
                    e
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| {
                crate::error::Error::Internal(format!("Failed to parse TLS certificates: {}", e))
            })?;

    if cert_chain.is_empty() {
        return Err(crate::error::Error::Internal(
            "TLS cert file contains no certificates".to_string(),
        ));
    }

    // Read private key (first PEM-encoded key found in the file)
    let key = PrivateKeyDer::from_pem_file(&tls_config.key_path).map_err(|e| {
        crate::error::Error::Internal(format!(
            "Failed to parse TLS private key from '{}': {}",
            tls_config.key_path.display(),
            e
        ))
    })?;

    // Install the chosen rustls crypto provider before any builder call.
    // Without this, `ServerConfig::builder()` panics when multiple providers
    // are compiled into the binary (common via transitive deps).
    crate::crypto::ensure_default_crypto_provider();

    // Select the client-authentication posture. A configured client CA bundle
    // switches the listener into mutual-TLS mode; its absence preserves the
    // prior server-only behaviour.
    let builder = ServerConfig::builder();
    let config = match tls_config.client_ca_path {
        Some(ref ca_path) => {
            let roots = load_client_ca_roots(ca_path)?;
            let verifier = build_client_verifier(roots, tls_config.client_auth_optional)?;
            builder.with_client_cert_verifier(verifier)
        }
        None => builder.with_no_client_auth(),
    }
    .with_single_cert(cert_chain, key)
    .map_err(|e| {
        crate::error::Error::Internal(format!("Failed to build TLS server config: {}", e))
    })?;

    Ok(Arc::new(config))
}

/// A verified client certificate chain from a mutual-TLS handshake.
///
/// The chain is ordered leaf-first: [`PeerCertificates::leaf`] returns the
/// client's end-entity certificate, followed by any intermediates. It is only
/// present when the connection completed an mTLS handshake and the client
/// presented a certificate that validated against the configured CA bundle.
#[derive(Clone, Debug)]
pub struct PeerCertificates(Arc<Vec<CertificateDer<'static>>>);

impl PeerCertificates {
    /// The full verified certificate chain, leaf first.
    #[must_use]
    pub fn as_slice(&self) -> &[CertificateDer<'static>] {
        &self.0
    }

    /// The client's end-entity (leaf) certificate.
    ///
    /// The chain is guaranteed non-empty: a `PeerCertificates` is only
    /// constructed from a non-empty rustls peer chain.
    #[must_use]
    pub fn leaf(&self) -> &CertificateDer<'static> {
        &self.0[0]
    }
}

/// Connection information for a TLS listener, carrying the peer's socket
/// address and any verified client certificate chain.
///
/// Extract it from a handler with
/// `axum::extract::ConnectInfo<acton_service::tls::TlsConnectInfo>`. The
/// certificate chain is present only for connections that completed a
/// mutual-TLS handshake with a valid client certificate.
///
/// This type supersedes the plain `SocketAddr` connect-info on TLS listeners,
/// so TLS connections also expose a real remote address for rate limiting.
#[derive(Clone, Debug)]
pub struct TlsConnectInfo {
    remote_addr: SocketAddr,
    peer_certificates: Option<PeerCertificates>,
}

impl TlsConnectInfo {
    /// The remote socket address of the connecting peer.
    #[must_use]
    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }

    /// The verified client certificate chain, if the peer authenticated with
    /// one during a mutual-TLS handshake.
    #[must_use]
    pub fn peer_certificates(&self) -> Option<&PeerCertificates> {
        self.peer_certificates.as_ref()
    }

    /// Whether the peer presented a verified client certificate.
    #[must_use]
    pub fn is_mutually_authenticated(&self) -> bool {
        self.peer_certificates.is_some()
    }
}

impl axum::extract::connect_info::Connected<axum::serve::IncomingStream<'_, TlsListener>>
    for TlsConnectInfo
{
    fn connect_info(stream: axum::serve::IncomingStream<'_, TlsListener>) -> Self {
        let remote_addr = *stream.remote_addr();
        let (_, connection) = stream.io().get_ref();
        let peer_certificates = connection
            .peer_certificates()
            .filter(|chain| !chain.is_empty())
            .map(|chain| PeerCertificates(Arc::new(chain.to_vec())));

        Self {
            remote_addr,
            peer_certificates,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    /// A self-signed certificate plus its PEM-encoded private key, usable both
    /// as a CA trust anchor and as a leaf server identity in tests.
    struct TestCert {
        cert_pem: String,
        key_pem: String,
        der: CertificateDer<'static>,
    }

    fn generate_cert(name: &str) -> TestCert {
        let certified = rcgen::generate_simple_self_signed(vec![name.to_string()])
            .expect("self-signed cert generation");
        TestCert {
            cert_pem: certified.cert.pem(),
            key_pem: certified.signing_key.serialize_pem(),
            der: certified.cert.der().clone(),
        }
    }

    fn write_temp(contents: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        file.write_all(contents.as_bytes()).expect("write temp");
        file.flush().expect("flush temp");
        file
    }

    #[test]
    fn load_client_ca_roots_reads_a_valid_bundle() {
        let ca = generate_cert("test-ca");
        let file = write_temp(&ca.cert_pem);

        let roots = load_client_ca_roots(file.path()).expect("valid CA bundle must load");

        assert_eq!(
            roots.len(),
            1,
            "the single CA certificate must become one trust anchor"
        );
    }

    #[test]
    fn load_client_ca_roots_rejects_a_missing_file() {
        let err = load_client_ca_roots(Path::new("/nonexistent/ca.pem"))
            .expect_err("a missing CA file must be an error, not an empty trust store");

        assert!(
            err.to_string().contains("Failed to open client CA file"),
            "error must name the failure to open the file: {err}"
        );
    }

    #[test]
    fn load_client_ca_roots_rejects_a_file_without_certificates() {
        // A syntactically valid PEM file that carries no certificates.
        let file = write_temp("# no certificates here\n");

        let err = load_client_ca_roots(file.path())
            .expect_err("a CA file with no certificates must be an error");

        assert!(
            err.to_string().contains("contains no certificates"),
            "error must explain that the bundle is empty: {err}"
        );
    }

    #[test]
    fn build_client_verifier_supports_required_and_optional_modes() {
        let ca = generate_cert("test-ca");
        let file = write_temp(&ca.cert_pem);

        let required_roots = load_client_ca_roots(file.path()).expect("roots");
        build_client_verifier(required_roots, false)
            .expect("a verifier requiring client auth must build");

        let optional_roots = load_client_ca_roots(file.path()).expect("roots");
        build_client_verifier(optional_roots, true)
            .expect("a verifier allowing unauthenticated clients must build");
    }

    #[test]
    fn load_server_config_without_client_ca_requests_no_client_auth() {
        let server = generate_cert("localhost");
        let cert_file = write_temp(&server.cert_pem);
        let key_file = write_temp(&server.key_pem);

        let config = TlsConfig {
            enabled: true,
            cert_path: cert_file.path().to_path_buf(),
            key_path: key_file.path().to_path_buf(),
            client_ca_path: None,
            client_auth_optional: false,
        };

        load_server_config(&config).expect("server-only TLS config must build");
    }

    #[test]
    fn load_server_config_with_client_ca_builds_a_mutual_tls_config() {
        let server = generate_cert("localhost");
        let ca = generate_cert("client-ca");
        let cert_file = write_temp(&server.cert_pem);
        let key_file = write_temp(&server.key_pem);
        let ca_file = write_temp(&ca.cert_pem);

        let config = TlsConfig {
            enabled: true,
            cert_path: cert_file.path().to_path_buf(),
            key_path: key_file.path().to_path_buf(),
            client_ca_path: Some(ca_file.path().to_path_buf()),
            client_auth_optional: true,
        };

        load_server_config(&config).expect("mutual-TLS config must build");
    }

    #[test]
    fn load_server_config_surfaces_an_invalid_client_ca() {
        let server = generate_cert("localhost");
        let cert_file = write_temp(&server.cert_pem);
        let key_file = write_temp(&server.key_pem);

        let config = TlsConfig {
            enabled: true,
            cert_path: cert_file.path().to_path_buf(),
            key_path: key_file.path().to_path_buf(),
            client_ca_path: Some(PathBuf::from("/nonexistent/client-ca.pem")),
            client_auth_optional: false,
        };

        load_server_config(&config)
            .expect_err("an unreadable client CA must fail the whole config build");
    }

    /// Write cert and key PEM into named files under a directory the test owns,
    /// so their contents can be rewritten in place to simulate a rotation.
    fn write_credentials(dir: &Path, cert: &TestCert) -> TlsConfig {
        let cert_path = dir.join("server.pem");
        let key_path = dir.join("server.key");
        std::fs::write(&cert_path, &cert.cert_pem).expect("write cert");
        std::fs::write(&key_path, &cert.key_pem).expect("write key");

        TlsConfig {
            enabled: true,
            cert_path,
            key_path,
            client_ca_path: None,
            client_auth_optional: false,
        }
    }

    #[test]
    fn static_source_serves_the_config_it_was_built_from() {
        let server = generate_cert("localhost");
        let cert_file = write_temp(&server.cert_pem);
        let key_file = write_temp(&server.key_pem);
        let config = TlsConfig {
            enabled: true,
            cert_path: cert_file.path().to_path_buf(),
            key_path: key_file.path().to_path_buf(),
            client_ca_path: None,
            client_auth_optional: false,
        };
        let server_config = load_server_config(&config).expect("config builds");

        let source = TlsConfigSource::from_server_config(server_config.clone());

        assert!(
            Arc::ptr_eq(&source.load(), &server_config),
            "a static source must hand back exactly the config it was given"
        );
        assert!(
            !source.is_reloadable(),
            "a source with no file origin is not reloadable"
        );
        assert!(source.origin().is_none());
    }

    #[test]
    fn reload_on_a_static_source_is_an_error_and_keeps_the_config() {
        let server = generate_cert("localhost");
        let cert_file = write_temp(&server.cert_pem);
        let key_file = write_temp(&server.key_pem);
        let config = TlsConfig {
            enabled: true,
            cert_path: cert_file.path().to_path_buf(),
            key_path: key_file.path().to_path_buf(),
            client_ca_path: None,
            client_auth_optional: false,
        };
        let server_config = load_server_config(&config).expect("config builds");
        let source = TlsConfigSource::from_server_config(server_config.clone());

        let err = source
            .reload()
            .expect_err("a static source has no files to reread");

        assert!(
            err.to_string().contains("cannot be reloaded"),
            "the error must say the source is not reloadable: {err}"
        );
        assert!(
            Arc::ptr_eq(&source.load(), &server_config),
            "a rejected reload must leave the served config untouched"
        );
    }

    #[test]
    fn successful_reload_swaps_in_the_new_credentials() {
        let dir = tempfile::tempdir().expect("temp dir");
        let first = generate_cert("localhost");
        let tls_config = write_credentials(dir.path(), &first);

        let source = TlsConfigSource::from_tls_config(&tls_config).expect("initial load");
        assert!(source.is_reloadable(), "a file-backed source is reloadable");
        let before = source.load();

        // Rotate: replace the files in place with a different key pair.
        let second = generate_cert("localhost");
        std::fs::write(&tls_config.cert_path, &second.cert_pem).expect("rewrite cert");
        std::fs::write(&tls_config.key_path, &second.key_pem).expect("rewrite key");

        source
            .reload()
            .expect("rereading valid credentials must succeed");

        assert!(
            !Arc::ptr_eq(&source.load(), &before),
            "a successful reload must install a newly loaded config"
        );
    }

    #[test]
    fn failed_reload_preserves_the_last_good_credentials() {
        let dir = tempfile::tempdir().expect("temp dir");
        let good = generate_cert("localhost");
        let tls_config = write_credentials(dir.path(), &good);

        let source = TlsConfigSource::from_tls_config(&tls_config).expect("initial load");
        let last_good = source.load();

        // Simulate a half-written rotation: the cert file is no longer parseable.
        std::fs::write(
            &tls_config.cert_path,
            "-----BEGIN CERTIFICATE-----\ntruncated",
        )
        .expect("corrupt cert");

        let err = source
            .reload()
            .expect_err("an unparseable certificate must fail the reload");

        assert!(
            Arc::ptr_eq(&source.load(), &last_good),
            "a failed reload must keep serving the last-good config, not drop it"
        );
        assert!(
            !err.to_string().is_empty(),
            "the failure must be reported to the caller: {err}"
        );

        // And the source recovers once the files are valid again.
        let replacement = generate_cert("localhost");
        std::fs::write(&tls_config.cert_path, &replacement.cert_pem).expect("rewrite cert");
        std::fs::write(&tls_config.key_path, &replacement.key_pem).expect("rewrite key");
        source.reload().expect("a later valid reload must succeed");
        assert!(
            !Arc::ptr_eq(&source.load(), &last_good),
            "the recovered reload must install the new config"
        );
    }

    #[test]
    fn clones_of_a_source_observe_the_same_reload() {
        let dir = tempfile::tempdir().expect("temp dir");
        let first = generate_cert("localhost");
        let tls_config = write_credentials(dir.path(), &first);

        let source = TlsConfigSource::from_tls_config(&tls_config).expect("initial load");
        let listener_side = source.clone();
        let before = listener_side.load();

        let second = generate_cert("localhost");
        std::fs::write(&tls_config.cert_path, &second.cert_pem).expect("rewrite cert");
        std::fs::write(&tls_config.key_path, &second.key_pem).expect("rewrite key");
        source.reload().expect("reload");

        assert!(
            Arc::ptr_eq(&listener_side.load(), &source.load()),
            "every clone must see the same installed config"
        );
        assert!(
            !Arc::ptr_eq(&listener_side.load(), &before),
            "the clone held by the listener must see the rotation"
        );
    }

    #[test]
    fn tls_connect_info_reports_absence_of_client_cert() {
        let info = TlsConnectInfo {
            remote_addr: "127.0.0.1:8443".parse().expect("addr"),
            peer_certificates: None,
        };

        assert!(
            !info.is_mutually_authenticated(),
            "a connection without a client cert is not mutually authenticated"
        );
        assert!(info.peer_certificates().is_none());
        assert_eq!(info.remote_addr(), "127.0.0.1:8443".parse().expect("addr"));
    }

    #[test]
    fn tls_connect_info_exposes_the_verified_chain() {
        let leaf = generate_cert("client-leaf");
        let chain = PeerCertificates(Arc::new(vec![leaf.der.clone()]));
        let info = TlsConnectInfo {
            remote_addr: "10.0.0.5:9000".parse().expect("addr"),
            peer_certificates: Some(chain),
        };

        assert!(info.is_mutually_authenticated());
        let certs = info.peer_certificates().expect("chain present");
        assert_eq!(certs.as_slice().len(), 1);
        assert_eq!(certs.leaf(), &leaf.der, "the leaf must be the first cert");
    }
}
