//! The service provider: starts logins, consumes responses, yields `Claims`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, SecondsFormat, Utc};
use saml_rs::error::SubjectConfirmationReason;
use saml_rs::{
    AcsEndpoint, AuthnRequest, AuthnRequestSigningPolicy, BrowserInput, CertificatePem, ClockSkew,
    Credentials, EntityId, FormField, IdpDescriptor, MetadataTrustPolicy, NameIdFormat,
    PendingAuthnRequest, PendingSnapshot, PrivateKeyPem, RelayStateParam, ReplayCache, ReplayKey,
    ReplayPolicy, Saml, SamlError, SamlInstant, SamlValidationContext, Sp, SpConfig,
    SpValidationPolicy, SsoResponse, SsoSession, StartSso, XmlEncryptionPolicy, XmlPolicy,
};

use super::config::{
    SamlAttributeMapping, SamlConfig, SamlConfigError, SamlNameIdFormat, SamlRequestBinding,
};
use super::store::{
    InMemorySamlStore, PendingLogin, PendingRelayState, SamlPendingStore, SamlReplayStore,
};
use crate::error::Error;
use crate::middleware::token::Claims;

/// Claims issued from an assertion that carries no expiry of its own live
/// this long. Token issuance applies its own, usually shorter, lifetime.
const FALLBACK_CLAIMS_LIFETIME: Duration = Duration::from_secs(3600);

/// A configured SAML 2.0 service provider bound to one identity provider.
///
/// The provider is `Send + Sync` and meant to be built once at startup and
/// shared through application state. The two HTTP handlers a service needs
/// map onto [`begin_login`](Self::begin_login) and
/// [`finish_login`](Self::finish_login); [`metadata_xml`](Self::metadata_xml)
/// serves the document the identity provider imports.
pub struct SamlServiceProvider {
    sp: Saml<Sp>,
    idp: IdpDescriptor,
    request_binding: SamlRequestBinding,
    clock_skew: Duration,
    pending_ttl: Duration,
    mapping: SamlAttributeMapping,
    pending: Arc<dyn SamlPendingStore>,
    replay: Arc<dyn SamlReplayStore>,
}

impl std::fmt::Debug for SamlServiceProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SamlServiceProvider")
            .field("idp", self.idp.entity_id())
            .field("request_binding", &self.request_binding)
            .finish_non_exhaustive()
    }
}

/// What the browser must do to reach the identity provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SamlLoginRequest {
    /// The `AuthnRequest/@ID`, useful for logging and for pairing a cookie
    /// with the pending login.
    pub request_id: String,
    /// How to deliver the request.
    pub delivery: SamlLoginDelivery,
}

/// Transport for a started login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SamlLoginDelivery {
    /// Respond with `302 Location: url`.
    Redirect {
        /// Fully formed IdP URL carrying `SAMLRequest`, `RelayState`, and
        /// the detached signature parameters when signing is on.
        url: String,
    },
    /// Render an auto-submitting HTML form.
    PostForm {
        /// Form `action`.
        action: String,
        /// Hidden `name`/`value` pairs, in order.
        fields: Vec<(String, String)>,
    },
}

/// A successfully validated login.
#[derive(Debug, Clone)]
pub struct SamlLogin {
    /// Claims ready for [`PasetoGenerator`](crate::auth::PasetoGenerator) or
    /// direct insertion into request extensions.
    pub claims: Claims,
    /// The asserted `NameID`.
    pub name_id: String,
    /// The `NameID/@Format`, when present.
    pub name_id_format: Option<String>,
    /// The identity provider's `entityID`.
    pub issuer: String,
    /// `AuthnStatement/@SessionIndex`, needed for single logout.
    pub session_index: Option<String>,
    /// Every attribute the assertion carried, in document order per name.
    pub attributes: BTreeMap<String, Vec<String>>,
    /// The `RelayState` echoed by the identity provider.
    pub relay_state: Option<String>,
}

/// Why a login could not be completed.
#[derive(Debug, thiserror::Error)]
pub enum SamlAuthError {
    /// The response failed validation. Never accept the identity.
    #[error("SAML response rejected ({reason}): {detail}")]
    Rejected {
        /// Coarse classification for metrics, logs, and tests.
        reason: SamlRejection,
        /// The validator's description.
        detail: String,
    },
    /// No pending login matches the response's `InResponseTo`. Either the
    /// request expired, was already consumed, or the response is unsolicited.
    #[error("no pending SAML login matches request '{0}'")]
    UnknownRequest(String),
    /// A store operation failed.
    #[error("SAML state store failure: {0}")]
    Store(String),
    /// The request could not be built, typically a `RelayState` over the
    /// 80-byte limit or a metadata conflict with the identity provider.
    #[error("SAML protocol failure: {0}")]
    Protocol(String),
}

/// Coarse reason a response was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SamlRejection {
    /// Signature missing, unverifiable, or not covering the assertion.
    InvalidSignature,
    /// Outside `NotBefore`/`NotOnOrAfter` or session validity, after skew.
    Expired,
    /// `AudienceRestriction` does not name this service.
    AudienceMismatch,
    /// Response or assertion identifier already accepted.
    Replayed,
    /// `Issuer`, `InResponseTo`, `Destination`, or `RelayState` does not match
    /// the pending login.
    RequestMismatch,
    /// Anything else: malformed XML, non-success status, profile violations.
    Other,
}

impl std::fmt::Display for SamlRejection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidSignature => "invalid signature",
            Self::Expired => "expired",
            Self::AudienceMismatch => "audience mismatch",
            Self::Replayed => "replayed",
            Self::RequestMismatch => "request mismatch",
            Self::Other => "invalid",
        })
    }
}

impl From<SamlAuthError> for Error {
    fn from(error: SamlAuthError) -> Self {
        match error {
            SamlAuthError::Rejected { .. } | SamlAuthError::UnknownRequest(_) => {
                Error::Unauthorized(error.to_string())
            }
            SamlAuthError::Store(detail) => Error::Internal(detail),
            SamlAuthError::Protocol(detail) => Error::BadRequest(detail),
        }
    }
}

impl SamlServiceProvider {
    /// Build a provider from configuration with single-process state.
    ///
    /// # Errors
    ///
    /// See [`from_config`](Self::from_config).
    pub fn from_config_in_memory(config: &SamlConfig) -> Result<Self, SamlConfigError> {
        let store = Arc::new(InMemorySamlStore::new());
        Self::from_config(config, store.clone(), store)
    }

    /// Build a provider from configuration, reading key, certificate, and
    /// metadata files, with the given stores.
    ///
    /// # Errors
    ///
    /// Returns a [`SamlConfigError`] when a field is invalid, a file cannot
    /// be read, the key material is unusable, or the identity provider
    /// metadata fails its `entityID` or signature check.
    pub fn from_config(
        config: &SamlConfig,
        pending: Arc<dyn SamlPendingStore>,
        replay: Arc<dyn SamlReplayStore>,
    ) -> Result<Self, SamlConfigError> {
        config.validate()?;
        saml_rs::initialize_crypto_provider()
            .map_err(|error| saml_error("crypto provider", &error))?;

        let signing_key = read(&config.signing_key_path)?;
        let certificate = read(&config.certificate_path)?;
        let decryption_key = config
            .decryption_key_path
            .as_deref()
            .map(read)
            .transpose()?;
        let idp_metadata = read(&config.idp.metadata_path)?;
        let metadata_signing_certificate = config
            .idp
            .metadata_signing_certificate_path
            .as_deref()
            .map(read)
            .transpose()?;

        let sp = build_sp(config, signing_key, certificate, decryption_key)?;
        let idp = load_idp(
            config,
            &idp_metadata,
            metadata_signing_certificate.as_deref(),
        )?;

        Ok(Self {
            sp,
            idp,
            request_binding: config.request_binding,
            clock_skew: Duration::from_secs(config.clock_skew_secs),
            pending_ttl: Duration::from_secs(config.pending_request_ttl_secs),
            mapping: config.attributes.clone(),
            pending,
            replay,
        })
    }

    /// This service's SAML metadata document.
    #[must_use]
    pub fn metadata_xml(&self) -> &str {
        self.sp.metadata_xml()
    }

    /// The identity provider's `entityID`.
    #[must_use]
    pub fn idp_entity_id(&self) -> &str {
        self.idp.entity_id().as_str()
    }

    /// Start an SP-initiated login.
    ///
    /// `relay_state` is returned untouched in [`SamlLogin::relay_state`]; use
    /// it for the post-login destination, never for anything trusted.
    ///
    /// # Errors
    ///
    /// [`SamlAuthError::Protocol`] if the request cannot be built, or
    /// [`SamlAuthError::Store`] if the pending login cannot be saved.
    pub async fn begin_login(
        &self,
        relay_state: Option<&str>,
    ) -> Result<SamlLoginRequest, SamlAuthError> {
        let (login, request) = self.start(relay_state, Utc::now())?;
        self.pending
            .put(&login, self.pending_ttl)
            .await
            .map_err(|error| SamlAuthError::Store(error.to_string()))?;
        Ok(request)
    }

    fn start(
        &self,
        relay_state: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<(PendingLogin, SamlLoginRequest), SamlAuthError> {
        let relay_state = RelayStateParam::try_from_option(relay_state.map(str::to_owned))
            .map_err(|error| SamlAuthError::Protocol(error.to_string()))?;
        let options = match self.request_binding {
            SamlRequestBinding::Redirect => StartSso::redirect(),
            SamlRequestBinding::Post => StartSso::post(),
        }
        .relay_state(relay_state);

        let started = self
            .sp
            .start_sso(&self.idp, options)
            .map_err(|error| SamlAuthError::Protocol(error.to_string()))?;

        let expires =
            now + chrono::Duration::from_std(self.pending_ttl).unwrap_or(chrono::Duration::MAX);
        let pending = started
            .pending
            .with_issue_instant(
                instant(now).map_err(|error| SamlAuthError::Protocol(error.to_string()))?,
            )
            .with_expiration(
                instant(expires).map_err(|error| SamlAuthError::Protocol(error.to_string()))?,
            );
        let login = pending_login_from_snapshot(pending.snapshot());

        let delivery = match self.request_binding {
            SamlRequestBinding::Redirect => SamlLoginDelivery::Redirect {
                url: started
                    .outbound
                    .redirect_url()
                    .map_err(|error| SamlAuthError::Protocol(error.to_string()))?
                    .to_owned(),
            },
            SamlRequestBinding::Post => {
                let form = started
                    .outbound
                    .post_form()
                    .map_err(|error| SamlAuthError::Protocol(error.to_string()))?;
                SamlLoginDelivery::PostForm {
                    action: form.action().as_str().to_owned(),
                    fields: form
                        .fields()
                        .iter()
                        .map(|field| (field.name().to_owned(), field.value().to_owned()))
                        .collect(),
                }
            }
        };

        let request = SamlLoginRequest {
            request_id: login.request_id.clone(),
            delivery,
        };
        Ok((login, request))
    }

    /// Consume the form the identity provider posted to the ACS.
    ///
    /// `fields` are the decoded `application/x-www-form-urlencoded` pairs
    /// (`SAMLResponse`, optionally `RelayState`). The pending login is looked
    /// up by the response's `InResponseTo`, consumed, and every check the
    /// profile demands is applied before any identity is returned:
    /// signature, issuer, destination, recipient, `InResponseTo`, audience,
    /// validity window, and replay.
    ///
    /// # Errors
    ///
    /// [`SamlAuthError::Rejected`] with a [`SamlRejection`] naming the failed
    /// check, [`SamlAuthError::UnknownRequest`] when nothing is pending for
    /// the response, or [`SamlAuthError::Store`] on backend failure.
    pub async fn finish_login<I, K, V>(&self, fields: I) -> Result<SamlLogin, SamlAuthError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let fields: Vec<FormField> = fields
            .into_iter()
            .map(|(name, value)| FormField::new(name, value))
            .collect();
        let request_id = in_response_to(&fields)?;
        let login = self
            .pending
            .take(&request_id)
            .await
            .map_err(|error| SamlAuthError::Store(error.to_string()))?
            .ok_or_else(|| SamlAuthError::UnknownRequest(request_id.clone()))?;

        let now = SystemTime::now();
        let (session, replay_keys) = self.validate(&login, fields, now)?;

        for (key, expires_at) in replay_keys {
            let retain_for = expires_at
                .duration_since(now)
                .unwrap_or(Duration::from_secs(1))
                .max(Duration::from_secs(1));
            let fresh = self
                .replay
                .insert_if_absent(&key.cache_key(), retain_for)
                .await
                .map_err(|error| SamlAuthError::Store(error.to_string()))?;
            if !fresh {
                return Err(SamlAuthError::Rejected {
                    reason: SamlRejection::Replayed,
                    detail: format!("{} {} was already accepted", key.kind(), key.value()),
                });
            }
        }

        Ok(self.login_from_session(&session, &login, now))
    }

    /// Everything synchronous: validation context is not `Send`, so it is
    /// built and dropped here without crossing an await point.
    fn validate(
        &self,
        login: &PendingLogin,
        fields: Vec<FormField>,
        now: SystemTime,
    ) -> Result<(SsoSession, Vec<(ReplayKey, SystemTime)>), SamlAuthError> {
        let pending = PendingAuthnRequest::from_snapshot(snapshot_from_pending_login(login))
            .map_err(|error| SamlAuthError::Protocol(error.to_string()))?;
        let skew_ms = i64::try_from(self.clock_skew.as_millis()).unwrap_or(i64::MAX);
        let mut recorder = RecordingReplayCache::default();
        let context = SamlValidationContext::new(now, ReplayPolicy::RequireCache(&mut recorder))
            .with_clock_skew(ClockSkew::from_millis(-skew_ms, skew_ms));

        let session = self
            .sp
            .finish_sso(
                &self.idp,
                &pending,
                BrowserInput::<SsoResponse>::post(fields),
                context,
            )
            .map_err(reject)?;
        Ok((session, recorder.keys))
    }

    fn login_from_session(
        &self,
        session: &SsoSession,
        login: &PendingLogin,
        now: SystemTime,
    ) -> SamlLogin {
        let attributes = collect_attributes(session);
        let name_id = session.name_id();
        let name_id_format = name_id.format().map(|format| format.as_uri().to_owned());
        let session_index = session
            .authn_session()
            .session_index()
            .map(|index| index.as_str().to_owned());
        let expires_at = session
            .authn_session()
            .not_on_or_after()
            .or_else(|| session.not_on_or_after())
            .and_then(|instant| parse_instant(instant.as_str()));

        let claims = claims_for(
            &self.mapping,
            &AssertedIdentity {
                name_id: name_id.value(),
                name_id_format: name_id_format.as_deref(),
                issuer: session.issuer().as_str(),
                session_index: session_index.as_deref(),
                attributes: &attributes,
            },
            DateTime::<Utc>::from(now),
            expires_at,
        );

        SamlLogin {
            claims,
            name_id: name_id.value().to_owned(),
            name_id_format,
            issuer: session.issuer().as_str().to_owned(),
            session_index,
            attributes,
            relay_state: match &login.relay_state {
                PendingRelayState::Value(value) => Some(value.clone()),
                PendingRelayState::Absent | PendingRelayState::Empty => None,
            },
        }
    }
}

/// Collects the replay keys the validator wants stored so they can be
/// written to the shared store asynchronously afterwards.
#[derive(Default)]
struct RecordingReplayCache {
    keys: Vec<(ReplayKey, SystemTime)>,
}

impl ReplayCache for RecordingReplayCache {
    fn check_and_store(&mut self, key: ReplayKey, expires_at: SystemTime) -> Result<(), SamlError> {
        self.keys.push((key, expires_at));
        Ok(())
    }
}

fn build_sp(
    config: &SamlConfig,
    signing_key: String,
    certificate: String,
    decryption_key: Option<String>,
) -> Result<Saml<Sp>, SamlConfigError> {
    let entity_id = EntityId::try_new(config.entity_id.as_str())
        .map_err(|error| saml_error("entity_id", &error))?;
    let acs = AcsEndpoint::post(config.acs_url.as_str())
        .map_err(|error| saml_error("acs_url", &error))?
        .mark_default();

    let mut validation = SpValidationPolicy::strict();
    if !config.sign_authn_requests {
        validation.authn_requests = AuthnRequestSigningPolicy::DoNotSignForCompatibility;
    }

    let mut xml = XmlPolicy::default();
    let mut credentials = Credentials {
        signing_key: Some(PrivateKeyPem::new(signing_key)),
        signing_certificate: Some(CertificatePem::new(certificate.clone())),
        ..Credentials::default()
    };
    if let Some(decryption_key) = decryption_key {
        credentials.encryption_certificate = Some(CertificatePem::new(certificate));
        credentials.decryption_key = Some(PrivateKeyPem::new(decryption_key));
        xml.encryption = encryption_policy(config)?;
    }

    let sp_config = SpConfig::builder(entity_id)
        .acs_endpoint(acs)
        .name_id_format(name_id_format(config.name_id_format))
        .credentials(credentials)
        .validation(validation)
        .xml(xml)
        .build()
        .map_err(|error| saml_error("service provider configuration", &error))?;

    Saml::sp(sp_config).map_err(|error| saml_error("service provider", &error))
}

#[cfg(feature = "crypto-ring")]
fn encryption_policy(config: &SamlConfig) -> Result<XmlEncryptionPolicy, SamlConfigError> {
    if !config.allow_software_rsa_decryption {
        return Err(SamlConfigError::Invalid(
            "decryption_key_path on a crypto-ring build uses RustCrypto RSA, which is subject to \
             RUSTSEC-2023-0071; set allow_software_rsa_decryption = true to accept that, or build \
             with crypto-aws-lc-rs"
                .to_owned(),
        ));
    }
    Ok(XmlEncryptionPolicy::encrypt_assertions()
        .with_insecure_software_rsa_key_transport_decryption_allowed())
}

#[cfg(not(feature = "crypto-ring"))]
fn encryption_policy(_config: &SamlConfig) -> Result<XmlEncryptionPolicy, SamlConfigError> {
    Ok(XmlEncryptionPolicy::encrypt_assertions())
}

fn load_idp(
    config: &SamlConfig,
    metadata_xml: &str,
    signing_certificate: Option<&str>,
) -> Result<IdpDescriptor, SamlConfigError> {
    let entity_id = EntityId::try_new(config.idp.entity_id.as_str())
        .map_err(|error| saml_error("idp.entity_id", &error))?;
    let pinned: Vec<CertificatePem> = signing_certificate
        .map(CertificatePem::new)
        .into_iter()
        .collect();
    let trust = if pinned.is_empty() {
        tracing::warn!(
            path = %config.idp.metadata_path.display(),
            "[auth.saml] identity provider metadata is accepted unsigned; the file is the trust anchor"
        );
        MetadataTrustPolicy::UnsignedForCompatibility
    } else {
        MetadataTrustPolicy::RequireSignature {
            trusted_certificates: &pinned,
        }
    };
    IdpDescriptor::from_metadata_xml_for(entity_id, metadata_xml, trust)
        .map_err(|error| saml_error("idp.metadata_path", &error))
}

fn name_id_format(format: SamlNameIdFormat) -> NameIdFormat {
    match format {
        SamlNameIdFormat::Unspecified => NameIdFormat::Unspecified,
        SamlNameIdFormat::EmailAddress => NameIdFormat::EmailAddress,
        SamlNameIdFormat::Persistent => NameIdFormat::Persistent,
        SamlNameIdFormat::Transient => NameIdFormat::Transient,
        SamlNameIdFormat::WindowsDomainQualifiedName => NameIdFormat::WindowsDomainQualifiedName,
    }
}

fn read(path: &Path) -> Result<String, SamlConfigError> {
    std::fs::read_to_string(path).map_err(|source| SamlConfigError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn saml_error(context: &'static str, error: &SamlError) -> SamlConfigError {
    SamlConfigError::Saml {
        context,
        detail: error.to_string(),
    }
}

fn instant(at: DateTime<Utc>) -> Result<SamlInstant, SamlError> {
    SamlInstant::try_new(at.to_rfc3339_opts(SecondsFormat::Secs, true))
}

fn parse_instant(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

fn pending_login_from_snapshot(snapshot: PendingSnapshot<AuthnRequest>) -> PendingLogin {
    PendingLogin {
        request_id: snapshot.id,
        relay_state: match snapshot.relay_state {
            RelayStateParam::Absent => PendingRelayState::Absent,
            RelayStateParam::PresentEmpty => PendingRelayState::Empty,
            RelayStateParam::PresentValue(value) => {
                PendingRelayState::Value(value.as_str().to_owned())
            }
        },
        idp_entity_id: snapshot.peer_entity_id,
        response_binding: snapshot.expected_binding,
        request_binding: snapshot.request_binding,
        acs_url: snapshot.acs_url,
        acs_binding: snapshot.acs_binding,
        acs_index: snapshot.acs_index,
        acs_is_default: snapshot.acs_is_default,
        issued_at: snapshot.issued_at.map(|at| at.as_str().to_owned()),
        expires_at: snapshot.expires_at.map(|at| at.as_str().to_owned()),
    }
}

fn snapshot_from_pending_login(login: &PendingLogin) -> PendingSnapshot<AuthnRequest> {
    let relay_state = match &login.relay_state {
        PendingRelayState::Absent => RelayStateParam::Absent,
        PendingRelayState::Empty => RelayStateParam::PresentEmpty,
        PendingRelayState::Value(value) => {
            RelayStateParam::try_from_option(Some(value.clone())).unwrap_or(RelayStateParam::Absent)
        }
    };
    let mut snapshot = PendingSnapshot::<AuthnRequest>::authn_request(
        login.request_id.clone(),
        relay_state,
        login.idp_entity_id.clone(),
        login.response_binding.clone(),
        login.acs_url.clone(),
        login.acs_binding.clone(),
    );
    snapshot.request_binding = login.request_binding.clone();
    snapshot.acs_index = login.acs_index;
    snapshot.acs_is_default = login.acs_is_default;
    snapshot.issued_at = login
        .issued_at
        .as_deref()
        .and_then(|at| SamlInstant::try_new(at).ok());
    snapshot.expires_at = login
        .expires_at
        .as_deref()
        .and_then(|at| SamlInstant::try_new(at).ok());
    snapshot
}

/// Pull `InResponseTo` out of the posted response *before* validation.
///
/// This is an unauthenticated peek used only to find the pending login; the
/// validator re-checks the value against the pending request afterwards, so a
/// forged `InResponseTo` buys an attacker nothing but a lookup.
fn in_response_to(fields: &[FormField]) -> Result<String, SamlAuthError> {
    let encoded = fields
        .iter()
        .find(|field| field.name() == "SAMLResponse")
        .map(FormField::value)
        .ok_or_else(|| reject_other("form has no SAMLResponse field"))?;
    let bytes = saml_rs::binding::base64_decode(encoded)
        .map_err(|error| reject_other(&error.to_string()))?;
    let xml = String::from_utf8(bytes).map_err(|_| reject_other("SAMLResponse is not UTF-8"))?;
    extract_in_response_to(&xml).ok_or_else(|| SamlAuthError::Rejected {
        reason: SamlRejection::RequestMismatch,
        detail: "response carries no InResponseTo; unsolicited (IdP-initiated) SSO is not accepted"
            .to_owned(),
    })
}

/// First `InResponseTo="…"` attribute in the document, which on a
/// well-formed response is the one on the root `Response` element.
fn extract_in_response_to(xml: &str) -> Option<String> {
    let start = xml.find("InResponseTo=")? + "InResponseTo=".len();
    let rest = &xml[start..];
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let rest = &rest[1..];
    let end = rest.find(quote)?;
    let value = &rest[..end];
    (!value.is_empty()).then(|| value.to_owned())
}

fn reject_other(detail: &str) -> SamlAuthError {
    SamlAuthError::Rejected {
        reason: SamlRejection::Other,
        detail: detail.to_owned(),
    }
}

fn reject(error: SamlError) -> SamlAuthError {
    SamlAuthError::Rejected {
        reason: classify(&error),
        detail: error.to_string(),
    }
}

/// Map the library's fine-grained error onto the coarse rejection classes.
fn classify(error: &SamlError) -> SamlRejection {
    match error {
        SamlError::SignatureVerification { .. }
        | SamlError::SignatureMissing
        | SamlError::SignedReferenceMismatch
        | SamlError::AssertionSignatureRequired
        | SamlError::PotentialWrappingAttack
        | SamlError::CertificateMismatch
        | SamlError::NoTrustedCertificate => SamlRejection::InvalidSignature,
        SamlError::TimeWindowInvalid { .. }
        | SamlError::SubjectConfirmationInvalid {
            reason: SubjectConfirmationReason::TimeWindowInvalid,
        } => SamlRejection::Expired,
        SamlError::AudienceMismatch { .. } => SamlRejection::AudienceMismatch,
        SamlError::ReplayDetected { .. } => SamlRejection::Replayed,
        SamlError::IssuerMismatch { .. }
        | SamlError::InResponseToMismatch { .. }
        | SamlError::DestinationMismatch { .. }
        | SamlError::RelayStateMismatch { .. }
        | SamlError::SubjectConfirmationInvalid { .. } => SamlRejection::RequestMismatch,
        _ => SamlRejection::Other,
    }
}

fn collect_attributes(session: &SsoSession) -> BTreeMap<String, Vec<String>> {
    let mut attributes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for attribute in session.attributes().as_slice() {
        attributes
            .entry(attribute.name().to_owned())
            .or_default()
            .extend(
                attribute
                    .values()
                    .iter()
                    .map(|value| value.as_str().to_owned()),
            );
    }
    attributes
}

const EMAIL_NAME_ID_FORMAT: &str = "urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress";

/// The facts an assertion established, as borrowed by the claims mapper.
struct AssertedIdentity<'a> {
    name_id: &'a str,
    name_id_format: Option<&'a str>,
    issuer: &'a str,
    session_index: Option<&'a str>,
    attributes: &'a BTreeMap<String, Vec<String>>,
}

/// Turn a validated identity into the unified claims shape. Pure, so the
/// mapping rules are testable without an assertion.
fn claims_for(
    mapping: &SamlAttributeMapping,
    identity: &AssertedIdentity<'_>,
    now: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>,
) -> Claims {
    let AssertedIdentity {
        name_id,
        name_id_format,
        issuer,
        session_index,
        attributes,
    } = *identity;
    let first = |name: &Option<String>| {
        name.as_deref()
            .and_then(|name| attributes.get(name))
            .and_then(|values| values.first())
            .cloned()
    };

    let email = first(&mapping.email)
        .or_else(|| (name_id_format == Some(EMAIL_NAME_ID_FORMAT)).then(|| name_id.to_owned()));
    let username = first(&mapping.username).unwrap_or_else(|| name_id.to_owned());

    let mut roles: BTreeSet<String> = mapping.default_roles.iter().cloned().collect();
    if let Some(groups) = mapping
        .groups
        .as_deref()
        .and_then(|name| attributes.get(name))
    {
        roles.extend(
            groups
                .iter()
                .filter_map(|group| mapping.group_roles.get(group).cloned()),
        );
    }

    let mut custom = HashMap::new();
    custom.insert(
        "authentication_method".to_owned(),
        serde_json::Value::String("saml".to_owned()),
    );
    custom.insert(
        "saml_issuer".to_owned(),
        serde_json::Value::String(issuer.to_owned()),
    );
    if let Some(format) = name_id_format {
        custom.insert(
            "saml_name_id_format".to_owned(),
            serde_json::Value::String(format.to_owned()),
        );
    }
    if let Some(index) = session_index {
        custom.insert(
            "saml_session_index".to_owned(),
            serde_json::Value::String(index.to_owned()),
        );
    }
    custom.insert("saml_attributes".to_owned(), serde_json::json!(attributes));

    let exp = expires_at
        .unwrap_or_else(|| {
            now + chrono::Duration::from_std(FALLBACK_CLAIMS_LIFETIME)
                .unwrap_or(chrono::Duration::MAX)
        })
        .timestamp();

    Claims {
        sub: format!("saml:{name_id}"),
        email,
        username: Some(username),
        roles: roles.into_iter().collect(),
        perms: Vec::new(),
        exp,
        iat: Some(now.timestamp()),
        jti: None,
        iss: Some(issuer.to_owned()),
        aud: None,
        custom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attributes(pairs: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
        pairs
            .iter()
            .map(|(name, values)| {
                (
                    (*name).to_owned(),
                    values.iter().map(|v| (*v).to_owned()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn claims_map_groups_to_roles_and_fall_back_to_name_id() {
        let mut mapping = SamlAttributeMapping {
            groups: Some("memberOf".to_owned()),
            default_roles: vec!["user".to_owned()],
            ..SamlAttributeMapping::default()
        };
        mapping
            .group_roles
            .insert("CN=Admins".to_owned(), "admin".to_owned());
        let now = Utc::now();
        let attributes = attributes(&[("memberOf", &["CN=Admins", "CN=Other"])]);
        let claims = claims_for(
            &mapping,
            &AssertedIdentity {
                name_id: "alice@example.test",
                name_id_format: Some(EMAIL_NAME_ID_FORMAT),
                issuer: "https://idp.test",
                session_index: Some("s1"),
                attributes: &attributes,
            },
            now,
            None,
        );
        assert_eq!(claims.sub, "saml:alice@example.test");
        assert_eq!(claims.email.as_deref(), Some("alice@example.test"));
        assert_eq!(claims.username.as_deref(), Some("alice@example.test"));
        assert_eq!(claims.roles, ["admin", "user"]);
        assert_eq!(claims.iss.as_deref(), Some("https://idp.test"));
        assert_eq!(
            claims.exp,
            (now + chrono::Duration::seconds(3600)).timestamp()
        );
        assert_eq!(
            claims
                .custom_claim("saml_session_index")
                .and_then(serde_json::Value::as_str),
            Some("s1")
        );
        assert_eq!(
            claims
                .custom_claim("authentication_method")
                .and_then(serde_json::Value::as_str),
            Some("saml")
        );
    }

    #[test]
    fn claims_prefer_mapped_attributes_and_assertion_expiry() {
        let mapping = SamlAttributeMapping {
            email: Some("mail".to_owned()),
            username: Some("uid".to_owned()),
            ..SamlAttributeMapping::default()
        };
        let now = Utc::now();
        let expires = now + chrono::Duration::seconds(120);
        let attributes = attributes(&[("mail", &["alice@example.test"]), ("uid", &["alice"])]);
        let claims = claims_for(
            &mapping,
            &AssertedIdentity {
                name_id: "opaque-id",
                name_id_format: None,
                issuer: "https://idp.test",
                session_index: None,
                attributes: &attributes,
            },
            now,
            Some(expires),
        );
        assert_eq!(claims.email.as_deref(), Some("alice@example.test"));
        assert_eq!(claims.username.as_deref(), Some("alice"));
        assert!(claims.roles.is_empty());
        assert_eq!(claims.exp, expires.timestamp());
        assert!(claims.custom_claim("saml_name_id_format").is_none());
    }

    #[test]
    fn opaque_name_id_without_mapping_yields_no_email() {
        let claims = claims_for(
            &SamlAttributeMapping::default(),
            &AssertedIdentity {
                name_id: "opaque-id",
                name_id_format: Some("urn:oasis:names:tc:SAML:2.0:nameid-format:persistent"),
                issuer: "https://idp.test",
                session_index: None,
                attributes: &BTreeMap::new(),
            },
            Utc::now(),
            None,
        );
        assert!(claims.email.is_none());
        assert_eq!(claims.username.as_deref(), Some("opaque-id"));
    }

    #[test]
    fn in_response_to_is_extracted_from_the_root_attribute() {
        let xml = r#"<samlp:Response ID="_r" InResponseTo="_req1" Version="2.0"><saml:SubjectConfirmationData InResponseTo="_req1"/></samlp:Response>"#;
        assert_eq!(extract_in_response_to(xml).as_deref(), Some("_req1"));
        assert_eq!(
            extract_in_response_to(r#"<Response InResponseTo='_x'/>"#).as_deref(),
            Some("_x")
        );
        assert_eq!(extract_in_response_to(r#"<Response ID="_r"/>"#), None);
        assert_eq!(
            extract_in_response_to(r#"<Response InResponseTo=""/>"#),
            None
        );
    }

    #[test]
    fn pending_login_round_trips_through_the_snapshot() {
        let login = PendingLogin {
            request_id: "_req".to_owned(),
            relay_state: PendingRelayState::Value("/next".to_owned()),
            idp_entity_id: "https://idp.test".to_owned(),
            response_binding: "post".to_owned(),
            request_binding: Some("redirect".to_owned()),
            acs_url: "https://sp.test/acs".to_owned(),
            acs_binding: "post".to_owned(),
            acs_index: Some(0),
            acs_is_default: true,
            issued_at: Some("2026-01-01T00:00:00Z".to_owned()),
            expires_at: Some("2026-01-01T00:10:00Z".to_owned()),
        };
        let snapshot = snapshot_from_pending_login(&login);
        let rebuilt = PendingAuthnRequest::from_snapshot(snapshot).expect("snapshot is valid");
        assert_eq!(pending_login_from_snapshot(rebuilt.snapshot()), login);
    }

    #[test]
    fn rejections_are_classified() {
        assert_eq!(
            classify(&SamlError::SignatureMissing),
            SamlRejection::InvalidSignature
        );
        assert_eq!(
            classify(&SamlError::AudienceMismatch {
                expected: "sp".to_owned()
            }),
            SamlRejection::AudienceMismatch
        );
        assert_eq!(
            classify(&SamlError::ReplayDetected {
                key: "k".to_owned()
            }),
            SamlRejection::Replayed
        );
        assert_eq!(
            classify(&SamlError::Invalid("x".to_owned())),
            SamlRejection::Other
        );
    }
}
