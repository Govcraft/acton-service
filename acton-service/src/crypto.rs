//! Rustls crypto provider bootstrap.
//!
//! `rustls 0.23+` requires a process-wide default [`CryptoProvider`] to be
//! installed before `ServerConfig::builder()` or `ClientConfig::builder()` is
//! called. When more than one provider is compiled into the binary (which
//! happens routinely via transitive deps such as `quinn-proto` and
//! `jsonwebtoken`), the implicit default is ambiguous and the builder panics.
//!
//! [`ensure_default_crypto_provider`] installs the provider chosen at compile
//! time via the `crypto-aws-lc-rs` (default) or `crypto-ring` feature. It is
//! idempotent and safe to call from multiple places.
//!
//! [`CryptoProvider`]: rustls::crypto::CryptoProvider

use std::sync::Once;

static INIT: Once = Once::new();

/// Install the rustls default [`CryptoProvider`] selected at compile time.
///
/// Called automatically by [`crate::tls::load_server_config`]. Binaries that
/// drive `reqwest`, `sqlx`, or `tonic` TLS clients without going through the
/// framework's TLS listener should call this from `main` before issuing any
/// TLS request.
///
/// Idempotent: subsequent calls are no-ops.
///
/// [`CryptoProvider`]: rustls::crypto::CryptoProvider
pub fn ensure_default_crypto_provider() {
    INIT.call_once(|| {
        #[cfg(feature = "crypto-aws-lc-rs")]
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        #[cfg(feature = "crypto-ring")]
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}
