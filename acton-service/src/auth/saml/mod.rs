//! SAML 2.0 service provider (requires the `saml` feature).
//!
//! Federates a service with identity providers that speak only SAML 2.0
//! (ADFS, Shibboleth, Okta/Entra in SAML mode). The service is the SP: it
//! issues signed `AuthnRequest`s over HTTP-Redirect or HTTP-POST, consumes
//! the identity provider's response at its Assertion Consumer Service, and
//! maps the validated assertion onto the same [`Claims`] every other
//! authentication path produces, so `PasetoAuth`, Cedar policies, and
//! application code stay unchanged.
//!
//! Validation is strict by default: assertions must be signed, responses
//! must be signed when assertions are CBC-encrypted, `AudienceRestriction`,
//! `InResponseTo`, `Destination`, `Recipient`, `Conditions`, and session
//! windows are all checked, and accepted identifiers are recorded in a
//! [`SamlReplayStore`] until they would have expired. IdP-initiated
//! (unsolicited) SSO is not accepted.
//!
//! XML signature and encryption run on aws-lc-rs on Linux x86_64/aarch64
//! (the only targets its backend supports, so this holds under `crypto-ring`
//! too) and on the pure-Rust RustCrypto provider everywhere else. No native
//! `xmlsec` or OpenSSL is involved. On RustCrypto, decrypting
//! `EncryptedAssertion`s requires the explicit
//! [`allow_software_rsa_decryption`](SamlConfig::allow_software_rsa_decryption)
//! opt-in.
//!
//! # Example
//!
//! ```rust,ignore
//! use acton_service::auth::saml::{SamlLoginDelivery, SamlServiceProvider};
//!
//! let sp = SamlServiceProvider::from_config_in_memory(&config.auth.saml)?;
//!
//! // GET /saml/metadata
//! let metadata = sp.metadata_xml();
//!
//! // GET /saml/login
//! match sp.begin_login(Some("/dashboard")).await?.delivery {
//!     SamlLoginDelivery::Redirect { url } => { /* 302 to url */ }
//!     SamlLoginDelivery::PostForm { action, fields } => { /* render form */ }
//! }
//!
//! // POST /saml/acs with the decoded form body
//! let login = sp.finish_login(form_fields).await?;
//! let token = paseto.generate_token(&login.claims)?;
//! ```
//!
//! [`Claims`]: crate::middleware::Claims

pub mod config;
pub mod sp;
pub mod store;

pub use config::{
    SamlAttributeMapping, SamlConfig, SamlConfigError, SamlIdpConfig, SamlNameIdFormat,
    SamlRequestBinding,
};
pub use sp::{
    SamlAuthError, SamlLogin, SamlLoginDelivery, SamlLoginRequest, SamlRejection,
    SamlServiceProvider,
};
pub use store::{
    InMemorySamlStore, PendingLogin, PendingRelayState, SamlPendingStore, SamlReplayStore,
};

#[cfg(feature = "cache")]
pub use store::RedisSamlStore;
