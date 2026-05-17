//! Regression test for the rustls crypto provider bootstrap.
//!
//! Without [`acton_service::crypto::ensure_default_crypto_provider`],
//! `ServerConfig::builder()` panics whenever more than one provider is
//! compiled into the binary (which happens routinely via transitive deps
//! like `quinn-proto` and `jsonwebtoken`).

#![cfg(feature = "tls")]

use acton_service::crypto::ensure_default_crypto_provider;
use tokio_rustls::rustls::ServerConfig;

#[test]
fn server_config_builder_does_not_panic_after_provider_install() {
    ensure_default_crypto_provider();
    // Idempotent — second call must not panic either.
    ensure_default_crypto_provider();

    let _builder = ServerConfig::builder().with_no_client_auth();
}
