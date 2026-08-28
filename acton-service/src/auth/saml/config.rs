//! Configuration for the SAML 2.0 service provider (`[auth.saml]`).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const DEFAULT_CLOCK_SKEW_SECS: u64 = 60;
const DEFAULT_PENDING_TTL_SECS: u64 = 600;

/// SAML 2.0 service-provider configuration.
///
/// Key and certificate material is read from PEM files at startup, mirroring
/// the `[tls]` section. The identity provider is described by its metadata
/// document, which is pinned to an expected `entityID` and, when
/// `metadata_signing_certificate_path` is set, to a signing certificate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SamlConfig {
    /// This service's SAML `entityID`, conventionally its metadata URL.
    pub entity_id: String,
    /// Assertion Consumer Service URL the identity provider posts to
    /// (HTTP-POST binding).
    pub acs_url: String,
    /// PEM private key used to sign `AuthnRequest`s and advertised in metadata.
    pub signing_key_path: PathBuf,
    /// PEM certificate matching `signing_key_path`.
    pub certificate_path: PathBuf,
    /// Optional PEM private key for decrypting `EncryptedAssertion`s. When
    /// set, `certificate_path` is also advertised as the encryption certificate
    /// and the identity provider is expected to encrypt assertions.
    #[serde(default)]
    pub decryption_key_path: Option<PathBuf>,
    /// Permit software RSA key-transport decryption on the RustCrypto backend,
    /// which every target other than Linux x86_64/aarch64 uses. RustCrypto's
    /// RSA decryption is subject to RUSTSEC-2023-0071 (Marvin timing side
    /// channel), so it is off unless explicitly accepted. The aws-lc-rs
    /// backend used on Linux does not need this.
    #[serde(default)]
    pub allow_software_rsa_decryption: bool,
    /// Binding used to deliver the `AuthnRequest` to the identity provider.
    #[serde(default)]
    pub request_binding: SamlRequestBinding,
    /// Sign outgoing `AuthnRequest`s (default `true`). This must agree with the
    /// identity provider's `WantAuthnRequestsSigned` metadata flag; the SP
    /// refuses to start a login when the two disagree.
    #[serde(default = "default_true")]
    pub sign_authn_requests: bool,
    /// `NameID` format requested from the identity provider.
    #[serde(default)]
    pub name_id_format: SamlNameIdFormat,
    /// Clock skew tolerated on assertion validity windows, in seconds
    /// (default 60).
    #[serde(default = "default_clock_skew_secs")]
    pub clock_skew_secs: u64,
    /// How long a started login waits for its response before the
    /// `AuthnRequest` is forgotten, in seconds (default 600).
    #[serde(default = "default_pending_ttl_secs")]
    pub pending_request_ttl_secs: u64,
    /// The identity provider this service federates with.
    pub idp: SamlIdpConfig,
    /// How assertion attributes become [`Claims`](crate::middleware::Claims).
    #[serde(default)]
    pub attributes: SamlAttributeMapping,
}

/// Identity-provider trust anchors.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SamlIdpConfig {
    /// The identity provider's `entityID`. Metadata whose `entityID` differs
    /// is rejected at startup, and every response's `Issuer` must match.
    pub entity_id: String,
    /// Path to the identity provider's SAML metadata XML.
    pub metadata_path: PathBuf,
    /// PEM certificate the metadata document must be signed with. Without it
    /// the metadata is accepted unsigned, and the file itself becomes the
    /// trust anchor: protect it accordingly.
    #[serde(default)]
    pub metadata_signing_certificate_path: Option<PathBuf>,
}

/// Binding used for the outgoing `AuthnRequest`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SamlRequestBinding {
    /// HTTP-Redirect: the browser is sent a `302` to the IdP with a deflated
    /// request in the query string.
    #[default]
    Redirect,
    /// HTTP-POST: the browser auto-submits a form carrying the request.
    Post,
}

/// `NameID` formats the SP can request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SamlNameIdFormat {
    /// `urn:oasis:names:tc:SAML:1.1:nameid-format:unspecified`
    #[default]
    Unspecified,
    /// `urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress`
    EmailAddress,
    /// `urn:oasis:names:tc:SAML:2.0:nameid-format:persistent`
    Persistent,
    /// `urn:oasis:names:tc:SAML:2.0:nameid-format:transient`
    Transient,
    /// `urn:oasis:names:tc:SAML:1.1:nameid-format:WindowsDomainQualifiedName`
    WindowsDomainQualifiedName,
}

/// Attribute-to-claim mapping.
///
/// Attribute names are matched exactly against the assertion's
/// `Attribute/@Name`, so ADFS-style claim URIs
/// (`http://schemas.xmlsoap.org/ws/2005/05/identity/claims/emailaddress`)
/// work as written.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SamlAttributeMapping {
    /// Attribute carrying the user's email. Falls back to the `NameID` when
    /// its format is `emailAddress`.
    #[serde(default)]
    pub email: Option<String>,
    /// Attribute carrying the username. Falls back to the `NameID` value.
    #[serde(default)]
    pub username: Option<String>,
    /// Multi-valued attribute carrying group memberships.
    #[serde(default)]
    pub groups: Option<String>,
    /// Exact group-value-to-application-role mappings.
    #[serde(default)]
    pub group_roles: BTreeMap<String, String>,
    /// Roles granted to every SAML-authenticated user.
    #[serde(default)]
    pub default_roles: Vec<String>,
}

impl SamlConfig {
    /// Check the configuration for problems that do not require reading any
    /// file: empty identifiers, non-HTTP endpoints, malformed role names.
    ///
    /// # Errors
    ///
    /// Returns [`SamlConfigError::Invalid`] naming the offending field.
    pub fn validate(&self) -> Result<(), SamlConfigError> {
        non_empty("entity_id", &self.entity_id)?;
        non_empty("idp.entity_id", &self.idp.entity_id)?;
        http_url("acs_url", &self.acs_url)?;
        if self.pending_request_ttl_secs == 0 {
            return Err(SamlConfigError::Invalid(
                "pending_request_ttl_secs must be greater than zero".to_owned(),
            ));
        }
        for role in self
            .attributes
            .group_roles
            .values()
            .chain(self.attributes.default_roles.iter())
        {
            if role.is_empty() || role.chars().any(char::is_whitespace) {
                return Err(SamlConfigError::Invalid(format!(
                    "role '{role}' must be non-empty and contain no whitespace"
                )));
            }
        }
        Ok(())
    }
}

fn non_empty(field: &'static str, value: &str) -> Result<(), SamlConfigError> {
    if value.trim().is_empty() {
        return Err(SamlConfigError::Invalid(format!(
            "{field} must not be empty"
        )));
    }
    Ok(())
}

fn http_url(field: &'static str, value: &str) -> Result<(), SamlConfigError> {
    non_empty(field, value)?;
    if !(value.starts_with("https://") || value.starts_with("http://")) {
        return Err(SamlConfigError::Invalid(format!(
            "{field} '{value}' must be an absolute http(s) URL"
        )));
    }
    Ok(())
}

fn default_true() -> bool {
    true
}

fn default_clock_skew_secs() -> u64 {
    DEFAULT_CLOCK_SKEW_SECS
}

fn default_pending_ttl_secs() -> u64 {
    DEFAULT_PENDING_TTL_SECS
}

/// The `[auth.saml]` section could not be turned into a service provider.
#[derive(Debug, thiserror::Error)]
pub enum SamlConfigError {
    /// A field has an unusable value.
    #[error("[auth.saml] {0}")]
    Invalid(String),
    /// A referenced file could not be read.
    #[error("[auth.saml] cannot read {path}: {source}")]
    Io {
        /// File that failed to load.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The SAML library rejected the material or the metadata.
    #[error("[auth.saml] {context}: {detail}")]
    Saml {
        /// What was being built when the error occurred.
        context: &'static str,
        /// The library's description of the problem.
        detail: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SamlConfig {
        toml::from_str(
            r#"
            entity_id = "https://sp.example.test/saml/metadata"
            acs_url = "https://sp.example.test/saml/acs"
            signing_key_path = "sp.key.pem"
            certificate_path = "sp.cert.pem"
            [idp]
            entity_id = "https://idp.example.test/metadata"
            metadata_path = "idp.xml"
            [attributes]
            groups = "memberOf"
            [attributes.group_roles]
            "CN=Admins" = "admin"
            "#,
        )
        .expect("sample config parses")
    }

    #[test]
    fn defaults_are_applied() {
        let config = sample();
        assert_eq!(config.request_binding, SamlRequestBinding::Redirect);
        assert_eq!(config.name_id_format, SamlNameIdFormat::Unspecified);
        assert!(config.sign_authn_requests);
        assert_eq!(config.clock_skew_secs, DEFAULT_CLOCK_SKEW_SECS);
        assert_eq!(config.pending_request_ttl_secs, DEFAULT_PENDING_TTL_SECS);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn rejects_non_http_acs() {
        let mut config = sample();
        config.acs_url = "sp.example.test/acs".to_owned();
        assert!(matches!(
            config.validate(),
            Err(SamlConfigError::Invalid(_))
        ));
    }

    #[test]
    fn rejects_roles_with_whitespace() {
        let mut config = sample();
        config.attributes.default_roles = vec!["read only".to_owned()];
        assert!(matches!(
            config.validate(),
            Err(SamlConfigError::Invalid(_))
        ));
    }

    #[test]
    fn unknown_fields_are_refused() {
        let result: Result<SamlConfig, _> = toml::from_str(
            r#"
            entity_id = "a"
            acs_url = "https://a/acs"
            signing_key_path = "k"
            certificate_path = "c"
            typo = true
            [idp]
            entity_id = "b"
            metadata_path = "m"
            "#,
        );
        assert!(result.is_err());
    }
}
