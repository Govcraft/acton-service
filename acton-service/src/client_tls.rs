//! Client-side mutual-TLS identity.
//!
//! The outbound mirror of [`crate::tls`]. Where that module describes the
//! certificate this service *presents* as a server, this one describes the
//! certificate it presents when it *calls* another mutual-TLS service, and the
//! trust anchors it uses to verify that peer.
//!
//! Everything here is driven by a [`ClientIdentityConfig`] and follows the same
//! fail-closed contract as the server side: a certificate, key or CA bundle
//! that cannot be read, cannot be parsed, is empty, or does not form a matching
//! pair is an error at load time. No loader here ever falls back to an
//! unauthenticated or unverified client, because a client that silently drops
//! its identity would keep working right up until the peer starts enforcing.
//!
//! # Which entry point to use
//!
//! | You are building | Use |
//! |---|---|
//! | An HTTP client whose certificate never changes | [`reqwest_client_builder`] |
//! | An HTTP client that must rotate its certificate | [`ClientIdentitySource::client`] |
//! | A gRPC channel that must rotate its certificate | [`ClientIdentitySource::grpc_channel`] |
//! | A gRPC channel whose certificate never changes | [`tonic_client_tls_config`] |
//! | Something else that speaks raw rustls | [`load_rustls_client_config`] |
//!
//! [`ClientIdentitySource`] is the right default for any long-lived service:
//! credentials that are rotated on disk take effect on the next handshake
//! without rebuilding, re-pooling or restarting anything. The standalone
//! loaders remain for one-shot clients and for callers assembling a transport
//! this module does not cover.

use std::path::Path;
use std::sync::Arc;

use arc_swap::ArcSwap;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use tokio_rustls::rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use tokio_rustls::rustls::client::{ResolvesClientCert, WebPkiServerVerifier};
use tokio_rustls::rustls::sign::CertifiedKey;
use tokio_rustls::rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use zeroize::{Zeroize, Zeroizing};

use crate::config::ClientIdentityConfig;
use crate::error::{Error, Result};

/// The ALPN protocols an HTTP client offers.
///
/// `reqwest::ClientBuilder::use_preconfigured_tls` hands the supplied rustls
/// configuration to the connector verbatim and injects nothing of its own, so
/// leaving this unset would silently downgrade every request to HTTP/1.1. These
/// are the protocols, in this order, that `reqwest` negotiates on its native
/// path; setting them here keeps the preconfigured path equivalent.
const HTTP_ALPN: [&[u8]; 2] = [b"h2", b"http/1.1"];

/// The ALPN protocols a gRPC channel offers.
///
/// gRPC is defined over HTTP/2 only, so a channel that negotiated `http/1.1`
/// would connect and then fail every RPC.
#[cfg(feature = "grpc")]
const GRPC_ALPN: [&[u8]; 1] = [b"h2"];

/// The role string used in trust-store error messages for the peer's CA bundle.
///
/// Mirrors the `"client CA"` role the server side passes to the same loader, so
/// a failure names which side of the handshake the bundle belongs to.
const PEER_CA_ROLE: &str = "peer CA";

/// A validated client certificate chain and private key, in both the parsed and
/// the raw PEM form.
///
/// Deliberately crate-private. It carries the private key in memory in two
/// representations, so handing it to callers would multiply the number of
/// places key bytes can be copied or logged. Callers get the specific artifact
/// they need instead: a [`ClientConfig`], a [`reqwest::Identity`], or a
/// [`reqwest::ClientBuilder`].
pub(crate) struct IdentityMaterial {
    /// The PEM-encoded certificate chain exactly as it was read from disk.
    cert_pem: Vec<u8>,
    /// The PEM-encoded private key, wiped from memory when this value drops.
    key_pem: Zeroizing<Vec<u8>>,
    /// The parsed certificate chain, leaf first.
    chain: Vec<CertificateDer<'static>>,
    /// The parsed private key.
    key: PrivateKeyDer<'static>,
}

impl Drop for IdentityMaterial {
    fn drop(&mut self) {
        // The PEM copies are already `Zeroizing`, but the *decoded* DER private
        // key is not: `rustls_pki_types::PrivateKeyDer` implements `Zeroize` yet
        // not `ZeroizeOnDrop`, so without this its key bytes would be freed onto
        // a heap page without being wiped. Wipe it explicitly. `cert_pem` and
        // `chain` are public certificate material and need no wiping.
        self.key.zeroize();
    }
}

impl std::fmt::Debug for IdentityMaterial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Every field here is either key material or a certificate, so report
        // the shape of the value and nothing else. Deriving `Debug` would print
        // the private key bytes into whatever asserted on this value.
        f.debug_struct("IdentityMaterial")
            .field("chain_len", &self.chain.len())
            .field("cert_pem_len", &self.cert_pem.len())
            .field("key_pem_len", &self.key_pem.len())
            .finish_non_exhaustive()
    }
}

/// Join a certificate chain and a private key into the single PEM buffer that
/// [`reqwest::Identity::from_pem`] requires.
///
/// `reqwest` takes one buffer holding both the key and at least one
/// certificate, whereas the two live in separate files. This concatenates them
/// in that order, inserting a single separating newline only when `cert_pem`
/// does not already end with one: a PEM file written without a trailing newline
/// would otherwise splice its `-----END CERTIFICATE-----` line onto the key's
/// `-----BEGIN` line and produce a buffer that parses as neither.
///
/// The result is [`Zeroizing`], so the concatenated copy of the private key is
/// wiped when it drops. This function is the one place in the crate that
/// duplicates key bytes into a new allocation, which is exactly why it is worth
/// wiping: the originals on the server side are consumed by rustls, but this
/// copy would otherwise linger in a freed heap page.
#[must_use]
pub fn concat_identity_pem(cert_pem: &[u8], key_pem: &[u8]) -> Zeroizing<Vec<u8>> {
    let needs_separator = !cert_pem.ends_with(b"\n");
    let mut joined = Zeroizing::new(Vec::with_capacity(
        cert_pem.len() + usize::from(needs_separator) + key_pem.len(),
    ));
    joined.extend_from_slice(cert_pem);
    if needs_separator {
        joined.push(b'\n');
    }
    joined.extend_from_slice(key_pem);
    joined
}

/// Read and validate the client certificate chain and private key named by
/// `config`.
///
/// Fails when either file is unreadable, when either fails to parse, when the
/// chain contains no certificates, or when the key does not match the leaf
/// certificate. The pair check is done here rather than left to each transport
/// because two of the three transports would otherwise defer it: `tonic`'s
/// `Identity::from_pem` is infallible, and a mismatched pair would surface as a
/// handshake failure against a live peer instead of a startup error.
pub(crate) fn load_identity_material(config: &ClientIdentityConfig) -> Result<IdentityMaterial> {
    use rustls_pki_types::pem::PemObject;

    let cert_pem = std::fs::read(&config.cert_path).map_err(|e| {
        Error::Internal(format!(
            "Failed to open client identity cert file '{}': {}",
            config.cert_path.display(),
            e
        ))
    })?;

    let key_pem = Zeroizing::new(std::fs::read(&config.key_path).map_err(|e| {
        Error::Internal(format!(
            "Failed to open client identity key file '{}': {}",
            config.key_path.display(),
            e
        ))
    })?);

    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(&cert_pem)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            Error::Internal(format!(
                "Failed to parse client identity certificates from '{}': {}",
                config.cert_path.display(),
                e
            ))
        })?;

    if chain.is_empty() {
        return Err(Error::Internal(format!(
            "Client identity cert file '{}' contains no certificates",
            config.cert_path.display()
        )));
    }

    let key = PrivateKeyDer::from_pem_slice(&key_pem).map_err(|e| {
        Error::Internal(format!(
            "Failed to parse client identity private key from '{}': {}",
            config.key_path.display(),
            e
        ))
    })?;

    let material = IdentityMaterial {
        cert_pem,
        key_pem,
        chain,
        key,
    };
    // Discarded here: this call exists for its validation, and every caller of
    // `load_identity_material` that needs the key in rustls form builds it from
    // the returned material through the same function.
    certified_key_from_material(&material, config)?;
    Ok(material)
}

/// Turn validated identity material into the rustls [`CertifiedKey`] a
/// handshake presents, confirming on the way that the private key is the one
/// belonging to the leaf certificate.
///
/// This is both the crate's only key-pair consistency check and the only place
/// a [`CertifiedKey`] is built, so an initial load and a rotation validate
/// exactly the same things. [`load_identity_material`] calls it for the check
/// and drops the key; [`RotatingClientCertResolver`] calls it for the key.
fn certified_key_from_material(
    material: &IdentityMaterial,
    config: &ClientIdentityConfig,
) -> Result<CertifiedKey> {
    use tokio_rustls::rustls::crypto::CryptoProvider;

    // The provider supplies the key loader, so it must be installed first, for
    // the same reason `ClientConfig::builder()` needs it.
    crate::crypto::ensure_default_crypto_provider();

    let provider = CryptoProvider::get_default().ok_or_else(|| {
        Error::Internal(
            "No rustls crypto provider is installed; enable exactly one of the \
             `crypto-aws-lc-rs` or `crypto-ring` features"
                .to_string(),
        )
    })?;

    let signing_key = provider
        .key_provider
        .load_private_key(material.key.clone_key())
        .map_err(|e| {
            Error::Internal(format!(
                "Client identity private key from '{}' is not usable by the \
                 configured crypto provider: {}",
                config.key_path.display(),
                e
            ))
        })?;

    let certified = CertifiedKey::new(material.chain.clone(), signing_key);
    certified.keys_match().map_err(|e| {
        Error::Internal(format!(
            "Client identity private key '{}' does not match the certificate \
             in '{}': {}",
            config.key_path.display(),
            config.cert_path.display(),
            e
        ))
    })?;
    Ok(certified)
}

/// Build the trust anchors used to verify the peer's server certificate.
///
/// With no `root_ca_path` the store is the built-in web PKI roots. With one,
/// those roots are joined by the bundle's certificates unless
/// [`ClientIdentityConfig::exclusive_roots`] is set, in which case the bundle
/// replaces them entirely. The bundle is loaded through the same routine the
/// server uses for its client-CA bundle, so an unreadable or empty file is an
/// error rather than a silently narrower trust store.
fn build_root_store(config: &ClientIdentityConfig) -> Result<RootCertStore> {
    let mut roots = RootCertStore::empty();

    let Some(ref path) = config.root_ca_path else {
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        return Ok(roots);
    };

    let peer_roots = crate::tls::load_root_store(path, PEER_CA_ROLE)?;
    if !config.exclusive_roots {
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    }
    roots.roots.extend(peer_roots.roots);
    Ok(roots)
}

/// Read the peer's CA bundle from disk after validating that it parses.
///
/// The validation pass is discarded; its purpose is to turn an unreadable,
/// unparseable or empty bundle into the same error the rustls path produces,
/// before the raw bytes are handed to a transport whose own parser would either
/// accept an empty bundle or defer the failure to connect time.
fn read_validated_ca_bundle(path: &Path) -> Result<Vec<u8>> {
    crate::tls::load_root_store(path, PEER_CA_ROLE)?;
    std::fs::read(path).map_err(|e| {
        Error::Internal(format!(
            "Failed to read {} file '{}': {}",
            PEER_CA_ROLE,
            path.display(),
            e
        ))
    })
}

/// A [`ResolvesClientCert`] that reads its answer from an [`ArcSwap`], so the
/// identity a handshake presents can be replaced while connections are live.
///
/// rustls calls [`resolve`](ResolvesClientCert::resolve) once per handshake
/// rather than reading a certificate fixed at `ClientConfig` build time. Storing
/// the [`CertifiedKey`] behind an `ArcSwap` therefore turns certificate rotation
/// into a pointer store: handshakes already in flight finish with the key they
/// picked up, every handshake after the store uses the new one, and nothing
/// above the TLS layer — client, connection pool, channel — needs rebuilding.
///
/// This is the client-side mirror of the server's `ArcSwap<ServerConfig>` in
/// [`crate::tls::TlsConfigSource`].
#[derive(Debug)]
struct RotatingClientCertResolver {
    /// The key presented by the next handshake to ask for one.
    current: ArcSwap<CertifiedKey>,
}

impl RotatingClientCertResolver {
    /// Build a resolver serving `initial` until the first successful swap.
    fn new(initial: CertifiedKey) -> Self {
        Self {
            current: ArcSwap::new(Arc::new(initial)),
        }
    }

    /// Install `key` as the identity every subsequent handshake presents.
    fn store(&self, key: CertifiedKey) {
        self.current.store(Arc::new(key));
    }

    /// The key the next handshake would present.
    ///
    /// Test-only: nothing in the crate needs to read the identity back, and
    /// exposing it more widely would put certificate bytes in reach of callers
    /// that only ever need to *use* the identity, never inspect it.
    #[cfg(test)]
    fn current(&self) -> Arc<CertifiedKey> {
        self.current.load_full()
    }
}

impl ResolvesClientCert for RotatingClientCertResolver {
    fn resolve(
        &self,
        _root_hint_subjects: &[&[u8]],
        _sigschemes: &[SignatureScheme],
    ) -> Option<Arc<CertifiedKey>> {
        // Both hints are ignored deliberately. This source is configured with
        // exactly one identity, so there is nothing to choose between: the
        // server either accepts it or rejects the handshake. Filtering on the
        // hints could only ever turn a rejection the peer would report into a
        // silent decision to present no certificate at all, which is the
        // failure mode this module exists to prevent.
        Some(self.current.load_full())
    }

    fn has_certs(&self) -> bool {
        // Always true: the source cannot be constructed without a valid
        // identity, and a failed reload keeps the last-good one.
        true
    }
}

/// A [`ServerCertVerifier`] that forwards every decision to a stock
/// [`WebPkiServerVerifier`] held behind an [`ArcSwap`], so the trust anchors
/// used to verify peers can be replaced while connections are live.
///
/// # This is not a verification bypass
///
/// Installing a custom verifier requires the `dangerous()` builder API, whose
/// usual purpose is to *weaken* verification. This type does the opposite of
/// weakening it: it performs no verification of its own and makes no policy
/// decision. Every trait method delegates, unchanged, to a `WebPkiServerVerifier`
/// built by [`build_root_store`] from exactly the same `root_ca_path` and
/// `exclusive_roots` settings the non-rotating path uses. The only thing the
/// indirection buys is the ability to swap that inner verifier when the peer's
/// CA bundle is rotated on disk. A caller gets standard webpki verification,
/// against anchors that can change.
#[derive(Debug)]
struct RotatingWebPkiVerifier {
    /// The verifier every trait method delegates to.
    current: ArcSwap<WebPkiServerVerifier>,
}

impl RotatingWebPkiVerifier {
    /// Build a verifier delegating to `initial` until the first swap.
    fn new(initial: Arc<WebPkiServerVerifier>) -> Self {
        Self {
            current: ArcSwap::new(initial),
        }
    }

    /// Delegate to `verifier` from the next verification onwards.
    fn store(&self, verifier: Arc<WebPkiServerVerifier>) {
        self.current.store(verifier);
    }
}

impl ServerCertVerifier for RotatingWebPkiVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, tokio_rustls::rustls::Error> {
        self.current.load().verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        )
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, tokio_rustls::rustls::Error> {
        self.current
            .load()
            .verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, tokio_rustls::rustls::Error> {
        self.current
            .load()
            .verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.current.load().supported_verify_schemes()
    }

    // `root_hint_subjects` is deliberately left at its default of `None` rather
    // than forwarded. It returns a borrow tied to `&self`, which cannot be
    // produced from a value read out of an `ArcSwap` without leaking the guard.
    // The cost is limited and not a correctness matter: the hint populates the
    // TLS 1.3 `certificate_authorities` extension, which only helps a peer
    // choose among several server certificates. Verification itself is
    // unaffected, because it runs through the delegated methods above.
}

/// Build the [`WebPkiServerVerifier`] for the peer roots named by `config`.
///
/// The single place both the initial load and a reload construct a verifier, so
/// `root_ca_path` and `exclusive_roots` cannot be honoured differently by the
/// two paths.
fn build_peer_verifier(config: &ClientIdentityConfig) -> Result<Arc<WebPkiServerVerifier>> {
    crate::crypto::ensure_default_crypto_provider();

    let roots = build_root_store(config)?;
    WebPkiServerVerifier::builder(Arc::new(roots))
        .build()
        .map_err(|e| {
            Error::Internal(format!(
                "Failed to build a peer certificate verifier from the configured \
                 {} roots: {}",
                PEER_CA_ROLE, e
            ))
        })
}

/// Load a rustls [`ClientConfig`] that presents this service's client
/// certificate and verifies the peer against the configured roots.
///
/// # ALPN
///
/// The returned configuration sets **no ALPN protocols**. That matters when it
/// is handed to `reqwest::ClientBuilder::use_preconfigured_tls`, which uses the
/// supplied configuration verbatim rather than layering its own negotiation on
/// top: without `alpn_protocols` the connection cannot negotiate `h2`, and
/// every request silently downgrades to HTTP/1.1. A caller taking that route
/// must set the field itself, for example:
///
/// ```rust,ignore
/// let mut tls = (*load_rustls_client_config(&config)?).clone();
/// tls.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
/// let client = reqwest::Client::builder().use_preconfigured_tls(tls).build()?;
/// ```
///
/// This is why [`reqwest_client_builder`] is the simpler path for a one-shot
/// HTTP client: it leaves ALPN and HTTP-version negotiation to `reqwest`.
///
/// # This configuration does not rotate
///
/// The identity and trust anchors here are fixed when this function returns.
/// For a long-lived service whose credentials are rotated on disk, use
/// [`ClientIdentitySource`], which builds an equivalent configuration around a
/// per-handshake [`ResolvesClientCert`] — including setting ALPN correctly — and
/// swaps both in place on reload.
///
/// # Errors
///
/// Returns an error when the certificate, key or CA bundle cannot be read or
/// parsed, when the chain is empty, when the key does not match the leaf
/// certificate, or when rustls rejects the resulting pair.
pub fn load_rustls_client_config(config: &ClientIdentityConfig) -> Result<Arc<ClientConfig>> {
    let material = load_identity_material(config)?;
    let roots = build_root_store(config)?;

    // Installed already by the pair check above; called again because this
    // function's contract is to be usable on its own.
    crate::crypto::ensure_default_crypto_provider();

    let client_config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_client_auth_cert(material.chain.clone(), material.key.clone_key())
        .map_err(|e| Error::Internal(format!("Failed to build rustls client config: {}", e)))?;

    Ok(Arc::new(client_config))
}

/// Load this service's client certificate as a [`reqwest::Identity`].
///
/// Useful when the caller already has a configured [`reqwest::ClientBuilder`]
/// and only needs the identity to attach. Prefer [`reqwest_client_builder`]
/// when starting from scratch, because it also wires up the peer's trust
/// anchors, which an identity alone does not.
///
/// # Errors
///
/// Returns an error when the certificate or key cannot be read or parsed, when
/// the chain is empty, when the key does not match the leaf certificate, or
/// when `reqwest` rejects the combined PEM buffer.
pub fn load_reqwest_identity(config: &ClientIdentityConfig) -> Result<reqwest::Identity> {
    let material = load_identity_material(config)?;
    let joined = concat_identity_pem(&material.cert_pem, &material.key_pem);

    reqwest::Identity::from_pem(&joined).map_err(|e| {
        Error::Internal(format!(
            "Failed to build a reqwest identity from '{}' and '{}': {}",
            config.cert_path.display(),
            config.key_path.display(),
            e
        ))
    })
}

/// Build a [`reqwest::ClientBuilder`] that authenticates with this service's
/// client certificate and trusts the configured peer roots.
///
/// **Use this when the certificate will not change** — a CLI, a test, a job that
/// finishes well inside the certificate's lifetime. A client built from it is
/// fixed at `build()`: rotating means building another one.
///
/// For a long-lived service, prefer [`ClientIdentitySource`], whose client
/// rotates in place and can safely be cached.
///
/// Compared with handing a [`load_rustls_client_config`] result to
/// `use_preconfigured_tls`, this route leaves ALPN and HTTP-version negotiation
/// with `reqwest`, which owns them; on the preconfigured route ALPN becomes the
/// caller's responsibility, and getting it wrong downgrades every request to
/// HTTP/1.1 without any error.
///
/// The builder is returned unbuilt so callers can still set timeouts, default
/// headers, redirect policy and so on before calling `build()`.
///
/// # Errors
///
/// Returns an error when the certificate, key or CA bundle cannot be read or
/// parsed, when the chain is empty, or when the key does not match the leaf
/// certificate.
pub fn reqwest_client_builder(config: &ClientIdentityConfig) -> Result<reqwest::ClientBuilder> {
    crate::crypto::ensure_default_crypto_provider();

    let identity = load_reqwest_identity(config)?;
    let mut builder = reqwest::Client::builder().identity(identity);

    if let Some(ref path) = config.root_ca_path {
        let pem = read_validated_ca_bundle(path)?;
        let certs = reqwest::Certificate::from_pem_bundle(&pem).map_err(|e| {
            Error::Internal(format!(
                "Failed to build reqwest certificates from {} file '{}': {}",
                PEER_CA_ROLE,
                path.display(),
                e
            ))
        })?;
        for cert in certs {
            builder = builder.add_root_certificate(cert);
        }
        if config.exclusive_roots {
            builder = builder.tls_built_in_root_certs(false);
        }
    }

    Ok(builder)
}

/// Build a [`tonic::transport::ClientTlsConfig`] presenting this service's
/// client certificate.
///
/// # This configuration does not rotate
///
/// A [`tonic::transport::Channel`] fixes its TLS configuration when it
/// connects, so a channel built from this value keeps the certificate it
/// started with. For a service whose gRPC identity must rotate, use
/// [`ClientIdentitySource::grpc_channel`], which connects through a connector
/// carrying a per-handshake resolver and rotates without rebuilding the channel
/// or the generated client holding it.
///
/// # Why this validates eagerly
///
/// `tonic::transport::Identity::from_pem` is **infallible**: it stores the bytes
/// and defers every parse and consistency error to the moment a channel
/// connects. A service with a typo in its key file would therefore start
/// cleanly, pass its health checks, and fail on its first real gRPC call. This
/// function parses and validates the certificate and key itself first, so the
/// same defects fail at configuration time, then hands `tonic` the raw bytes it
/// wants.
///
/// # Errors
///
/// Returns an error when the certificate, key or CA bundle cannot be read or
/// parsed, when the chain is empty, or when the key does not match the leaf
/// certificate.
#[cfg(feature = "grpc")]
pub fn tonic_client_tls_config(
    config: &ClientIdentityConfig,
) -> Result<tonic::transport::ClientTlsConfig> {
    // `tonic` builds its rustls configuration lazily at connect time, which is
    // after any point where we could install the provider for it.
    crate::crypto::ensure_default_crypto_provider();

    let material = load_identity_material(config)?;
    let identity = tonic::transport::Identity::from_pem(&material.cert_pem, &material.key_pem);
    let mut tls = tonic::transport::ClientTlsConfig::new().identity(identity);

    let Some(ref path) = config.root_ca_path else {
        return Ok(tls.trust_anchors(webpki_roots::TLS_SERVER_ROOTS.iter().cloned()));
    };

    let pem = read_validated_ca_bundle(path)?;
    tls = tls.ca_certificate(tonic::transport::Certificate::from_pem(pem));
    if !config.exclusive_roots {
        tls = tls.trust_anchors(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    }
    Ok(tls)
}

/// A rotatable client identity: the certificate this service presents to its
/// peers, and the anchors it verifies them against, both replaceable from disk
/// while the service runs.
///
/// # Rotation happens in place
///
/// This behaves like its server-side counterpart
/// [`crate::tls::TlsConfigSource`]: credentials are swapped *inside* live
/// objects rather than by replacing them. One [`reqwest::Client`] is built at
/// construction and never rebuilt. Its rustls configuration carries a
/// [`ResolvesClientCert`] and a delegating [`ServerCertVerifier`], both reading
/// from an [`ArcSwap`], and rustls consults them **once per handshake**. A
/// [`reload`](Self::reload) is therefore two pointer stores:
///
/// - Handshakes already completed keep running on their existing connections.
/// - Handshakes in flight finish with whichever credentials they picked up.
/// - Every handshake after the store uses the new credentials.
///
/// ## Caching the client handle is fine
///
/// ```rust,ignore
/// // Both of these rotate. Take the handle once and keep it.
/// let client = source.client();
/// loop { client.get(url).send().await?; }
/// ```
///
/// The handle is stable across reloads: it is the same client, and it is the
/// client's TLS layer that rotates underneath it. Storing it in application
/// state, handing it to a background task, or cloning it into a struct are all
/// correct.
///
/// ## The connection pool survives a reload
///
/// Nothing is discarded, so there is no reconnect storm and no thundering herd
/// to schedule around. Existing pooled connections keep serving requests with
/// the credentials they were established under, which is exactly right: a TLS
/// connection is authenticated at handshake time, and re-authenticating a live
/// one is neither possible nor necessary. New connections pick up the new
/// credentials.
///
/// Because a reload is now cheap, driving it from a timer is harmless rather
/// than costly. Signal-driven reloads — `SIGHUP`, an inotify watch on the
/// certificate directory, a cert-manager hook — remain the better design, since
/// they rotate when something actually changed instead of re-reading identical
/// bytes on a schedule.
///
/// ## gRPC channels rotate too
///
/// [`grpc_channel`](Self::grpc_channel) builds a [`tonic::transport::Channel`]
/// over a connector that shares this source's rustls configuration, so a
/// channel held by a generated client rotates on reload like everything else.
/// [`tonic_client_tls_config_snapshot`](Self::tonic_client_tls_config_snapshot)
/// is retained for callers who need `tonic`'s own TLS plumbing, and remains a
/// point-in-time copy that does not rotate.
///
/// # Failure behaviour
///
/// Every source is reloadable — it is always built from a
/// [`ClientIdentityConfig`], so there is no static variant and no
/// `is_reloadable()` to check. A failed [`reload`](Self::reload) is
/// **fail-closed and all-or-nothing**: the last-good certificate *and* the
/// last-good trust anchors both stay installed, the error is logged at `ERROR`
/// level and returned. A rotation that produced an unusable certificate must
/// not leave the service unable to call its peers, and a half-applied rotation
/// — new certificate against stale anchors, or the reverse — must never be
/// observable.
#[derive(Clone)]
pub struct ClientIdentitySource {
    inner: Arc<ClientIdentitySourceInner>,
}

struct ClientIdentitySourceInner {
    /// The one client handed out for this source's whole lifetime. Stable
    /// across reloads; its TLS layer rotates underneath it.
    client: Arc<reqwest::Client>,
    /// The identity presented by the next handshake.
    resolver: Arc<RotatingClientCertResolver>,
    /// The anchors the next peer verification runs against.
    verifier: Arc<RotatingWebPkiVerifier>,
    /// The rustls configuration shared by every transport this source builds,
    /// with no ALPN protocols set. Each transport clones it and sets the ALPN
    /// list appropriate to its protocol; the rotating resolver and verifier are
    /// behind `Arc`s, so every clone rotates together.
    tls: ClientConfig,
    /// The files a reload rereads. Never absent: every source is reloadable.
    origin: ClientIdentityConfig,
}

impl ClientIdentitySource {
    /// Build a source by loading the identity named by `config`.
    ///
    /// Reads and validates the certificate, key and any peer-CA bundle, then
    /// builds the rotating rustls configuration and the single
    /// [`reqwest::Client`] that will serve this source for its whole lifetime.
    /// Returns the load error if any of that fails, leaving no source rather
    /// than one that would hand out an unauthenticated client.
    ///
    /// # Errors
    ///
    /// Returns an error when the credentials cannot be loaded or validated,
    /// when the peer roots cannot be assembled into a verifier, or when
    /// `reqwest` cannot build a client from the resulting configuration.
    pub fn from_config(config: &ClientIdentityConfig) -> Result<Self> {
        crate::crypto::ensure_default_crypto_provider();

        let material = load_identity_material(config)?;
        let resolver = Arc::new(RotatingClientCertResolver::new(
            certified_key_from_material(&material, config)?,
        ));
        let verifier = Arc::new(RotatingWebPkiVerifier::new(build_peer_verifier(config)?));

        // `dangerous()` is required to install a custom verifier. See
        // `RotatingWebPkiVerifier`: it delegates to stock webpki verification
        // and weakens nothing. The rotation ability is the entire reason it
        // exists.
        let tls = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::clone(&verifier) as Arc<dyn ServerCertVerifier>)
            .with_client_cert_resolver(Arc::clone(&resolver) as Arc<dyn ResolvesClientCert>);

        let client = build_client(&tls, config)?;

        Ok(Self {
            inner: Arc::new(ClientIdentitySourceInner {
                client: Arc::new(client),
                resolver,
                verifier,
                tls,
                origin: config.clone(),
            }),
        })
    }

    /// The HTTP client for this identity.
    ///
    /// A refcount bump and no I/O. The same client for the lifetime of the
    /// source: caching the returned handle is correct and preferred, because
    /// its TLS layer rotates in place on every [`reload`](Self::reload).
    #[must_use]
    pub fn client(&self) -> Arc<reqwest::Client> {
        Arc::clone(&self.inner.client)
    }

    /// The configuration this source reloads from.
    #[must_use]
    pub fn origin(&self) -> &ClientIdentityConfig {
        &self.inner.origin
    }

    /// Reread the identity files and install the new credentials in place.
    ///
    /// On success, every handshake from this point on presents the new
    /// certificate and verifies peers against the newly read anchors. No client
    /// is rebuilt, no channel is invalidated and no pooled connection is
    /// dropped.
    ///
    /// Everything is read and validated before anything is installed, so a
    /// reload either applies in full or not at all. A new certificate that
    /// arrives alongside an unreadable CA bundle installs neither.
    ///
    /// # Errors
    ///
    /// Returns an error when the certificate, key or CA bundle fails to read,
    /// parse or validate. The previously installed certificate *and* trust
    /// anchors both stay in place and keep working in every such case. Failures
    /// are also logged at `ERROR` level, because a service whose rotation has
    /// silently stopped working will keep working until the certificate expires
    /// and then fail every outbound call at once.
    pub fn reload(&self) -> Result<()> {
        let origin = &self.inner.origin;
        match self.prepare_reload() {
            Ok((key, verifier)) => {
                // The presented identity and the peer trust anchors live in two
                // separate `ArcSwap`s, stored one after the other, so a handshake
                // that runs between these two stores can observe the new
                // certificate paired with the old anchors (or, after the first
                // store, the reverse). That is deliberately left unsynchronised:
                // each value is individually valid, and which certificate this
                // client presents is orthogonal to which CAs it trusts in the
                // peer, so any pairing of an old-or-new identity with old-or-new
                // anchors is a correct handshake. A lock spanning both stores
                // would buy nothing but contention on the handshake path.
                self.inner.resolver.store(key);
                self.inner.verifier.store(verifier);
                tracing::info!(
                    cert_path = %origin.cert_path.display(),
                    key_path = %origin.key_path.display(),
                    "client identity reloaded in place; new handshakes use the new \
                     credentials and existing pooled connections are untouched"
                );
                Ok(())
            }
            Err(e) => {
                tracing::error!(
                    cert_path = %origin.cert_path.display(),
                    key_path = %origin.key_path.display(),
                    error = %e,
                    "client identity reload failed; continuing to use the previous \
                     certificate and trust anchors. New credentials will not take \
                     effect until a reload succeeds."
                );
                Err(e)
            }
        }
    }

    /// Read and validate every artifact a reload installs, installing none of
    /// them.
    ///
    /// Split out of [`reload`](Self::reload) so the all-or-nothing property is
    /// structural rather than a matter of ordering statements carefully: the
    /// caller cannot store anything until this has returned `Ok` for both.
    fn prepare_reload(&self) -> Result<(CertifiedKey, Arc<WebPkiServerVerifier>)> {
        let origin = &self.inner.origin;
        let material = load_identity_material(origin)?;
        let key = certified_key_from_material(&material, origin)?;
        let verifier = build_peer_verifier(origin)?;
        Ok((key, verifier))
    }

    /// The rustls configuration behind this source, with `alpn` as its ALPN
    /// protocol list.
    ///
    /// The returned value shares this source's rotating resolver and verifier,
    /// so a transport built from it rotates with the source.
    fn tls_config_with_alpn(&self, alpn: &[&[u8]]) -> ClientConfig {
        let mut tls = self.inner.tls.clone();
        tls.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
        tls
    }

    /// A gRPC channel whose TLS identity rotates with this source.
    ///
    /// The channel connects lazily, through a connector carrying this source's
    /// rotating rustls configuration. A [`reload`](Self::reload) therefore
    /// reaches a channel that has already been handed to a generated client:
    /// connections established after the reload present the new certificate,
    /// exactly as on the HTTP path.
    ///
    /// Configure the [`Endpoint`](tonic::transport::Endpoint) before passing it
    /// in — timeouts, keep-alive, concurrency limits and user agent are all the
    /// caller's to set, and this method changes none of them. It supplies only
    /// the transport:
    ///
    /// ```rust,ignore
    /// let endpoint = tonic::transport::Endpoint::from_shared("https://peer.internal:8443")?
    ///     .timeout(Duration::from_secs(5))
    ///     .tcp_keepalive(Some(Duration::from_secs(30)));
    /// let channel = source.grpc_channel(endpoint)?;
    /// let client = MyServiceClient::new(channel);
    /// ```
    ///
    /// ALPN is fixed to `h2` alone, because gRPC is defined over HTTP/2 only; a
    /// channel that negotiated `http/1.1` would connect and then fail every
    /// RPC.
    ///
    /// # Errors
    ///
    /// Returns an error when the endpoint's URI is not `https`. Plaintext
    /// `http` is rejected rather than honoured: this source exists to present a
    /// client certificate, and silently connecting without TLS would drop the
    /// identity the caller asked for. Use a plain
    /// [`Endpoint::connect`](tonic::transport::Endpoint::connect) for
    /// deliberately unauthenticated channels.
    ///
    /// Connection and handshake failures are *not* reported here. The channel
    /// is lazy, so they surface on the first RPC, which is also where `tonic`'s
    /// own reconnection logic can act on them.
    #[cfg(feature = "grpc")]
    pub fn grpc_channel(
        &self,
        endpoint: tonic::transport::Endpoint,
    ) -> Result<tonic::transport::Channel> {
        let scheme = endpoint.uri().scheme_str();
        if scheme != Some("https") {
            return Err(Error::Internal(format!(
                "gRPC endpoint '{}' must use the https scheme to present a client \
                 identity, but it uses '{}'",
                endpoint.uri(),
                scheme.unwrap_or("(none)")
            )));
        }

        let connector = RotatingTlsConnector {
            tls: Arc::new(self.tls_config_with_alpn(&GRPC_ALPN)),
        };
        Ok(endpoint.connect_with_connector_lazy(connector))
    }

    /// A point-in-time gRPC TLS configuration built from this source's files.
    ///
    /// Still named `_snapshot`, and still one: a channel built from this value
    /// through `tonic`'s own TLS plumbing fixes its configuration when it
    /// connects and is unaffected by any later [`reload`](Self::reload).
    /// Prefer [`grpc_channel`](Self::grpc_channel), which rotates. Reach for
    /// this only when something in `tonic`'s `ClientTlsConfig` is needed that
    /// the connector path does not offer, and accept that rotating then means
    /// rebuilding the channel and the generated client that holds it.
    ///
    /// Reads the files directly rather than deriving from the installed
    /// credentials, so the returned configuration reflects what is on disk now,
    /// which may differ from what the source is serving if a reload has failed
    /// since.
    ///
    /// # Errors
    ///
    /// Returns an error when the certificate, key or CA bundle cannot be read,
    /// parsed or validated.
    #[cfg(feature = "grpc")]
    pub fn tonic_client_tls_config_snapshot(&self) -> Result<tonic::transport::ClientTlsConfig> {
        tonic_client_tls_config(&self.inner.origin)
    }
}

impl std::fmt::Debug for ClientIdentitySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The installed credentials include the private key, so describe the
        // source by where it came from rather than by what it holds. The origin
        // names file paths only, never their contents.
        f.debug_struct("ClientIdentitySource")
            .field("origin", &self.inner.origin)
            .finish_non_exhaustive()
    }
}

/// A `tower` service that TCP-connects and performs a rustls handshake, for
/// [`tonic::transport::Endpoint::connect_with_connector_lazy`].
///
/// Holds the rustls configuration rather than any credential, so the rotating
/// resolver and verifier inside it are shared with the source that built it and
/// every connection this connector makes picks up whatever is current.
#[cfg(feature = "grpc")]
#[derive(Clone)]
struct RotatingTlsConnector {
    /// Shared with the owning source; ALPN restricted to `h2`.
    tls: Arc<ClientConfig>,
}

#[cfg(feature = "grpc")]
impl std::fmt::Debug for RotatingTlsConnector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The configuration reaches a private key through its cert resolver, so
        // report only the one field that is safe and useful to see.
        f.debug_struct("RotatingTlsConnector")
            .field("alpn_protocols", &self.tls.alpn_protocols)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "grpc")]
impl tower::Service<http::Uri> for RotatingTlsConnector {
    type Response = hyper_util::rt::TokioIo<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>;
    type Error = Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response>> + Send + 'static>,
    >;

    fn poll_ready(&mut self, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<()>> {
        // Nothing is pooled or rate-limited here: every call opens its own
        // socket, so the connector is always ready.
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, uri: http::Uri) -> Self::Future {
        let tls = Arc::clone(&self.tls);
        Box::pin(async move { connect_tls(tls, uri).await })
    }
}

/// Split a URI host into the address to dial and the name to verify the peer
/// against.
///
/// [`http::Uri::host`] returns an IPv6 literal still wrapped in the square
/// brackets the URI syntax requires (`[::1]`), and neither
/// [`std::net::ToSocketAddrs`] nor [`ServerName`] accepts that form: the
/// brackets are URI punctuation, not part of the address. They are stripped
/// here, exactly as `hyper-util`'s own connector does, so an IPv6-literal
/// endpoint is usable rather than failing to parse.
///
/// The dial address and the verified name are derived from the same string, so
/// a channel cannot end up connecting to one host and verifying a certificate
/// for another. IP literals are carried through as
/// [`ServerName::IpAddress`], which is correct for the internal mutual-TLS
/// deployments this module serves, where peers legitimately hold certificates
/// with IP subject-alternative names.
///
/// Kept pure and separate from [`connect_tls`] so the parsing can be tested
/// without a listener.
#[cfg(feature = "grpc")]
fn resolve_target(host: &str) -> Result<(&str, ServerName<'static>)> {
    let connect_host = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(host);

    let server_name = ServerName::try_from(connect_host.to_string()).map_err(|e| {
        Error::Internal(format!(
            "gRPC endpoint host '{}' is not a valid TLS server name: {}",
            connect_host, e
        ))
    })?;

    Ok((connect_host, server_name))
}

/// Open a TCP connection to `uri`'s authority and complete a rustls handshake
/// over it using `tls`.
///
/// Kept out of [`RotatingTlsConnector::call`] so the connector's `Service`
/// implementation stays a thin adapter and this stays an ordinary async
/// function that can be reasoned about, and tested, on its own.
#[cfg(feature = "grpc")]
async fn connect_tls(
    tls: Arc<ClientConfig>,
    uri: http::Uri,
) -> Result<hyper_util::rt::TokioIo<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>> {
    let host = uri.host().ok_or_else(|| {
        Error::Internal(format!("gRPC endpoint '{}' has no host to connect to", uri))
    })?;
    // 443 is the default for https, which `grpc_channel` has already required.
    let port = uri.port_u16().unwrap_or(443);

    let (connect_host, server_name) = resolve_target(host)?;

    let tcp = tokio::net::TcpStream::connect((connect_host, port))
        .await
        .map_err(|e| {
            Error::Internal(format!(
                "Failed to connect to gRPC endpoint '{}': {}",
                uri, e
            ))
        })?;
    // gRPC sends many small frames; Nagle's algorithm would add latency to
    // every one. `tonic`'s own connector sets this for the same reason.
    tcp.set_nodelay(true).map_err(|e| {
        Error::Internal(format!(
            "Failed to disable Nagle's algorithm on the connection to '{}': {}",
            uri, e
        ))
    })?;

    let stream = tokio_rustls::TlsConnector::from(tls)
        .connect(server_name, tcp)
        .await
        .map_err(|e| {
            Error::Internal(format!(
                "TLS handshake with gRPC endpoint '{}' failed: {}",
                uri, e
            ))
        })?;

    Ok(hyper_util::rt::TokioIo::new(stream))
}

/// Build the single `reqwest` client a [`ClientIdentitySource`] hands out.
///
/// Takes the rotating rustls configuration rather than reloading from disk,
/// because this client is built once and never rebuilt: rotation reaches it
/// through the resolver and verifier inside `tls`.
///
/// ALPN is set here rather than left to `reqwest`. `use_preconfigured_tls`
/// takes the supplied configuration verbatim and adds no protocols of its own,
/// so an unset list would negotiate nothing and silently downgrade every
/// request to HTTP/1.1.
fn build_client(tls: &ClientConfig, config: &ClientIdentityConfig) -> Result<reqwest::Client> {
    let mut tls = tls.clone();
    tls.alpn_protocols = HTTP_ALPN.iter().map(|p| p.to_vec()).collect();

    reqwest::Client::builder()
        .use_preconfigured_tls(tls)
        .build()
        .map_err(|e| {
            Error::Internal(format!(
                "Failed to build a reqwest client for the identity in '{}': {}",
                config.cert_path.display(),
                e
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

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

    fn write_temp(contents: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        file.write_all(contents.as_bytes()).expect("write temp");
        file.flush().expect("flush temp");
        file
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
        }
    }

    /// The base64 body of a PEM document: every line that is not a delimiter.
    /// Used to check that key bytes do not leak into diagnostic output.
    fn pem_body(pem: &str) -> String {
        pem.lines()
            .filter(|line| !line.starts_with("-----"))
            .collect::<Vec<_>>()
            .join("")
    }

    #[test]
    fn concat_identity_pem_inserts_a_separator_when_the_cert_lacks_one() {
        let joined = concat_identity_pem(b"CERT", b"KEY");

        assert_eq!(
            joined.as_slice(),
            b"CERT\nKEY",
            "a cert without a trailing newline must not be spliced onto the key"
        );
    }

    #[test]
    fn concat_identity_pem_does_not_double_an_existing_separator() {
        let joined = concat_identity_pem(b"CERT\n", b"KEY");

        assert_eq!(
            joined.as_slice(),
            b"CERT\nKEY",
            "a cert already ending in a newline must not gain a blank line"
        );
    }

    #[test]
    fn concat_identity_pem_preserves_both_documents_in_order() {
        let cert = generate_cert("client");
        let joined = concat_identity_pem(cert.cert_pem.as_bytes(), cert.key_pem.as_bytes());
        let text = String::from_utf8(joined.to_vec()).expect("pem is utf-8");

        let cert_at = text.find("BEGIN CERTIFICATE").expect("cert present");
        let key_at = text.find("PRIVATE KEY").expect("key present");
        assert!(
            cert_at < key_at,
            "the certificate must precede the key in the joined buffer"
        );
    }

    #[test]
    fn load_identity_material_reads_a_matching_pair() {
        let cert = generate_cert("client");
        let cert_file = write_temp(&cert.cert_pem);
        let key_file = write_temp(&cert.key_pem);
        let config = config_for(
            cert_file.path().to_path_buf(),
            key_file.path().to_path_buf(),
        );

        let material = load_identity_material(&config).expect("a matching pair must load");

        assert_eq!(
            material.chain.len(),
            1,
            "a single self-signed cert must yield a one-element chain"
        );
        assert!(!material.key_pem.is_empty(), "the key bytes must be read");
    }

    #[test]
    fn load_identity_material_rejects_a_missing_cert_file() {
        let cert = generate_cert("client");
        let key_file = write_temp(&cert.key_pem);
        let config = config_for(
            PathBuf::from("/nonexistent/client.pem"),
            key_file.path().to_path_buf(),
        );

        let err = load_identity_material(&config)
            .expect_err("a missing certificate must fail the load, not yield an empty chain");

        assert!(
            err.to_string()
                .contains("Failed to open client identity cert file"),
            "error must name the failure to open the cert: {err}"
        );
    }

    #[test]
    fn load_identity_material_rejects_a_missing_key_file() {
        let cert = generate_cert("client");
        let cert_file = write_temp(&cert.cert_pem);
        let config = config_for(
            cert_file.path().to_path_buf(),
            PathBuf::from("/nonexistent/client.key"),
        );

        let err = load_identity_material(&config).expect_err("a missing key must fail the load");

        assert!(
            err.to_string()
                .contains("Failed to open client identity key file"),
            "error must name the failure to open the key: {err}"
        );
    }

    #[test]
    fn load_identity_material_rejects_a_cert_file_without_certificates() {
        let cert = generate_cert("client");
        let cert_file = write_temp("# no certificates here\n");
        let key_file = write_temp(&cert.key_pem);
        let config = config_for(
            cert_file.path().to_path_buf(),
            key_file.path().to_path_buf(),
        );

        let err = load_identity_material(&config)
            .expect_err("a cert file with no certificates must fail the load");

        assert!(
            err.to_string().contains("contains no certificates"),
            "error must explain that the chain is empty: {err}"
        );
    }

    #[test]
    fn load_identity_material_rejects_an_unparseable_key() {
        let cert = generate_cert("client");
        let cert_file = write_temp(&cert.cert_pem);
        let key_file = write_temp("-----BEGIN PRIVATE KEY-----\ntruncated");
        let config = config_for(
            cert_file.path().to_path_buf(),
            key_file.path().to_path_buf(),
        );

        let err = load_identity_material(&config).expect_err("a truncated key must fail the load");

        assert!(
            err.to_string()
                .contains("Failed to parse client identity private key"),
            "error must name the key parse failure: {err}"
        );
    }

    #[test]
    fn load_identity_material_rejects_a_mismatched_cert_and_key() {
        let cert = generate_cert("client");
        let other = generate_cert("someone-else");
        let cert_file = write_temp(&cert.cert_pem);
        let key_file = write_temp(&other.key_pem);
        let config = config_for(
            cert_file.path().to_path_buf(),
            key_file.path().to_path_buf(),
        );

        let err = load_identity_material(&config)
            .expect_err("a key from a different key pair must fail the load");

        assert!(
            err.to_string().contains("does not match the certificate"),
            "error must say the pair does not match: {err}"
        );
    }

    #[test]
    fn load_rustls_client_config_accepts_a_matching_pair() {
        let cert = generate_cert("client");
        let cert_file = write_temp(&cert.cert_pem);
        let key_file = write_temp(&cert.key_pem);
        let config = config_for(
            cert_file.path().to_path_buf(),
            key_file.path().to_path_buf(),
        );

        let tls = load_rustls_client_config(&config).expect("a matching pair must build");

        assert!(
            tls.alpn_protocols.is_empty(),
            "the documented contract is that no ALPN protocols are set"
        );
    }

    #[test]
    fn load_rustls_client_config_rejects_a_mismatched_cert_and_key() {
        let cert = generate_cert("client");
        let other = generate_cert("someone-else");
        let cert_file = write_temp(&cert.cert_pem);
        let key_file = write_temp(&other.key_pem);
        let config = config_for(
            cert_file.path().to_path_buf(),
            key_file.path().to_path_buf(),
        );

        let err = load_rustls_client_config(&config)
            .expect_err("the pair must be validated, not merely parsed");

        assert!(
            err.to_string().contains("does not match the certificate"),
            "error must identify the mismatch rather than a generic build failure: {err}"
        );
    }

    #[test]
    fn load_rustls_client_config_rejects_an_unreadable_peer_ca() {
        let cert = generate_cert("client");
        let cert_file = write_temp(&cert.cert_pem);
        let key_file = write_temp(&cert.key_pem);
        let mut config = config_for(
            cert_file.path().to_path_buf(),
            key_file.path().to_path_buf(),
        );
        config.root_ca_path = Some(PathBuf::from("/nonexistent/peer-ca.pem"));

        let err = load_rustls_client_config(&config)
            .expect_err("an unreadable peer CA must fail the whole build");

        assert!(
            err.to_string().contains("Failed to open peer CA file"),
            "error must name the peer CA, not the server's client CA: {err}"
        );
    }

    #[test]
    fn additive_roots_keep_the_built_in_web_pki_anchors() {
        let cert = generate_cert("client");
        let ca = generate_cert("peer-ca");
        let cert_file = write_temp(&cert.cert_pem);
        let key_file = write_temp(&cert.key_pem);
        let ca_file = write_temp(&ca.cert_pem);
        let mut config = config_for(
            cert_file.path().to_path_buf(),
            key_file.path().to_path_buf(),
        );
        config.root_ca_path = Some(ca_file.path().to_path_buf());

        let roots = build_root_store(&config).expect("roots must build");

        assert_eq!(
            roots.len(),
            webpki_roots::TLS_SERVER_ROOTS.len() + 1,
            "the default is additive: the private CA joins the public roots"
        );
    }

    #[test]
    fn exclusive_roots_replace_the_built_in_web_pki_anchors() {
        let cert = generate_cert("client");
        let ca = generate_cert("peer-ca");
        let cert_file = write_temp(&cert.cert_pem);
        let key_file = write_temp(&cert.key_pem);
        let ca_file = write_temp(&ca.cert_pem);
        let mut config = config_for(
            cert_file.path().to_path_buf(),
            key_file.path().to_path_buf(),
        );
        config.root_ca_path = Some(ca_file.path().to_path_buf());
        config.exclusive_roots = true;

        let roots = build_root_store(&config).expect("roots must build");

        assert_eq!(
            roots.len(),
            1,
            "exclusive roots must pin trust to the configured CA alone"
        );
    }

    #[test]
    fn no_peer_ca_falls_back_to_the_built_in_web_pki_anchors() {
        let cert = generate_cert("client");
        let cert_file = write_temp(&cert.cert_pem);
        let key_file = write_temp(&cert.key_pem);
        let mut config = config_for(
            cert_file.path().to_path_buf(),
            key_file.path().to_path_buf(),
        );
        config.exclusive_roots = true;

        let roots = build_root_store(&config).expect("roots must build");

        assert_eq!(
            roots.len(),
            webpki_roots::TLS_SERVER_ROOTS.len(),
            "exclusive_roots must be ignored without a bundle to replace them with"
        );
    }

    #[test]
    fn load_reqwest_identity_accepts_a_matching_pair() {
        let cert = generate_cert("client");
        let cert_file = write_temp(&cert.cert_pem);
        let key_file = write_temp(&cert.key_pem);
        let config = config_for(
            cert_file.path().to_path_buf(),
            key_file.path().to_path_buf(),
        );

        load_reqwest_identity(&config).expect("a matching pair must yield a reqwest identity");
    }

    #[test]
    fn reqwest_client_builder_builds_a_usable_client() {
        let cert = generate_cert("client");
        let cert_file = write_temp(&cert.cert_pem);
        let key_file = write_temp(&cert.key_pem);
        let config = config_for(
            cert_file.path().to_path_buf(),
            key_file.path().to_path_buf(),
        );

        reqwest_client_builder(&config)
            .expect("builder must be produced")
            .build()
            .expect("the builder must produce a client");
    }

    #[test]
    fn reqwest_client_builder_rejects_a_peer_ca_without_certificates() {
        let cert = generate_cert("client");
        let cert_file = write_temp(&cert.cert_pem);
        let key_file = write_temp(&cert.key_pem);
        let ca_file = write_temp("# no certificates here\n");
        let mut config = config_for(
            cert_file.path().to_path_buf(),
            key_file.path().to_path_buf(),
        );
        config.root_ca_path = Some(ca_file.path().to_path_buf());

        let err = reqwest_client_builder(&config)
            .expect_err("an empty peer CA bundle must fail rather than trust nothing extra");

        assert!(
            err.to_string().contains("contains no certificates"),
            "error must explain that the bundle is empty: {err}"
        );
    }

    #[test]
    fn from_config_rejects_credentials_that_do_not_load() {
        let cert = generate_cert("client");
        let key_file = write_temp(&cert.key_pem);
        let config = config_for(
            PathBuf::from("/nonexistent/client.pem"),
            key_file.path().to_path_buf(),
        );

        ClientIdentitySource::from_config(&config)
            .expect_err("no source must exist when the identity cannot be loaded");
    }

    #[test]
    fn origin_reports_the_files_the_source_reloads_from() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = write_identity(dir.path(), &generate_cert("client"));

        let source = ClientIdentitySource::from_config(&config).expect("initial load");

        assert_eq!(source.origin().cert_path, config.cert_path);
        assert_eq!(source.origin().key_path, config.key_path);
    }

    /// The DER bytes of the leaf certificate the source would present on its
    /// next handshake. The value rotation is supposed to change.
    fn presented_leaf(source: &ClientIdentitySource) -> Vec<u8> {
        source
            .inner
            .resolver
            .current()
            .end_entity_cert()
            .expect("a resolver always holds a chain with a leaf")
            .to_vec()
    }

    #[test]
    fn a_successful_reload_swaps_the_key_the_resolver_presents() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = write_identity(dir.path(), &generate_cert("client"));
        let source = ClientIdentitySource::from_config(&config).expect("initial load");
        let before = presented_leaf(&source);

        let second = generate_cert("client");
        std::fs::write(&config.cert_path, &second.cert_pem).expect("rewrite cert");
        std::fs::write(&config.key_path, &second.key_pem).expect("rewrite key");
        source
            .reload()
            .expect("rereading valid credentials must succeed");

        assert_ne!(
            presented_leaf(&source),
            before,
            "a successful reload must install the new certificate in the resolver, \
             because that is what every subsequent handshake reads"
        );
    }

    #[test]
    fn a_cached_client_handle_observes_a_reload() {
        // The inversion of the footgun this type used to document. Rotation now
        // happens inside the client's TLS layer via `ResolvesClientCert`, so the
        // client itself is stable: the same handle rotates, and the connection
        // pool it carries survives. Callers may hold on to it.
        let dir = tempfile::tempdir().expect("temp dir");
        let config = write_identity(dir.path(), &generate_cert("client"));
        let source = ClientIdentitySource::from_config(&config).expect("initial load");

        let cached = source.client();
        let before = presented_leaf(&source);

        let second = generate_cert("client");
        std::fs::write(&config.cert_path, &second.cert_pem).expect("rewrite cert");
        std::fs::write(&config.key_path, &second.key_pem).expect("rewrite key");
        source.reload().expect("reload");

        assert!(
            Arc::ptr_eq(&cached, &source.client()),
            "the handle must be the same client after a reload: rebuilding it would \
             discard the connection pool and reintroduce the do-not-cache rule"
        );
        assert_ne!(
            presented_leaf(&source),
            before,
            "the cached handle must nevertheless present the rotated certificate"
        );
    }

    #[test]
    fn failed_reload_preserves_the_last_good_identity() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = write_identity(dir.path(), &generate_cert("client"));
        let source = ClientIdentitySource::from_config(&config).expect("initial load");
        let last_good = presented_leaf(&source);
        let client = source.client();

        // Simulate a half-written rotation: the cert file is no longer parseable.
        std::fs::write(&config.cert_path, "-----BEGIN CERTIFICATE-----\ntruncated")
            .expect("corrupt cert");

        let err = source
            .reload()
            .expect_err("an unparseable certificate must fail the reload");

        assert_eq!(
            presented_leaf(&source),
            last_good,
            "a failed reload must keep the last-good identity, not drop it"
        );
        assert!(
            Arc::ptr_eq(&source.client(), &client),
            "a failed reload must not disturb the client either"
        );
        assert!(
            !err.to_string().is_empty(),
            "the failure must be reported to the caller: {err}"
        );

        // And the source recovers once the files are valid again.
        let replacement = generate_cert("client");
        std::fs::write(&config.cert_path, &replacement.cert_pem).expect("rewrite cert");
        std::fs::write(&config.key_path, &replacement.key_pem).expect("rewrite key");
        source.reload().expect("a later valid reload must succeed");
        assert_ne!(
            presented_leaf(&source),
            last_good,
            "the recovered reload must install the new identity"
        );
    }

    #[test]
    fn a_corrupt_ca_bundle_blocks_an_otherwise_valid_identity_rotation() {
        // The all-or-nothing half of the fail-closed contract. A rotation that
        // writes a good certificate and a bad CA bundle must install neither:
        // a new identity paired with stale anchors is a state no observer
        // should ever see.
        let dir = tempfile::tempdir().expect("temp dir");
        let mut config = write_identity(dir.path(), &generate_cert("client"));
        let ca_path = dir.path().join("peer-ca.pem");
        std::fs::write(&ca_path, generate_cert("peer-ca").cert_pem).expect("write ca");
        config.root_ca_path = Some(ca_path.clone());

        let source = ClientIdentitySource::from_config(&config).expect("initial load");
        let last_good = presented_leaf(&source);

        // A perfectly good new identity...
        let second = generate_cert("client");
        std::fs::write(&config.cert_path, &second.cert_pem).expect("rewrite cert");
        std::fs::write(&config.key_path, &second.key_pem).expect("rewrite key");
        // ...alongside a CA bundle that no longer parses.
        std::fs::write(&ca_path, "-----BEGIN CERTIFICATE-----\ntruncated").expect("corrupt ca");

        source
            .reload()
            .expect_err("a corrupt CA bundle must fail the whole reload");

        assert_eq!(
            presented_leaf(&source),
            last_good,
            "the valid new certificate must NOT have been installed: the reload is \
             all-or-nothing, and its trust anchors could not be loaded"
        );

        // Fixing only the CA bundle lets the whole rotation through.
        std::fs::write(&ca_path, generate_cert("peer-ca-2").cert_pem).expect("rewrite ca");
        source.reload().expect("a fully valid reload must succeed");
        assert_ne!(
            presented_leaf(&source),
            last_good,
            "once every artifact loads, the rotation must apply in full"
        );
    }

    #[test]
    fn clones_of_a_source_observe_the_same_reload() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = write_identity(dir.path(), &generate_cert("client"));
        let source = ClientIdentitySource::from_config(&config).expect("initial load");
        let caller_side = source.clone();
        let before = presented_leaf(&caller_side);

        let second = generate_cert("client");
        std::fs::write(&config.cert_path, &second.cert_pem).expect("rewrite cert");
        std::fs::write(&config.key_path, &second.key_pem).expect("rewrite key");
        source.reload().expect("reload");

        assert!(
            Arc::ptr_eq(&caller_side.client(), &source.client()),
            "every clone must hand out the same client"
        );
        assert_ne!(
            presented_leaf(&caller_side),
            before,
            "the clone held by the caller must see the rotation"
        );
    }

    #[test]
    fn the_http_client_negotiates_http2_and_http11() {
        // `use_preconfigured_tls` injects no ALPN protocols, so an unset list
        // would silently downgrade every request to HTTP/1.1. This pins the
        // list the source sets on the client's behalf.
        let dir = tempfile::tempdir().expect("temp dir");
        let config = write_identity(dir.path(), &generate_cert("client"));
        let source = ClientIdentitySource::from_config(&config).expect("initial load");

        let tls = source.tls_config_with_alpn(&HTTP_ALPN);

        assert_eq!(
            tls.alpn_protocols,
            vec![b"h2".to_vec(), b"http/1.1".to_vec()],
            "the HTTP path must offer h2 first, then http/1.1, as reqwest does natively"
        );
    }

    #[test]
    fn the_resolver_always_reports_that_it_has_certificates() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = write_identity(dir.path(), &generate_cert("client"));
        let source = ClientIdentitySource::from_config(&config).expect("initial load");

        assert!(
            source.inner.resolver.has_certs(),
            "a source cannot exist without a valid identity, so the resolver must \
             never tell rustls to skip client authentication"
        );
        assert!(
            source.inner.resolver.resolve(&[], &[]).is_some(),
            "resolve must return the identity even when the server sends no hints"
        );
    }

    #[test]
    fn rotating_type_debug_output_does_not_expose_key_material() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cert = generate_cert("client");
        let config = write_identity(dir.path(), &cert);
        let source = ClientIdentitySource::from_config(&config).expect("initial load");

        let key_body = pem_body(&cert.key_pem);
        assert!(
            !key_body.is_empty(),
            "the test needs a non-empty key body to search for"
        );

        for rendered in [
            format!("{:?}", source.inner.resolver),
            format!("{:?}", source.inner.verifier),
        ] {
            assert!(
                !rendered.contains(&key_body),
                "Debug must never render private key material: {rendered}"
            );
        }
    }

    #[test]
    fn debug_does_not_expose_key_material() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cert = generate_cert("client");
        let config = write_identity(dir.path(), &cert);
        let source = ClientIdentitySource::from_config(&config).expect("initial load");

        let rendered = format!("{source:?}");

        let key_body = pem_body(&cert.key_pem);
        assert!(
            !key_body.is_empty(),
            "the test needs a non-empty key body to search for"
        );
        assert!(
            !rendered.contains(&key_body),
            "Debug must never render private key material"
        );
        assert!(
            rendered.contains("ClientIdentitySource"),
            "Debug must still identify the type: {rendered}"
        );
        assert!(
            rendered.contains("cert_path"),
            "Debug must report the origin so a misconfiguration is diagnosable: {rendered}"
        );
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn tonic_client_tls_config_accepts_a_matching_pair() {
        let cert = generate_cert("client");
        let cert_file = write_temp(&cert.cert_pem);
        let key_file = write_temp(&cert.key_pem);
        let config = config_for(
            cert_file.path().to_path_buf(),
            key_file.path().to_path_buf(),
        );

        tonic_client_tls_config(&config).expect("a matching pair must yield a tonic TLS config");
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn tonic_client_tls_config_rejects_unparseable_pem_at_config_time() {
        // `tonic::transport::Identity::from_pem` is infallible and defers every
        // parse error to connect time. This asserts that the eager validation
        // in `tonic_client_tls_config` turns that into a startup failure.
        let cert = generate_cert("client");
        let cert_file = write_temp("-----BEGIN CERTIFICATE-----\ntruncated");
        let key_file = write_temp(&cert.key_pem);
        let config = config_for(
            cert_file.path().to_path_buf(),
            key_file.path().to_path_buf(),
        );

        tonic_client_tls_config(&config)
            .expect_err("unparseable PEM must fail here rather than on the first gRPC call");
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn tonic_snapshot_reflects_the_files_not_the_installed_client() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = write_identity(dir.path(), &generate_cert("client"));
        let source = ClientIdentitySource::from_config(&config).expect("initial load");

        source
            .tonic_client_tls_config_snapshot()
            .expect("a snapshot must build from valid files");

        // Corrupt the files without reloading: the installed HTTP client is
        // still good, but a snapshot reads disk and must fail.
        std::fs::write(&config.cert_path, "-----BEGIN CERTIFICATE-----\ntruncated")
            .expect("corrupt cert");

        source
            .tonic_client_tls_config_snapshot()
            .expect_err("a snapshot reads the files, so it must surface a corrupt rotation");
        source
            .client()
            .get("https://example.invalid/")
            .build()
            .expect("the installed client must be unaffected by the corrupt files");
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn grpc_channel_rejects_a_plaintext_endpoint() {
        // Honouring `http` would silently drop the client identity this source
        // exists to present, so it must be an error rather than a downgrade.
        let dir = tempfile::tempdir().expect("temp dir");
        let config = write_identity(dir.path(), &generate_cert("client"));
        let source = ClientIdentitySource::from_config(&config).expect("initial load");

        let endpoint = tonic::transport::Endpoint::from_static("http://peer.internal:8443");
        let err = source
            .grpc_channel(endpoint)
            .expect_err("a plaintext endpoint must not silently drop the client identity");

        assert!(
            err.to_string().contains("https"),
            "the error must say the scheme is the problem: {err}"
        );
    }

    // `connect_with_connector_lazy` installs hyper's timer and I/O plumbing, so
    // it requires a reactor even though it dials nothing.
    #[cfg(feature = "grpc")]
    #[tokio::test]
    async fn grpc_channel_builds_lazily_from_an_https_endpoint() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = write_identity(dir.path(), &generate_cert("client"));
        let source = ClientIdentitySource::from_config(&config).expect("initial load");

        // Nothing is dialled: the channel is lazy, so an unroutable authority
        // must still yield a channel rather than a connection error.
        let endpoint = tonic::transport::Endpoint::from_static("https://peer.invalid:8443");
        source
            .grpc_channel(endpoint)
            .expect("a lazy channel must build without contacting the peer");
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn the_grpc_path_offers_only_http2() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = write_identity(dir.path(), &generate_cert("client"));
        let source = ClientIdentitySource::from_config(&config).expect("initial load");

        let connector = RotatingTlsConnector {
            tls: Arc::new(source.tls_config_with_alpn(&GRPC_ALPN)),
        };

        assert_eq!(
            connector.tls.alpn_protocols,
            vec![b"h2".to_vec()],
            "gRPC is defined over HTTP/2 only; offering http/1.1 would let a peer \
             negotiate a protocol on which every RPC then fails"
        );
        assert!(
            !format!("{connector:?}").contains("PRIVATE KEY"),
            "the connector's Debug must not reach into key material"
        );
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn resolve_target_passes_a_plain_hostname_through_unchanged() {
        let (host, name) = resolve_target("peer.internal").expect("a DNS name must resolve");

        assert_eq!(host, "peer.internal", "the dial address must be unchanged");
        assert_eq!(
            name,
            ServerName::try_from("peer.internal").expect("valid"),
            "a hostname must be verified as a DNS name"
        );
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn resolve_target_accepts_an_ipv4_literal() {
        let (host, name) = resolve_target("127.0.0.1").expect("an IPv4 literal must resolve");

        assert_eq!(host, "127.0.0.1");
        assert!(
            matches!(name, ServerName::IpAddress(_)),
            "an IP literal must be verified as an IP address, not a DNS name: {name:?}"
        );
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn resolve_target_strips_the_brackets_from_an_ipv6_literal() {
        // `http::Uri::host` returns IPv6 literals still bracketed. Leaving the
        // brackets on would fail both the ServerName parse and the TCP connect,
        // making IPv6 gRPC endpoints unusable.
        let (host, name) = resolve_target("[::1]").expect("a bracketed IPv6 literal must resolve");

        assert_eq!(
            host, "::1",
            "the brackets are URI punctuation and must not reach the resolver"
        );
        assert!(
            matches!(name, ServerName::IpAddress(_)),
            "an IPv6 literal must be verified as an IP address: {name:?}"
        );
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn resolve_target_rejects_a_host_that_is_not_a_valid_server_name() {
        let err = resolve_target("not a hostname")
            .expect_err("an unparseable host must fail rather than reach the network");

        assert!(
            err.to_string().contains("is not a valid TLS server name"),
            "the error must name the problem: {err}"
        );
    }

    // ---------------------------------------------------------------------
    // End-to-end: a live handshake against a local rustls server, proving that
    // the certificate actually presented on the wire changes after a reload
    // while the client handle stays the same.
    // ---------------------------------------------------------------------

    /// A client-certificate verifier that accepts every chain and records the
    /// leaf it was shown.
    ///
    /// Test-only, and deliberately not a policy: the assertion under test is
    /// *which certificate the client presented*, so the server must accept
    /// whatever arrives in order to observe it. Building a real client-CA chain
    /// instead would test rcgen and webpki rather than the rotation logic.
    #[derive(Debug)]
    struct CapturingClientVerifier {
        /// Leaf certificates presented, in the order the handshakes completed.
        seen: Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
        /// A stock verifier, used for the handshake *signature* checks only.
        ///
        /// Those stay genuine, so a client that presented a certificate whose
        /// private key it did not hold would still fail the handshake and the
        /// test. Only the chain-of-trust decision is overridden, and this
        /// verifier's own roots are therefore never consulted.
        inner: Arc<dyn tokio_rustls::rustls::server::danger::ClientCertVerifier>,
    }

    impl tokio_rustls::rustls::server::danger::ClientCertVerifier for CapturingClientVerifier {
        fn root_hint_subjects(&self) -> &[tokio_rustls::rustls::DistinguishedName] {
            &[]
        }

        fn verify_client_cert(
            &self,
            end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _now: UnixTime,
        ) -> std::result::Result<
            tokio_rustls::rustls::server::danger::ClientCertVerified,
            tokio_rustls::rustls::Error,
        > {
            self.seen
                .lock()
                .expect("the capture list is only locked to push")
                .push(end_entity.to_vec());
            Ok(tokio_rustls::rustls::server::danger::ClientCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> std::result::Result<HandshakeSignatureValid, tokio_rustls::rustls::Error> {
            self.inner.verify_tls12_signature(message, cert, dss)
        }

        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> std::result::Result<HandshakeSignatureValid, tokio_rustls::rustls::Error> {
            self.inner.verify_tls13_signature(message, cert, dss)
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            self.inner.supported_verify_schemes()
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

    #[tokio::test]
    async fn a_live_handshake_presents_the_rotated_certificate_on_the_same_client() {
        use rustls_pki_types::pem::PemObject;

        crate::crypto::ensure_default_crypto_provider();

        // --- a server that captures whatever client certificate it is shown ---
        let chain = generate_server_chain();
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));

        // Roots for the inner verifier the capturing one borrows its signature
        // checks from. Never consulted for the chain decision, which is
        // overridden, so any valid CA serves.
        let mut inner_roots = RootCertStore::empty();
        for cert in CertificateDer::pem_slice_iter(chain.ca_pem.as_bytes()) {
            inner_roots
                .add(cert.expect("ca parses"))
                .expect("ca is a usable anchor");
        }
        let inner_verifier =
            tokio_rustls::rustls::server::WebPkiClientVerifier::builder(Arc::new(inner_roots))
                .build()
                .expect("inner verifier builds");

        let server_certs: Vec<CertificateDer<'static>> =
            CertificateDer::pem_slice_iter(chain.leaf_pem.as_bytes())
                .collect::<std::result::Result<Vec<_>, _>>()
                .expect("server chain parses");
        let server_key = PrivateKeyDer::from_pem_slice(chain.leaf_key_pem.as_bytes())
            .expect("server key parses");

        let mut server_config = tokio_rustls::rustls::ServerConfig::builder()
            .with_client_cert_verifier(Arc::new(CapturingClientVerifier {
                seen: Arc::clone(&seen),
                inner: inner_verifier,
            }))
            .with_single_cert(server_certs, server_key)
            .expect("server config");
        server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_config));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port");
        let port = listener.local_addr().expect("local addr").port();

        // Accept and complete one handshake per connection, then drop it. The
        // handshake is the whole point; nothing is spoken on top of it.
        let server = tokio::spawn(async move {
            loop {
                let Ok((tcp, _)) = listener.accept().await else {
                    return;
                };
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    let _ = acceptor.accept(tcp).await;
                });
            }
        });

        // --- a client that trusts that CA and presents a rotatable identity ---
        let dir = tempfile::tempdir().expect("temp dir");
        let first = generate_cert("client-one");
        let mut config = write_identity(dir.path(), &first);
        let ca_path = dir.path().join("peer-ca.pem");
        std::fs::write(&ca_path, &chain.ca_pem).expect("write ca");
        config.root_ca_path = Some(ca_path);
        config.exclusive_roots = true;

        let source = ClientIdentitySource::from_config(&config).expect("initial load");
        // Taken once, before the rotation, and still held after it. Under the
        // previous design this handle could never have rotated.
        let client = source.client();

        // Handshakes are driven through `tokio_rustls` rather than through
        // `client`, for determinism: this is exactly the configuration the
        // client holds — same `Arc`d resolver, same verifier, obtained from the
        // source itself — but connecting explicitly guarantees one fresh
        // handshake per call, with no connection pool deciding to reuse an
        // existing one and no HTTP-layer error to disentangle from a TLS one.
        // What is asserted is the certificate that reaches the wire.
        let handshake = |tls: ClientConfig| async move {
            let tcp = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .expect("connect to the test server");
            tokio_rustls::TlsConnector::from(Arc::new(tls))
                .connect(
                    ServerName::try_from("127.0.0.1").expect("valid server name"),
                    tcp,
                )
                .await
                .expect("the handshake must succeed against the test CA")
        };

        handshake(source.tls_config_with_alpn(&HTTP_ALPN)).await;

        // --- rotate on disk and reload ---
        let second = generate_cert("client-two");
        std::fs::write(&config.cert_path, &second.cert_pem).expect("rewrite cert");
        std::fs::write(&config.key_path, &second.key_pem).expect("rewrite key");
        source.reload().expect("a valid rotation must reload");

        handshake(source.tls_config_with_alpn(&HTTP_ALPN)).await;

        // Wait for the server side of both handshakes to have recorded what it
        // was shown; the capture happens on the server's task, not ours.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while seen.lock().expect("capture list").len() < 2 {
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for two handshakes to be recorded"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        server.abort();

        let presented = seen.lock().expect("capture list").clone();
        let expected_first = CertificateDer::from_pem_slice(first.cert_pem.as_bytes())
            .expect("first cert parses")
            .to_vec();
        let expected_second = CertificateDer::from_pem_slice(second.cert_pem.as_bytes())
            .expect("second cert parses")
            .to_vec();

        assert_eq!(
            presented[0], expected_first,
            "the first handshake must present the originally configured certificate"
        );
        assert_eq!(
            presented[1], expected_second,
            "the second handshake must present the ROTATED certificate: rustls asked \
             the resolver again, and the resolver had been swapped in place"
        );
        assert!(
            Arc::ptr_eq(&client, &source.client()),
            "and none of this rebuilt the HTTP client that shares the very same \
             rotating configuration"
        );
    }
}
