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
//! | An HTTP client | [`reqwest_client_builder`] |
//! | An HTTP client that must rotate its certificate | [`ClientIdentitySource`] |
//! | A gRPC channel | [`tonic_client_tls_config`] |
//! | Something else that speaks raw rustls | [`load_rustls_client_config`] |
//!
//! [`reqwest_client_builder`] is the preferred path for HTTP. See its
//! documentation for why it is preferred over handing a
//! [`load_rustls_client_config`] result to `use_preconfigured_tls`.

use std::path::Path;
use std::sync::Arc;

use arc_swap::ArcSwap;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use zeroize::Zeroizing;

use crate::config::ClientIdentityConfig;
use crate::error::{Error, Result};

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
    verify_key_matches_chain(&material, config)?;
    Ok(material)
}

/// Confirm the private key is the one that belongs to the leaf certificate.
///
/// Split out of [`load_identity_material`] so that function stays a
/// read-and-parse routine and this one owns the single cryptographic check.
fn verify_key_matches_chain(
    material: &IdentityMaterial,
    config: &ClientIdentityConfig,
) -> Result<()> {
    use tokio_rustls::rustls::crypto::CryptoProvider;
    use tokio_rustls::rustls::sign::CertifiedKey;

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

    CertifiedKey::new(material.chain.clone(), signing_key)
        .keys_match()
        .map_err(|e| {
            Error::Internal(format!(
                "Client identity private key '{}' does not match the certificate \
                 in '{}': {}",
                config.key_path.display(),
                config.cert_path.display(),
                e
            ))
        })
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
/// This is why [`reqwest_client_builder`] is the preferred path for HTTP: it
/// leaves ALPN and HTTP-version negotiation to `reqwest`, which is the only
/// component that knows which versions the rest of its stack is prepared to
/// speak.
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
/// **This is the preferred way to make mutual-TLS HTTP calls.** The alternative,
/// building a rustls [`ClientConfig`] with [`load_rustls_client_config`] and
/// passing it to `use_preconfigured_tls`, hands `reqwest` a configuration it
/// will not adjust: ALPN, and therefore the HTTP version, becomes the caller's
/// responsibility, and getting it wrong downgrades every request to HTTP/1.1
/// without any error. Going through the builder leaves ALPN and HTTP-version
/// negotiation with `reqwest`, which owns them.
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

/// A rotatable client identity: an HTTP client whose certificate can be
/// replaced from disk while the service runs.
///
/// # This is not an in-place swap
///
/// Read this before reaching for the type, because it does not behave like its
/// server-side counterpart [`crate::tls::TlsConfigSource`]. That type swaps
/// credentials *inside* a live listener; this one cannot do the equivalent. A
/// [`reqwest::Client`] bakes its TLS configuration in at `build()` and exposes
/// no way to change it afterwards, and a [`tonic::transport::Channel`] fixes
/// its configuration when it connects. So a reload here does not mutate an
/// existing client: it **builds a new one and swaps which client this source
/// hands out**.
///
/// Three consequences follow, and none of them are visible from the method
/// signatures alone.
///
/// ## Call [`client()`](Self::client) per request; never cache the handle
///
/// ```rust,ignore
/// // Correct: every request reads the current client.
/// let response = source.client().get(url).send().await?;
///
/// // Wrong: this handle is pinned to the certificate that was current when it
/// // was taken, and will keep using it forever. It will never rotate.
/// let client = source.client();
/// loop { client.get(url).send().await?; }
/// ```
///
/// A cached handle is not stale in a way that produces an error. It keeps
/// working, with the old certificate, until that certificate expires.
///
/// ## A reload discards the connection pool
///
/// The new client starts with an empty pool, so every peer must reconnect and
/// re-handshake on its next request. That cost is proportional to the number of
/// peers and is paid all at once. Reloads must therefore be driven by an actual
/// rotation signal: a `SIGHUP`, an inotify watch on the certificate directory,
/// or a cert-manager hook. **Never drive them from a timer.** A periodic reload
/// spends a full reconnect storm on every tick to install credentials that are
/// almost always byte-identical to the ones already loaded.
///
/// ## gRPC channels cannot rotate at all
///
/// [`tonic_client_tls_config_snapshot`](Self::tonic_client_tls_config_snapshot)
/// is named for what it is: a point-in-time copy. Once that configuration has
/// been used to build a channel and the channel has been handed to a generated
/// client, nothing this source does can affect it. Rotating a gRPC identity
/// means rebuilding the channel and the client from a fresh snapshot, which is
/// the caller's job.
///
/// # Failure behaviour
///
/// Every source is reloadable — it is always built from a
/// [`ClientIdentityConfig`], so there is no static variant and no
/// `is_reloadable()` to check. A failed [`reload`](Self::reload) is
/// **fail-closed**: the last-good client stays installed and keeps serving, the
/// error is logged at `ERROR` level and returned. A rotation that produced an
/// unusable certificate must not leave the service unable to call its peers.
#[derive(Clone)]
pub struct ClientIdentitySource {
    inner: Arc<ClientIdentitySourceInner>,
}

struct ClientIdentitySourceInner {
    /// The client [`client`](ClientIdentitySource::client) currently hands out.
    current: ArcSwap<reqwest::Client>,
    /// The files a reload rereads. Never absent: every source is reloadable.
    origin: ClientIdentityConfig,
}

impl ClientIdentitySource {
    /// Build a source by loading the identity named by `config`.
    ///
    /// Reads and validates the certificate, key and any peer-CA bundle through
    /// [`reqwest_client_builder`], then builds the first client. Returns the
    /// load error if that fails, leaving no source rather than one that would
    /// hand out an unauthenticated client.
    ///
    /// # Errors
    ///
    /// Returns an error when the credentials cannot be loaded or when
    /// `reqwest` cannot build a client from them.
    pub fn from_config(config: &ClientIdentityConfig) -> Result<Self> {
        let client = build_client(config)?;
        Ok(Self {
            inner: Arc::new(ClientIdentitySourceInner {
                current: ArcSwap::new(Arc::new(client)),
                origin: config.clone(),
            }),
        })
    }

    /// The client to use for the request you are about to make.
    ///
    /// A cheap atomic read that does no I/O, so it belongs on the request path.
    /// Call it for **every** request. Holding on to the returned handle pins
    /// the certificate that was current when it was taken; see the type-level
    /// documentation.
    #[must_use]
    pub fn client(&self) -> Arc<reqwest::Client> {
        self.inner.current.load_full()
    }

    /// The configuration this source reloads from.
    #[must_use]
    pub fn origin(&self) -> &ClientIdentityConfig {
        &self.inner.origin
    }

    /// Reread the identity files and install a newly built client.
    ///
    /// On success, every subsequent [`client()`](Self::client) call returns the
    /// new client. Handles taken before the reload keep the previous identity
    /// and the previous connection pool for as long as they are held.
    ///
    /// Drive this from a rotation signal, not a timer: the new client's
    /// connection pool starts empty, so every peer reconnects.
    ///
    /// # Errors
    ///
    /// Returns an error when the files fail to load, parse or validate, or when
    /// `reqwest` cannot build a client from them. The previously installed
    /// client stays in place and keeps working in every such case. Failures are
    /// also logged at `ERROR` level, because a service whose rotation has
    /// silently stopped working will keep working until the certificate expires
    /// and then fail every outbound call at once.
    pub fn reload(&self) -> Result<()> {
        let origin = &self.inner.origin;
        match build_client(origin) {
            Ok(client) => {
                self.inner.current.store(Arc::new(client));
                tracing::info!(
                    cert_path = %origin.cert_path.display(),
                    key_path = %origin.key_path.display(),
                    "client identity reloaded; new requests use the new certificate \
                     and a fresh connection pool"
                );
                Ok(())
            }
            Err(e) => {
                tracing::error!(
                    cert_path = %origin.cert_path.display(),
                    key_path = %origin.key_path.display(),
                    error = %e,
                    "client identity reload failed; continuing to use the previous \
                     certificate. New credentials will not take effect until a reload \
                     succeeds."
                );
                Err(e)
            }
        }
    }

    /// A point-in-time gRPC TLS configuration built from this source's files.
    ///
    /// Named `_snapshot` because that is all it can be. A
    /// [`tonic::transport::Channel`] fixes its TLS configuration when it
    /// connects, so a channel built from this value is unaffected by any later
    /// [`reload`](Self::reload). Rotating a gRPC identity means taking a fresh
    /// snapshot, rebuilding the channel, and replacing the generated client
    /// that holds it.
    ///
    /// Reads the files directly rather than deriving from the currently
    /// installed HTTP client, so the returned configuration reflects what is on
    /// disk now, which may differ from what [`client()`](Self::client) is
    /// serving if a reload has failed since.
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
        // The installed `reqwest::Client` holds the private key, so describe
        // the source by where it came from rather than by what it holds. The
        // origin names file paths only, never their contents.
        f.debug_struct("ClientIdentitySource")
            .field("origin", &self.inner.origin)
            .finish()
    }
}

/// Load the identity from disk and build a `reqwest` client from it.
///
/// The single place both [`ClientIdentitySource::from_config`] and
/// [`ClientIdentitySource::reload`] go through, so the initial load and every
/// rotation validate exactly the same things.
fn build_client(config: &ClientIdentityConfig) -> Result<reqwest::Client> {
    reqwest_client_builder(config)?.build().map_err(|e| {
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

    #[test]
    fn successful_reload_installs_a_new_client() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = write_identity(dir.path(), &generate_cert("client"));
        let source = ClientIdentitySource::from_config(&config).expect("initial load");
        let before = source.client();

        let second = generate_cert("client");
        std::fs::write(&config.cert_path, &second.cert_pem).expect("rewrite cert");
        std::fs::write(&config.key_path, &second.key_pem).expect("rewrite key");
        source
            .reload()
            .expect("rereading valid credentials must succeed");

        assert!(
            !Arc::ptr_eq(&source.client(), &before),
            "a successful reload must install a newly built client"
        );
    }

    #[test]
    fn a_cached_client_handle_does_not_observe_a_reload() {
        // This asserts the documented footgun as behaviour, so that nobody
        // later "fixes" the documentation into a promise the type cannot keep.
        // A `reqwest::Client` bakes its TLS configuration in at `build()`, so a
        // handle taken before a rotation keeps the old certificate forever.
        let dir = tempfile::tempdir().expect("temp dir");
        let config = write_identity(dir.path(), &generate_cert("client"));
        let source = ClientIdentitySource::from_config(&config).expect("initial load");

        let cached = source.client();

        let second = generate_cert("client");
        std::fs::write(&config.cert_path, &second.cert_pem).expect("rewrite cert");
        std::fs::write(&config.key_path, &second.key_pem).expect("rewrite key");
        source.reload().expect("reload");

        assert!(
            !Arc::ptr_eq(&cached, &source.client()),
            "the cached handle must still be the pre-rotation client: callers who \
             hold one never rotate, which is why `client()` must be called per request"
        );
    }

    #[test]
    fn failed_reload_preserves_the_last_good_client() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = write_identity(dir.path(), &generate_cert("client"));
        let source = ClientIdentitySource::from_config(&config).expect("initial load");
        let last_good = source.client();

        // Simulate a half-written rotation: the cert file is no longer parseable.
        std::fs::write(&config.cert_path, "-----BEGIN CERTIFICATE-----\ntruncated")
            .expect("corrupt cert");

        let err = source
            .reload()
            .expect_err("an unparseable certificate must fail the reload");

        assert!(
            Arc::ptr_eq(&source.client(), &last_good),
            "a failed reload must keep the last-good client, not drop it"
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
        assert!(
            !Arc::ptr_eq(&source.client(), &last_good),
            "the recovered reload must install the new client"
        );
    }

    #[test]
    fn clones_of_a_source_observe_the_same_reload() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = write_identity(dir.path(), &generate_cert("client"));
        let source = ClientIdentitySource::from_config(&config).expect("initial load");
        let caller_side = source.clone();
        let before = caller_side.client();

        let second = generate_cert("client");
        std::fs::write(&config.cert_path, &second.cert_pem).expect("rewrite cert");
        std::fs::write(&config.key_path, &second.key_pem).expect("rewrite key");
        source.reload().expect("reload");

        assert!(
            Arc::ptr_eq(&caller_side.client(), &source.client()),
            "every clone must hand out the same installed client"
        );
        assert!(
            !Arc::ptr_eq(&caller_side.client(), &before),
            "the clone held by the caller must see the rotation"
        );
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
}
