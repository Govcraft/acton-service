//! TLS support using rustls
//!
//! Provides a [`TlsListener`] that wraps a TCP listener with TLS termination,
//! implementing [`axum::serve::Listener`] for seamless integration with axum's server.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::server::TlsStream;
use tokio_rustls::TlsAcceptor;

use crate::config::TlsConfig;
use crate::error::Result;

/// A TLS-enabled listener wrapping a [`TcpListener`] with a [`TlsAcceptor`].
///
/// Implements [`axum::serve::Listener`] so it can be used as a drop-in
/// replacement for `TcpListener` when calling `axum::serve()`.
pub struct TlsListener {
    tcp: TcpListener,
    acceptor: TlsAcceptor,
}

impl TlsListener {
    /// Create a new TLS listener from an existing TCP listener and server config.
    pub fn new(tcp: TcpListener, server_config: Arc<ServerConfig>) -> Self {
        Self {
            tcp,
            acceptor: TlsAcceptor::from(server_config),
        }
    }
}

impl axum::serve::Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    fn accept(&mut self) -> impl std::future::Future<Output = (Self::Io, Self::Addr)> + Send {
        let acceptor = self.acceptor.clone();
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

/// Load a rustls [`ServerConfig`] from PEM certificate and key files.
///
/// Reads the certificate chain and private key from disk and constructs
/// a server configuration with no client authentication required.
pub fn load_server_config(tls_config: &TlsConfig) -> Result<Arc<ServerConfig>> {
    use rustls_pki_types::pem::PemObject;
    use rustls_pki_types::{CertificateDer, PrivateKeyDer};
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

    // Build server config
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)
        .map_err(|e| {
            crate::error::Error::Internal(format!("Failed to build TLS server config: {}", e))
        })?;

    Ok(Arc::new(config))
}
