//! Certificate-based caller authorization for mutual-TLS deployments.
//!
//! Mutual TLS *authenticates* a caller: rustls verifies the presented chain
//! against [`TlsConfig::client_ca_path`](crate::config::TlsConfig::client_ca_path)
//! and rejects anything that does not chain to a trusted root. It does not
//! *authorize* one. A certificate issued by a private fleet CA proves only
//! that the CA has, at some point, issued to this principal — every workload
//! the CA has ever signed for is admitted identically. One compromised
//! workload then reaches every mTLS route of every peer.
//!
//! This module closes that gap: it names the caller from its leaf
//! certificate's `subjectAltName` and compares that name against an
//! operator-configured allowlist.
//!
//! # Model
//!
//! - [`CallerSan`] is a caller's name, taken from a DNS or URI SAN. Matching is
//!   byte-exact within a kind. There is no wildcard, suffix or subdomain
//!   matching, and a DNS SAN never matches a URI allowlist entry.
//! - [`CallerAllowlist`] is the set of names permitted to call. It cannot be
//!   constructed empty: an empty allowlist that admits everyone is the classic
//!   way this control fails open.
//! - [`CallerAuthMode`] selects what counts as proof — a certificate, a bearer
//!   token, or either. [`CallerAuthMode::MtlsOrBearer`] exists so a deployment
//!   can cut over without a flag day.
//! - [`authorize`] is the whole decision, as a pure function over the policy,
//!   the leaf DER and whether a bearer credential was presented. It names no
//!   transport type, so the same decision serves HTTP and gRPC and is testable
//!   without a socket.
//! - [`CallerAuthLayer`] applies that decision as a tower layer, and
//!   [`CallerIdentity`] carries the result to handlers.
//!
//! # Failures are distinguishable
//!
//! Nothing proven answers `401`; an identity proven but not allowlisted answers
//! `403`. The operator of a misconfigured caller needs to tell "my certificate
//! is not being accepted" from "my certificate is fine, you have not authorized
//! me" without reading the server's logs.
//!
//! # What this does not do
//!
//! A certificate-authorized request carries no
//! [`Claims`](crate::middleware::token::Claims). Cedar authorization
//! ([`CedarAuthz`](crate::middleware::CedarAuthz)) derives its principal from
//! claims, so Cedar-protected routes still require a bearer token even when
//! the caller is allowlisted here. This layer is a transport-level admission
//! control, not a replacement for policy authorization.
//!
//! # Example
//!
//! ```no_run
//! use acton_service::caller_auth::{
//!     CallerAllowlist, CallerAuthLayer, CallerAuthPolicy, CallerSan,
//! };
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let allowlist = CallerAllowlist::new([
//!     CallerSan::uri("spiffe://cluster.local/ns/prod/sa/ingest")?,
//!     CallerSan::dns("reporter.internal")?,
//! ])?;
//!
//! let router: axum::Router = axum::Router::new()
//!     .route("/admin/audit", axum::routing::get(|| async { "ok" }))
//!     .route_layer(CallerAuthLayer::http(CallerAuthPolicy::mtls(allowlist)));
//! # Ok(())
//! # }
//! ```

use std::collections::BTreeSet;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::response::IntoResponse;
use http::StatusCode;
use serde::{Deserialize, Serialize};
use tower::{Layer, Service};

use crate::error::ErrorResponse;

/// Longest accepted `subjectAltName` value, in bytes.
///
/// DNS names are bounded at 253 by the DNS itself; URI SANs have no such
/// bound. The cap keeps a pathological certificate from turning into a
/// pathological allowlist comparison, and is far above any real SPIFFE ID.
const MAX_SAN_LEN: usize = 1024;

/// HTTP probe endpoints exempt from caller authorization, matched exactly.
///
/// Infrastructure probes call these without credentials of any kind — a
/// kubelet does not present a client certificate — and the token middleware
/// exempts the same set. Matched exactly rather than by prefix so a route like
/// `/health-admin` does not inherit the exemption. Extend it per deployment
/// with
/// [`CallerAuthConfig::public_paths`](crate::config::CallerAuthConfig::public_paths).
const HTTP_PROBE_PATHS: [&str; 2] = ["/health", "/ready"];

/// HTTP documentation trees exempt from caller authorization, matched as
/// prefixes because each serves a whole subtree of assets.
const HTTP_DOC_PATH_PREFIXES: [&str; 2] = ["/swagger-ui", "/api-docs"];

// ---------------------------------------------------------------------------
// Caller names
// ---------------------------------------------------------------------------

/// Which kind of `subjectAltName` a [`CallerSan`] came from.
///
/// The kind is part of a caller's identity, not decoration. Comparing across
/// kinds would let a DNS name that happens to spell a URI stand in for that
/// URI, so [`CallerSan`] equality requires the kinds to match too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SanKind {
    /// A `dNSName` general name, e.g. `reporter.internal`.
    Dns,
    /// A `uniformResourceIdentifier` general name, e.g. a SPIFFE ID.
    Uri,
}

impl fmt::Display for SanKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dns => f.write_str("DNS"),
            Self::Uri => f.write_str("URI"),
        }
    }
}

/// A caller's name, taken from one `subjectAltName` of its leaf certificate.
///
/// Values are byte-exact. `reporter.internal` does not match
/// `REPORTER.INTERNAL`, `a.reporter.internal` or `reporter.internal.`, and no
/// value containing `*` can be constructed at all — a wildcard certificate
/// names a set of hosts, which is not an identity anything should be
/// authorized as.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CallerSan {
    kind: SanKind,
    value: String,
}

impl CallerSan {
    /// Build a name from a `dNSName` SAN.
    ///
    /// # Errors
    ///
    /// Returns [`CallerSanError`] if the value is empty, longer than 1024
    /// bytes, contains a wildcard, or contains whitespace or control
    /// characters.
    pub fn dns(value: impl Into<String>) -> Result<Self, CallerSanError> {
        Self::new(SanKind::Dns, value.into())
    }

    /// Build a name from a `uniformResourceIdentifier` SAN.
    ///
    /// # Errors
    ///
    /// Returns [`CallerSanError`] under the same conditions as
    /// [`CallerSan::dns`].
    pub fn uri(value: impl Into<String>) -> Result<Self, CallerSanError> {
        Self::new(SanKind::Uri, value.into())
    }

    /// Build a name from a configuration entry, inferring the kind.
    ///
    /// An entry containing `://` is a URI SAN; anything else is a DNS SAN.
    /// This inference exists only so operators can write a plain list of
    /// strings in `[caller_auth].allowlist`. Code that knows the kind should
    /// say so with [`CallerSan::dns`] or [`CallerSan::uri`].
    ///
    /// # Errors
    ///
    /// Returns [`CallerSanError`] under the same conditions as
    /// [`CallerSan::dns`].
    pub fn parse(entry: &str) -> Result<Self, CallerSanError> {
        if entry.contains("://") {
            Self::uri(entry)
        } else {
            Self::dns(entry)
        }
    }

    fn new(kind: SanKind, value: String) -> Result<Self, CallerSanError> {
        if value.is_empty() {
            return Err(CallerSanError::Empty);
        }
        if value.len() > MAX_SAN_LEN {
            return Err(CallerSanError::TooLong {
                len: value.len(),
                max: MAX_SAN_LEN,
            });
        }
        if value.contains('*') {
            return Err(CallerSanError::Wildcard { value });
        }
        if value
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '\u{feff}')
        {
            return Err(CallerSanError::IllegalCharacter { value });
        }
        Ok(Self { kind, value })
    }

    /// Which kind of SAN this name came from.
    #[must_use]
    pub fn kind(&self) -> SanKind {
        self.kind
    }

    /// The name itself, without the kind.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for CallerSan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind, self.value)
    }
}

/// Why a `subjectAltName` value was rejected as a caller name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CallerSanError {
    /// The value was empty.
    #[error("subjectAltName is empty")]
    Empty,

    /// The value exceeded [`MAX_SAN_LEN`].
    #[error("subjectAltName is {len} bytes, over the {max}-byte limit")]
    TooLong {
        /// Length of the offending value.
        len: usize,
        /// The accepted maximum.
        max: usize,
    },

    /// The value contained `*`.
    #[error(
        "subjectAltName '{value}' contains a wildcard; a wildcard names a set of hosts, not a \
         caller identity"
    )]
    Wildcard {
        /// The offending value.
        value: String,
    },

    /// The value contained whitespace or a control character.
    #[error("subjectAltName '{value}' contains whitespace or control characters")]
    IllegalCharacter {
        /// The offending value.
        value: String,
    },
}

// ---------------------------------------------------------------------------
// Allowlist
// ---------------------------------------------------------------------------

/// The set of callers permitted to reach a route.
///
/// Cannot be constructed empty. An allowlist with no entries either admits
/// everyone (fails open, silently) or admits no one (fails closed, and no
/// operator writes that on purpose); refusing to build one turns the mistake
/// into a startup error instead of a runtime surprise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerAllowlist(BTreeSet<CallerSan>);

impl CallerAllowlist {
    /// Build an allowlist from caller names.
    ///
    /// # Errors
    ///
    /// Returns [`CallerAuthConfigError::EmptyAllowlist`] if the iterator
    /// yields nothing.
    pub fn new<I>(entries: I) -> Result<Self, CallerAuthConfigError>
    where
        I: IntoIterator<Item = CallerSan>,
    {
        let set: BTreeSet<CallerSan> = entries.into_iter().collect();
        if set.is_empty() {
            return Err(CallerAuthConfigError::EmptyAllowlist);
        }
        Ok(Self(set))
    }

    /// Build an allowlist from configuration strings, inferring each kind with
    /// [`CallerSan::parse`].
    ///
    /// # Errors
    ///
    /// Returns [`CallerAuthConfigError::InvalidEntry`] for the first
    /// unparseable entry, or [`CallerAuthConfigError::EmptyAllowlist`] if
    /// there are none.
    pub fn from_entries<I, S>(entries: I) -> Result<Self, CallerAuthConfigError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let parsed = entries
            .into_iter()
            .map(|entry| {
                let entry = entry.as_ref();
                CallerSan::parse(entry).map_err(|source| CallerAuthConfigError::InvalidEntry {
                    entry: entry.to_string(),
                    source,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(parsed)
    }

    /// Whether this caller is allowed.
    #[must_use]
    pub fn contains(&self, san: &CallerSan) -> bool {
        self.0.contains(san)
    }

    /// How many callers are allowed. Always at least one — there is
    /// deliberately no `is_empty`, because an empty allowlist cannot exist.
    #[must_use]
    pub fn count(&self) -> usize {
        self.0.len()
    }

    /// Iterate the allowed callers.
    pub fn iter(&self) -> impl Iterator<Item = &CallerSan> {
        self.0.iter()
    }
}

impl<'a> IntoIterator for &'a CallerAllowlist {
    type Item = &'a CallerSan;
    type IntoIter = std::collections::btree_set::Iter<'a, CallerSan>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

/// What a caller must present to be admitted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CallerAuthMode {
    /// Bearer credentials only. Caller authorization is inert: the token
    /// middleware remains the whole of the identity check. This is the default
    /// and matches the behaviour of deployments that never configure
    /// `[caller_auth]`.
    #[default]
    Bearer,

    /// A verified, allowlisted client certificate is required. Requests
    /// without one are rejected before reaching the inner service, whatever
    /// bearer credential they carry.
    Mtls,

    /// Either a verified allowlisted client certificate *or* a bearer
    /// credential. Exists so a fleet can move to mTLS caller-by-caller: a
    /// caller not yet issued a certificate keeps working on its token, and a
    /// caller whose certificate is not yet allowlisted falls back to its token
    /// rather than breaking.
    MtlsOrBearer,
}

impl CallerAuthMode {
    /// Whether this mode can admit a caller on its certificate alone.
    #[must_use]
    pub fn uses_client_certificates(self) -> bool {
        matches!(self, Self::Mtls | Self::MtlsOrBearer)
    }
}

impl fmt::Display for CallerAuthMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bearer => f.write_str("bearer"),
            Self::Mtls => f.write_str("mtls"),
            Self::MtlsOrBearer => f.write_str("mtls-or-bearer"),
        }
    }
}

/// A complete caller-authorization policy.
///
/// The constructors make the illegal combinations unrepresentable: a
/// certificate mode always carries an allowlist, and [`CallerAuthMode::Bearer`]
/// never does. Configuration that expresses one of those combinations is
/// refused at startup rather than accepted and ignored — dead config that
/// looks like protection is worse than no config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerAuthPolicy {
    mode: CallerAuthMode,
    allowlist: Option<CallerAllowlist>,
    public_paths: Arc<[String]>,
}

impl CallerAuthPolicy {
    /// A policy that defers entirely to bearer credentials.
    #[must_use]
    pub fn bearer() -> Self {
        Self {
            mode: CallerAuthMode::Bearer,
            allowlist: None,
            public_paths: Arc::from(Vec::new()),
        }
    }

    /// Require a verified certificate whose SAN is on `allowlist`.
    #[must_use]
    pub fn mtls(allowlist: CallerAllowlist) -> Self {
        Self {
            mode: CallerAuthMode::Mtls,
            allowlist: Some(allowlist),
            public_paths: Arc::from(Vec::new()),
        }
    }

    /// Accept either a certificate on `allowlist` or a bearer credential.
    #[must_use]
    pub fn mtls_or_bearer(allowlist: CallerAllowlist) -> Self {
        Self {
            mode: CallerAuthMode::MtlsOrBearer,
            allowlist: Some(allowlist),
            public_paths: Arc::from(Vec::new()),
        }
    }

    /// Exempt these path prefixes, in addition to the built-in infrastructure
    /// paths (`/health`, `/ready`, `/swagger-ui`, `/api-docs` on HTTP; the
    /// gRPC health and reflection services on gRPC).
    #[must_use]
    pub fn with_public_paths<I, S>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.public_paths = paths.into_iter().map(Into::into).collect();
        self
    }

    /// The configured mode.
    #[must_use]
    pub fn mode(&self) -> CallerAuthMode {
        self.mode
    }

    /// The allowlist, absent in [`CallerAuthMode::Bearer`].
    #[must_use]
    pub fn allowlist(&self) -> Option<&CallerAllowlist> {
        self.allowlist.as_ref()
    }

    /// The configured public path prefixes.
    #[must_use]
    pub fn public_paths(&self) -> &[String] {
        &self.public_paths
    }

    /// Whether this policy needs the listener to verify client certificates.
    ///
    /// A `true` here with no `client_ca_path` on the listener is a
    /// misconfiguration: the policy would reject every caller, because no
    /// certificate is ever requested during the handshake.
    #[must_use]
    pub fn requires_client_ca(&self) -> bool {
        self.mode.uses_client_certificates()
    }
}

/// Configuration that cannot produce a usable policy.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CallerAuthConfigError {
    /// A certificate mode was configured with no allowed callers.
    #[error(
        "[caller_auth].allowlist is empty; a certificate mode with no allowed callers cannot \
         admit anyone, and an allowlist that admits everyone is not an allowlist"
    )]
    EmptyAllowlist,

    /// An allowlist was configured under [`CallerAuthMode::Bearer`], where
    /// nothing would ever consult it.
    #[error(
        "[caller_auth].allowlist is set but mode is 'bearer', so it is never consulted; either \
         set mode to 'mtls' or 'mtls-or-bearer', or remove the allowlist"
    )]
    AllowlistWithoutCertificateMode,

    /// An allowlist entry was not a usable caller name.
    #[error("[caller_auth].allowlist entry '{entry}' is not a usable caller name: {source}")]
    InvalidEntry {
        /// The offending entry.
        entry: String,
        /// Why it was rejected.
        source: CallerSanError,
    },

    /// A certificate mode was configured on a listener that never asks for a
    /// client certificate.
    #[error(
        "[caller_auth].mode = '{mode}' requires verified client certificates, but {section} has \
         no client_ca_path, so no certificate is ever requested and every caller would be \
         rejected"
    )]
    MissingClientCa {
        /// The configured mode.
        mode: CallerAuthMode,
        /// The configuration section that should carry `client_ca_path`.
        section: &'static str,
    },

    /// A certificate mode was configured on a listener that serves plaintext.
    #[error(
        "[caller_auth].mode = '{mode}' requires verified client certificates, but {section} \
         terminates no TLS; caller certificates cannot exist on a plaintext listener"
    )]
    NoTlsListener {
        /// The configured mode.
        mode: CallerAuthMode,
        /// The configuration section that should carry the TLS credentials.
        section: &'static str,
    },
}

/// What a listener's configuration says about client certificates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenerClientCa {
    /// The listener verifies client certificates against a CA bundle.
    Verified,

    /// The listener terminates TLS but requests no client certificate, so no
    /// caller can ever present one.
    NotVerified,

    /// The listener terminates no TLS at all.
    Plaintext,

    /// The listener's TLS was supplied programmatically rather than from
    /// configuration.
    ///
    /// The framework cannot inspect an already-built `ServerConfig` for a
    /// client verifier, so it does not guess. A caller that hands over its own
    /// TLS configuration owns that check.
    Unknown,
}

/// Whether a listener can satisfy a policy that needs client certificates.
///
/// Pure: the verdict follows from its three arguments. `section` names the
/// configuration section in the resulting message, so an operator is pointed
/// at the listener actually short a `client_ca_path` — `[tls]` and
/// `[grpc.tls]` are configured independently, and a fleet that got one right
/// and the other wrong is exactly the case worth naming precisely.
///
/// # Errors
///
/// Returns [`CallerAuthConfigError::MissingClientCa`] when the listener
/// verifies no client certificates, and
/// [`CallerAuthConfigError::NoTlsListener`] when it terminates no TLS at all.
/// Both are startup failures rather than warnings: a policy that cannot admit
/// anyone is not protection, and a service that boots anyway would report
/// every caller as unauthorized with no hint as to why.
pub fn validate_listener(
    policy: &CallerAuthPolicy,
    listener: ListenerClientCa,
    section: &'static str,
) -> Result<(), CallerAuthConfigError> {
    if !policy.requires_client_ca() {
        return Ok(());
    }

    match listener {
        ListenerClientCa::Verified | ListenerClientCa::Unknown => Ok(()),
        ListenerClientCa::NotVerified => Err(CallerAuthConfigError::MissingClientCa {
            mode: policy.mode,
            section,
        }),
        ListenerClientCa::Plaintext => Err(CallerAuthConfigError::NoTlsListener {
            mode: policy.mode,
            section,
        }),
    }
}

// ---------------------------------------------------------------------------
// The decision
// ---------------------------------------------------------------------------

/// How a caller proved itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthenticatedCaller {
    /// The caller presented a verified certificate whose SAN is allowlisted.
    Certificate(CallerSan),

    /// The caller presented a bearer credential, which this layer has *not*
    /// validated.
    ///
    /// Caller authorization deliberately does not parse or verify tokens: the
    /// token middleware owns that, and doing it twice in two places is how the
    /// two drift apart. This variant means only "a bearer credential is
    /// present, let the token middleware rule on it" — a request carrying a
    /// forged token reaches this outcome and is then rejected downstream.
    BearerDeferred,
}

/// Why a caller was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CallerAuthError {
    /// No client certificate was presented.
    #[error("no client certificate presented")]
    NoClientCertificate,

    /// Neither a client certificate nor a bearer credential was presented.
    #[error("no client certificate and no bearer credential presented")]
    NoCredential,

    /// The leaf certificate could not be parsed.
    #[error("client certificate could not be parsed: {0}")]
    MalformedCertificate(String),

    /// The leaf certificate carries no usable DNS or URI SAN.
    #[error("client certificate carries no DNS or URI subjectAltName")]
    NoUsableSan,

    /// The caller was named, and that name is not allowlisted.
    #[error("caller {0} is not on the allowlist")]
    NotAllowlisted(CallerSan),
}

impl CallerAuthError {
    /// Whether the caller failed to prove any identity (`401`), as opposed to
    /// proving one that is not authorized (`403`).
    ///
    /// [`CallerAuthError::NoUsableSan`] counts as authorization: the chain
    /// verified, so the caller *has* a trusted certificate — it simply carries
    /// no name that an allowlist could ever contain. Answering `401` there
    /// would send an operator hunting for a credential problem that does not
    /// exist.
    #[must_use]
    pub fn is_authentication_failure(&self) -> bool {
        match self {
            Self::NoClientCertificate | Self::NoCredential | Self::MalformedCertificate(_) => true,
            Self::NoUsableSan | Self::NotAllowlisted(_) => false,
        }
    }

    /// The HTTP status this refusal answers with.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        if self.is_authentication_failure() {
            StatusCode::UNAUTHORIZED
        } else {
            StatusCode::FORBIDDEN
        }
    }

    /// A stable machine-readable code, distinct per cause, carried in the
    /// error body's `code` field.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::NoClientCertificate => "CLIENT_CERT_REQUIRED",
            Self::NoCredential => "CALLER_CREDENTIAL_REQUIRED",
            Self::MalformedCertificate(_) => "CLIENT_CERT_UNPARSABLE",
            Self::NoUsableSan => "CLIENT_CERT_NO_SAN",
            Self::NotAllowlisted(_) => "CALLER_NOT_ALLOWED",
        }
    }
}

/// Decide whether a caller may proceed.
///
/// Pure: the verdict follows from the three arguments and nothing else. No
/// transport type appears in the signature, so the same decision serves the
/// HTTP and gRPC listeners and can be tested without a socket.
///
/// `leaf_der` is the DER-encoded end-entity certificate from a *completed*
/// mutual-TLS handshake — rustls has already verified it against the
/// configured CA bundle. This function never re-checks the chain; it only
/// names the caller and consults the allowlist.
///
/// `bearer_present` says whether the request carried a bearer credential. It
/// says nothing about that credential being valid, which is the token
/// middleware's job.
///
/// # Errors
///
/// Returns [`CallerAuthError`] describing which of the two distinguishable
/// failures occurred: no identity proven, or an identity proven that is not
/// allowlisted.
pub fn authorize(
    policy: &CallerAuthPolicy,
    leaf_der: Option<&[u8]>,
    bearer_present: bool,
) -> Result<AuthenticatedCaller, CallerAuthError> {
    match policy.mode {
        CallerAuthMode::Bearer => Ok(AuthenticatedCaller::BearerDeferred),

        CallerAuthMode::Mtls => {
            let leaf = leaf_der.ok_or(CallerAuthError::NoClientCertificate)?;
            authorize_certificate(policy, leaf).map(AuthenticatedCaller::Certificate)
        }

        CallerAuthMode::MtlsOrBearer => {
            // The certificate is tried first so an allowlisted caller is
            // identified as itself even when it also sends a token. Only if
            // the certificate cannot carry the request does the bearer
            // credential get its turn: during a cutover, a caller whose
            // certificate is not yet allowlisted must keep working on the
            // credential it had before, not start failing.
            let cert_outcome = match leaf_der {
                Some(leaf) => authorize_certificate(policy, leaf),
                None => Err(CallerAuthError::NoClientCertificate),
            };

            match cert_outcome {
                Ok(san) => Ok(AuthenticatedCaller::Certificate(san)),
                Err(cert_error) if bearer_present => {
                    tracing::debug!(
                        reason = %cert_error,
                        "client certificate did not authorize the caller; deferring to bearer \
                         credential"
                    );
                    Ok(AuthenticatedCaller::BearerDeferred)
                }
                Err(CallerAuthError::NoClientCertificate) => Err(CallerAuthError::NoCredential),
                Err(cert_error) => Err(cert_error),
            }
        }
    }
}

/// Name the caller from its leaf certificate and consult the allowlist.
fn authorize_certificate(
    policy: &CallerAuthPolicy,
    leaf_der: &[u8],
) -> Result<CallerSan, CallerAuthError> {
    let sans = subject_alt_names(leaf_der)?;
    if sans.is_empty() {
        return Err(CallerAuthError::NoUsableSan);
    }

    let Some(allowlist) = policy.allowlist.as_ref() else {
        // Unreachable through the constructors, which pair every certificate
        // mode with an allowlist. Treated as "not allowlisted" rather than
        // "allowed" so that any future path that loses the allowlist fails
        // closed.
        return Err(CallerAuthError::NotAllowlisted(sans[0].clone()));
    };

    sans.iter()
        .find(|san| allowlist.contains(san))
        .cloned()
        .ok_or_else(|| CallerAuthError::NotAllowlisted(sans[0].clone()))
}

/// Extract every usable DNS and URI `subjectAltName` from a DER leaf.
///
/// SAN values that are not usable caller names — wildcards above all — are
/// dropped rather than rejected: a certificate legitimately carrying
/// `*.internal` alongside `reporter.internal` should authorize as
/// `reporter.internal`, and the wildcard must never match anything.
///
/// # Errors
///
/// Returns [`CallerAuthError::MalformedCertificate`] if the DER cannot be
/// parsed or its SAN extension is malformed.
fn subject_alt_names(leaf_der: &[u8]) -> Result<Vec<CallerSan>, CallerAuthError> {
    use x509_parser::extensions::GeneralName;
    use x509_parser::prelude::FromDer;
    use x509_parser::prelude::X509Certificate;

    let (_, cert) = X509Certificate::from_der(leaf_der)
        .map_err(|e| CallerAuthError::MalformedCertificate(e.to_string()))?;

    let Some(san_extension) = cert
        .subject_alternative_name()
        .map_err(|e| CallerAuthError::MalformedCertificate(e.to_string()))?
    else {
        return Ok(Vec::new());
    };

    let mut names = Vec::new();
    for general_name in &san_extension.value.general_names {
        let parsed = match general_name {
            GeneralName::DNSName(value) => CallerSan::dns(*value),
            GeneralName::URI(value) => CallerSan::uri(*value),
            _ => continue,
        };

        match parsed {
            Ok(san) => names.push(san),
            Err(e) => {
                tracing::debug!(
                    reason = %e,
                    "ignoring subjectAltName that cannot name a caller"
                );
            }
        }
    }

    Ok(names)
}

// ---------------------------------------------------------------------------
// Request-scoped identity
// ---------------------------------------------------------------------------

/// The caller identity established by [`CallerAuthLayer`], inserted into the
/// request extensions.
///
/// Read it from a handler with `Extension<CallerIdentity>`, or from middleware
/// with `request.extensions().get::<CallerIdentity>()`. Its presence means a
/// verified, allowlisted certificate: the layer never inserts one for a
/// request it let through on a bearer credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerIdentity {
    san: CallerSan,
    waives_bearer: bool,
}

impl CallerIdentity {
    /// The allowlisted name this caller proved.
    #[must_use]
    pub fn san(&self) -> &CallerSan {
        &self.san
    }

    /// Whether this certificate identity stands in for a bearer credential.
    ///
    /// True only under [`CallerAuthMode::MtlsOrBearer`], where the certificate
    /// is an alternative to a token and the token middleware stands down for
    /// this request. Under [`CallerAuthMode::Mtls`] the certificate is an
    /// *additional* requirement, so a configured token middleware still runs
    /// and still demands a valid token.
    #[must_use]
    pub fn waives_bearer(&self) -> bool {
        self.waives_bearer
    }
}

/// Whether a request already carries a certificate identity that stands in for
/// a bearer credential, so token middleware should not demand one.
pub(crate) fn bearer_waived(extensions: &http::Extensions) -> bool {
    extensions
        .get::<CallerIdentity>()
        .is_some_and(CallerIdentity::waives_bearer)
}

// ---------------------------------------------------------------------------
// Tower layer
// ---------------------------------------------------------------------------

/// How a refusal is shaped on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RejectionStyle {
    /// A JSON [`ErrorResponse`] with a `401`/`403` status.
    Http,
    /// A trailers-only gRPC response carrying `UNAUTHENTICATED` (16) or
    /// `PERMISSION_DENIED` (7).
    #[cfg(feature = "grpc")]
    Grpc,
}

/// Applies a [`CallerAuthPolicy`] to every request reaching the inner service.
///
/// Use [`CallerAuthLayer::http`] on an HTTP router and
/// [`CallerAuthLayer::grpc`] on a gRPC router; the decision is identical and
/// only the shape of a refusal differs. Apply it with `Router::route_layer` to
/// scope it to a set of routes, or `Router::layer` for the whole surface.
///
/// The layer reads the verified certificate from
/// [`TlsConnectInfo`](crate::tls::TlsConnectInfo), which the TLS listener
/// installs as connect-info, so it only sees certificates on a listener that
/// terminates TLS itself. Behind a TLS-terminating proxy there is no client
/// certificate to read, and a certificate mode will refuse every request —
/// which is the correct direction to fail, but means caller authorization
/// belongs on the process that terminates the mTLS connection.
#[derive(Debug, Clone)]
pub struct CallerAuthLayer {
    policy: Arc<CallerAuthPolicy>,
    style: RejectionStyle,
}

impl CallerAuthLayer {
    /// A layer that refuses with a JSON error body, for HTTP routers.
    #[must_use]
    pub fn http(policy: CallerAuthPolicy) -> Self {
        Self {
            policy: Arc::new(policy),
            style: RejectionStyle::Http,
        }
    }

    /// A layer that refuses with a gRPC status, for gRPC routers.
    #[cfg(feature = "grpc")]
    #[must_use]
    pub fn grpc(policy: CallerAuthPolicy) -> Self {
        Self {
            policy: Arc::new(policy),
            style: RejectionStyle::Grpc,
        }
    }

    /// The policy this layer applies.
    #[must_use]
    pub fn policy(&self) -> &CallerAuthPolicy {
        &self.policy
    }
}

impl<S> Layer<S> for CallerAuthLayer {
    type Service = CallerAuthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        CallerAuthService {
            inner,
            policy: self.policy.clone(),
            style: self.style,
        }
    }
}

/// The service produced by [`CallerAuthLayer`].
#[derive(Debug, Clone)]
pub struct CallerAuthService<S> {
    inner: S,
    policy: Arc<CallerAuthPolicy>,
    style: RejectionStyle,
}

impl<S, ReqBody> Service<http::Request<ReqBody>> for CallerAuthService<S>
where
    S: Service<http::Request<ReqBody>, Response = http::Response<axum::body::Body>>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
{
    type Response = http::Response<axum::body::Body>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: http::Request<ReqBody>) -> Self::Future {
        // Take the ready inner service and leave a fresh clone in its place,
        // so the readiness obtained via poll_ready is the one consumed here.
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);

        if self.policy.mode == CallerAuthMode::Bearer || self.is_exempt(req.uri().path()) {
            return Box::pin(async move { inner.call(req).await });
        }

        let leaf = leaf_certificate(req.extensions());
        let bearer_present = has_bearer_credential(req.headers());

        match authorize(&self.policy, leaf.as_deref(), bearer_present) {
            Ok(AuthenticatedCaller::Certificate(san)) => {
                tracing::debug!(caller = %san, "caller authorized by client certificate");
                req.extensions_mut().insert(CallerIdentity {
                    san,
                    waives_bearer: self.policy.mode == CallerAuthMode::MtlsOrBearer,
                });
                Box::pin(async move { inner.call(req).await })
            }
            Ok(AuthenticatedCaller::BearerDeferred) => {
                Box::pin(async move { inner.call(req).await })
            }
            Err(e) => {
                tracing::warn!(
                    path = %req.uri().path(),
                    code = e.code(),
                    reason = %e,
                    "caller authorization refused"
                );
                let response = self.rejection(&e);
                Box::pin(async move { Ok(response) })
            }
        }
    }
}

impl<S> CallerAuthService<S> {
    /// Whether this path is exempt from caller authorization.
    fn is_exempt(&self, path: &str) -> bool {
        let infra = match self.style {
            RejectionStyle::Http => {
                HTTP_PROBE_PATHS.contains(&path)
                    || HTTP_DOC_PATH_PREFIXES.iter().any(|p| path.starts_with(p))
            }
            #[cfg(feature = "grpc")]
            RejectionStyle::Grpc => crate::grpc::middleware::is_grpc_infra_path(path),
        };

        infra
            || self
                .policy
                .public_paths
                .iter()
                .any(|p| path.starts_with(p.as_str()))
    }

    /// Build the wire response for a refusal.
    fn rejection(&self, error: &CallerAuthError) -> http::Response<axum::body::Body> {
        match self.style {
            RejectionStyle::Http => {
                let status = error.status();
                (
                    status,
                    axum::Json(ErrorResponse::with_code(
                        status,
                        error.code(),
                        error.to_string(),
                    )),
                )
                    .into_response()
            }
            #[cfg(feature = "grpc")]
            RejectionStyle::Grpc => {
                let message = format!("{} ({})", error, error.code());
                let status = if error.is_authentication_failure() {
                    tonic::Status::unauthenticated(message)
                } else {
                    tonic::Status::permission_denied(message)
                };
                status.into_http()
            }
        }
    }
}

/// The verified leaf certificate for this request, if the connection was
/// mutually authenticated.
fn leaf_certificate(extensions: &http::Extensions) -> Option<Vec<u8>> {
    let connect_info =
        extensions.get::<axum::extract::ConnectInfo<crate::tls::TlsConnectInfo>>()?;
    connect_info
        .0
        .peer_certificates()
        .map(|chain| chain.leaf().as_ref().to_vec())
}

/// Whether the request carries a bearer credential, without judging it.
fn has_bearer_credential(headers: &http::HeaderMap) -> bool {
    headers
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.split_once(' ').is_some_and(|(scheme, rest)| {
                scheme.eq_ignore_ascii_case("bearer") && !rest.is_empty()
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowlist(entries: &[&str]) -> CallerAllowlist {
        CallerAllowlist::from_entries(entries.iter().copied()).expect("valid allowlist")
    }

    /// A self-signed leaf carrying exactly the given SANs, DER-encoded.
    ///
    /// Self-signed is enough: [`authorize`] never re-verifies the chain — by
    /// the time a certificate reaches it, rustls has already validated it
    /// against the configured CA bundle.
    fn leaf_with_sans(sans: &[rcgen::SanType]) -> Vec<u8> {
        let key = rcgen::KeyPair::generate().expect("key generation");
        let mut params = rcgen::CertificateParams::default();
        params.subject_alt_names = sans.to_vec();
        params
            .self_signed(&key)
            .expect("self-signed leaf")
            .der()
            .to_vec()
    }

    fn dns_san(value: &str) -> rcgen::SanType {
        rcgen::SanType::DnsName(value.try_into().expect("valid IA5 string"))
    }

    fn uri_san(value: &str) -> rcgen::SanType {
        rcgen::SanType::URI(value.try_into().expect("valid IA5 string"))
    }

    mod caller_san {
        use super::*;

        #[test]
        fn dns_and_uri_of_the_same_string_are_different_callers() {
            let dns = CallerSan::dns("spiffe.example").expect("valid");
            let uri = CallerSan::uri("spiffe.example").expect("valid");
            assert_ne!(dns, uri);
        }

        #[test]
        fn parse_infers_uri_from_a_scheme() {
            let parsed =
                CallerSan::parse("spiffe://cluster.local/ns/prod/sa/ingest").expect("valid entry");
            assert_eq!(parsed.kind(), SanKind::Uri);

            let parsed = CallerSan::parse("reporter.internal").expect("valid entry");
            assert_eq!(parsed.kind(), SanKind::Dns);
        }

        #[test]
        fn wildcards_are_refused() {
            let error = CallerSan::dns("*.internal").expect_err("wildcard must be refused");
            assert!(matches!(error, CallerSanError::Wildcard { .. }));
        }

        #[test]
        fn empty_and_oversized_values_are_refused() {
            assert_eq!(
                CallerSan::dns("").expect_err("empty"),
                CallerSanError::Empty
            );

            let long = "a".repeat(MAX_SAN_LEN + 1);
            assert!(matches!(
                CallerSan::dns(long).expect_err("oversized"),
                CallerSanError::TooLong { .. }
            ));
        }

        #[test]
        fn whitespace_and_control_characters_are_refused() {
            assert!(matches!(
                CallerSan::dns("reporter .internal").expect_err("space"),
                CallerSanError::IllegalCharacter { .. }
            ));
            assert!(matches!(
                CallerSan::dns("reporter\n.internal").expect_err("newline"),
                CallerSanError::IllegalCharacter { .. }
            ));
        }

        #[test]
        fn matching_is_byte_exact() {
            let list = allowlist(&["reporter.internal"]);
            assert!(list.contains(&CallerSan::dns("reporter.internal").expect("valid")));

            for near_miss in [
                "REPORTER.INTERNAL",
                "reporter.internal.",
                "a.reporter.internal",
                "reporter.interna",
            ] {
                assert!(
                    !list.contains(&CallerSan::dns(near_miss).expect("valid")),
                    "{near_miss} must not match"
                );
            }
        }
    }

    mod allowlist_construction {
        use super::*;

        #[test]
        fn an_empty_allowlist_cannot_be_built() {
            assert_eq!(
                CallerAllowlist::new(Vec::new()).expect_err("empty must be refused"),
                CallerAuthConfigError::EmptyAllowlist
            );
            assert_eq!(
                CallerAllowlist::from_entries(Vec::<String>::new())
                    .expect_err("empty must be refused"),
                CallerAuthConfigError::EmptyAllowlist
            );
        }

        #[test]
        fn an_invalid_entry_names_itself() {
            let error = CallerAllowlist::from_entries(["ok.internal", "*.internal"])
                .expect_err("wildcard entry must be refused");
            match error {
                CallerAuthConfigError::InvalidEntry { entry, .. } => {
                    assert_eq!(entry, "*.internal");
                }
                other => panic!("unexpected error: {other}"),
            }
        }

        #[test]
        fn duplicate_entries_collapse() {
            let list = allowlist(&["a.internal", "a.internal", "b.internal"]);
            assert_eq!(list.count(), 2);
        }
    }

    mod error_mapping {
        use super::*;

        #[test]
        fn nothing_proven_is_401_and_identity_proven_is_403() {
            assert_eq!(
                CallerAuthError::NoClientCertificate.status(),
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(
                CallerAuthError::NoCredential.status(),
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(
                CallerAuthError::MalformedCertificate("bad".into()).status(),
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(CallerAuthError::NoUsableSan.status(), StatusCode::FORBIDDEN);
            assert_eq!(
                CallerAuthError::NotAllowlisted(CallerSan::dns("x.internal").expect("valid"))
                    .status(),
                StatusCode::FORBIDDEN
            );
        }

        #[test]
        fn every_cause_has_a_distinct_code() {
            let codes = [
                CallerAuthError::NoClientCertificate.code(),
                CallerAuthError::NoCredential.code(),
                CallerAuthError::MalformedCertificate("bad".into()).code(),
                CallerAuthError::NoUsableSan.code(),
                CallerAuthError::NotAllowlisted(CallerSan::dns("x.internal").expect("valid"))
                    .code(),
            ];
            let unique: BTreeSet<&str> = codes.iter().copied().collect();
            assert_eq!(unique.len(), codes.len());
        }
    }

    mod san_extraction {
        use super::*;

        #[test]
        fn dns_and_uri_names_are_both_extracted() {
            let der = leaf_with_sans(&[
                dns_san("reporter.internal"),
                uri_san("spiffe://cluster.local/ns/prod/sa/ingest"),
            ]);

            let names = subject_alt_names(&der).expect("parseable leaf");

            assert!(names.contains(&CallerSan::dns("reporter.internal").expect("valid")));
            assert!(names.contains(
                &CallerSan::uri("spiffe://cluster.local/ns/prod/sa/ingest").expect("valid")
            ));
        }

        #[test]
        fn a_certificate_with_no_san_yields_no_names() {
            let der = leaf_with_sans(&[]);
            assert!(subject_alt_names(&der).expect("parseable leaf").is_empty());
        }

        #[test]
        fn wildcard_sans_are_dropped_not_matched() {
            let der = leaf_with_sans(&[dns_san("*.internal"), dns_san("reporter.internal")]);

            let names = subject_alt_names(&der).expect("parseable leaf");

            assert_eq!(
                names,
                vec![CallerSan::dns("reporter.internal").expect("valid")],
                "a wildcard SAN must never become a caller name"
            );
        }

        #[test]
        fn unparseable_der_is_reported_as_malformed() {
            let error = subject_alt_names(b"not a certificate").expect_err("garbage must fail");
            assert!(matches!(error, CallerAuthError::MalformedCertificate(_)));
        }
    }

    mod decision {
        use super::*;

        #[test]
        fn bearer_mode_admits_everything_it_is_given() {
            let policy = CallerAuthPolicy::bearer();
            assert_eq!(
                authorize(&policy, None, false).expect("bearer mode never refuses"),
                AuthenticatedCaller::BearerDeferred
            );
        }

        #[test]
        fn mtls_admits_an_allowlisted_caller() {
            let policy = CallerAuthPolicy::mtls(allowlist(&["reporter.internal"]));
            let der = leaf_with_sans(&[dns_san("reporter.internal")]);

            assert_eq!(
                authorize(&policy, Some(&der), false).expect("allowlisted caller"),
                AuthenticatedCaller::Certificate(
                    CallerSan::dns("reporter.internal").expect("valid")
                )
            );
        }

        #[test]
        fn mtls_admits_an_allowlisted_uri_caller() {
            let spiffe = "spiffe://cluster.local/ns/prod/sa/ingest";
            let policy = CallerAuthPolicy::mtls(allowlist(&[spiffe]));
            let der = leaf_with_sans(&[uri_san(spiffe)]);

            assert_eq!(
                authorize(&policy, Some(&der), false).expect("allowlisted caller"),
                AuthenticatedCaller::Certificate(CallerSan::uri(spiffe).expect("valid"))
            );
        }

        #[test]
        fn a_ca_issued_certificate_that_is_not_allowlisted_is_refused() {
            // The whole point of the control: this certificate is valid, and
            // chains to a CA the server trusts. It is still not authorized.
            let policy = CallerAuthPolicy::mtls(allowlist(&["reporter.internal"]));
            let der = leaf_with_sans(&[dns_san("someone-else.internal")]);

            assert_eq!(
                authorize(&policy, Some(&der), false).expect_err("must be refused"),
                CallerAuthError::NotAllowlisted(
                    CallerSan::dns("someone-else.internal").expect("valid")
                )
            );
        }

        #[test]
        fn a_dns_name_never_satisfies_a_uri_allowlist_entry() {
            let spiffe = "spiffe://cluster.local/ns/prod/sa/ingest";
            let policy = CallerAuthPolicy::mtls(allowlist(&[spiffe]));
            // A DNS SAN spelling the URI verbatim must not stand in for it.
            let der = leaf_with_sans(&[dns_san("ingest.internal")]);

            assert!(matches!(
                authorize(&policy, Some(&der), false),
                Err(CallerAuthError::NotAllowlisted(_))
            ));
        }

        #[test]
        fn mtls_refuses_a_caller_with_no_certificate() {
            let policy = CallerAuthPolicy::mtls(allowlist(&["reporter.internal"]));

            assert_eq!(
                authorize(&policy, None, true)
                    .expect_err("a bearer token is not a certificate in mtls mode"),
                CallerAuthError::NoClientCertificate
            );
        }

        #[test]
        fn a_certificate_with_no_usable_name_is_refused_as_unauthorized() {
            let policy = CallerAuthPolicy::mtls(allowlist(&["reporter.internal"]));
            let der = leaf_with_sans(&[dns_san("*.internal")]);

            let error = authorize(&policy, Some(&der), false).expect_err("must be refused");
            assert_eq!(error, CallerAuthError::NoUsableSan);
            assert_eq!(
                error.status(),
                StatusCode::FORBIDDEN,
                "the chain verified, so this is an authorization failure"
            );
        }

        #[test]
        fn mtls_or_bearer_prefers_the_certificate() {
            let policy = CallerAuthPolicy::mtls_or_bearer(allowlist(&["reporter.internal"]));
            let der = leaf_with_sans(&[dns_san("reporter.internal")]);

            assert_eq!(
                authorize(&policy, Some(&der), true).expect("allowlisted caller"),
                AuthenticatedCaller::Certificate(
                    CallerSan::dns("reporter.internal").expect("valid")
                )
            );
        }

        #[test]
        fn mtls_or_bearer_falls_back_for_a_caller_without_a_certificate() {
            let policy = CallerAuthPolicy::mtls_or_bearer(allowlist(&["reporter.internal"]));

            assert_eq!(
                authorize(&policy, None, true).expect("bearer fallback"),
                AuthenticatedCaller::BearerDeferred
            );
        }

        #[test]
        fn mtls_or_bearer_falls_back_for_a_caller_not_yet_allowlisted() {
            // The cutover case: a caller already issued a certificate, but not
            // yet added to the allowlist, must keep working on its token.
            let policy = CallerAuthPolicy::mtls_or_bearer(allowlist(&["reporter.internal"]));
            let der = leaf_with_sans(&[dns_san("not-yet-listed.internal")]);

            assert_eq!(
                authorize(&policy, Some(&der), true).expect("bearer fallback"),
                AuthenticatedCaller::BearerDeferred
            );
        }

        #[test]
        fn mtls_or_bearer_refuses_a_caller_with_neither_credential() {
            let policy = CallerAuthPolicy::mtls_or_bearer(allowlist(&["reporter.internal"]));

            let error = authorize(&policy, None, false).expect_err("must be refused");
            assert_eq!(error, CallerAuthError::NoCredential);
            assert_eq!(error.status(), StatusCode::UNAUTHORIZED);
        }

        #[test]
        fn mtls_or_bearer_reports_the_certificate_failure_when_there_is_no_token() {
            let policy = CallerAuthPolicy::mtls_or_bearer(allowlist(&["reporter.internal"]));
            let der = leaf_with_sans(&[dns_san("not-listed.internal")]);

            assert_eq!(
                authorize(&policy, Some(&der), false).expect_err("must be refused"),
                CallerAuthError::NotAllowlisted(
                    CallerSan::dns("not-listed.internal").expect("valid")
                ),
                "with no token to fall back to, the caller deserves the real reason"
            );
        }
    }

    mod listener_validation {
        use super::*;

        #[test]
        fn a_certificate_mode_needs_a_verifying_listener() {
            let policy = CallerAuthPolicy::mtls(allowlist(&["reporter.internal"]));

            assert!(
                validate_listener(&policy, ListenerClientCa::Verified, "[tls]").is_ok(),
                "a listener with a client CA satisfies the policy"
            );
            assert!(
                validate_listener(&policy, ListenerClientCa::Unknown, "[tls]").is_ok(),
                "caller-supplied TLS cannot be inspected, so it is not second-guessed"
            );
            assert!(matches!(
                validate_listener(&policy, ListenerClientCa::NotVerified, "[tls]"),
                Err(CallerAuthConfigError::MissingClientCa { .. })
            ));
            assert!(matches!(
                validate_listener(&policy, ListenerClientCa::Plaintext, "[grpc.tls]"),
                Err(CallerAuthConfigError::NoTlsListener { .. })
            ));
        }

        #[test]
        fn bearer_mode_places_no_demands_on_the_listener() {
            let policy = CallerAuthPolicy::bearer();
            for listener in [
                ListenerClientCa::Verified,
                ListenerClientCa::NotVerified,
                ListenerClientCa::Plaintext,
                ListenerClientCa::Unknown,
            ] {
                assert!(validate_listener(&policy, listener, "[tls]").is_ok());
            }
        }

        #[test]
        fn the_message_names_the_offending_section() {
            let policy = CallerAuthPolicy::mtls(allowlist(&["reporter.internal"]));
            let error = validate_listener(&policy, ListenerClientCa::NotVerified, "[grpc.tls]")
                .expect_err("must be refused");

            assert!(
                error.to_string().contains("[grpc.tls]"),
                "an operator with two listeners needs to know which one is short a CA: {error}"
            );
        }
    }

    mod bearer_detection {
        use super::*;

        fn headers(value: &str) -> http::HeaderMap {
            let mut headers = http::HeaderMap::new();
            headers.insert(
                http::header::AUTHORIZATION,
                http::HeaderValue::from_str(value).expect("valid header"),
            );
            headers
        }

        #[test]
        fn a_bearer_credential_is_detected_case_insensitively() {
            assert!(has_bearer_credential(&headers("Bearer abc")));
            assert!(has_bearer_credential(&headers("bearer abc")));
        }

        #[test]
        fn other_schemes_and_empty_credentials_are_not_bearer() {
            assert!(!has_bearer_credential(&headers("Basic abc")));
            assert!(!has_bearer_credential(&headers("Bearer ")));
            assert!(!has_bearer_credential(&headers("abc")));
            assert!(!has_bearer_credential(&http::HeaderMap::new()));
        }
    }

    /// End-to-end through a real router: the layer must find the certificate
    /// where the TLS listener actually puts it, and shape refusals per
    /// transport.
    mod layer {
        use super::*;
        use axum::extract::ConnectInfo;
        use axum::routing::get;
        use axum::Router;
        use tower::ServiceExt;

        /// Echoes back the caller identity the layer established, so a test can
        /// tell "admitted anonymously" from "admitted as this caller".
        async fn echo_identity(request: http::Request<axum::body::Body>) -> String {
            request.extensions().get::<CallerIdentity>().map_or_else(
                || "none".to_string(),
                |identity| format!("{}|{}", identity.san(), identity.waives_bearer()),
            )
        }

        fn router(layer: CallerAuthLayer) -> Router {
            Router::new()
                .route("/rpc", get(echo_identity))
                .route("/health", get(|| async { "ok" }))
                .layer(layer)
        }

        fn request(
            path: &str,
            leaf: Option<Vec<u8>>,
            bearer: bool,
        ) -> http::Request<axum::body::Body> {
            let chain = leaf.map(|der| vec![der.into()]).unwrap_or_default();
            let connect_info = crate::tls::TlsConnectInfo::for_test(
                "203.0.113.10:44300".parse().expect("addr"),
                chain,
            );

            let mut builder = http::Request::builder().uri(path);
            if bearer {
                builder = builder.header(http::header::AUTHORIZATION, "Bearer opaque-token");
            }
            let mut request = builder
                .body(axum::body::Body::empty())
                .expect("valid request");
            request.extensions_mut().insert(ConnectInfo(connect_info));
            request
        }

        async fn body_of(response: http::Response<axum::body::Body>) -> String {
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body");
            String::from_utf8(bytes.to_vec()).expect("utf-8 body")
        }

        #[tokio::test]
        async fn an_allowlisted_caller_reaches_the_handler_as_itself() {
            let policy = CallerAuthPolicy::mtls(allowlist(&["reporter.internal"]));
            let der = leaf_with_sans(&[dns_san("reporter.internal")]);

            let response = router(CallerAuthLayer::http(policy))
                .oneshot(request("/rpc", Some(der), false))
                .await
                .expect("router responds");

            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                body_of(response).await,
                "DNS:reporter.internal|false",
                "under mtls the certificate is an additional requirement, not a token substitute"
            );
        }

        #[tokio::test]
        async fn mtls_or_bearer_marks_the_certificate_as_standing_in_for_a_token() {
            let policy = CallerAuthPolicy::mtls_or_bearer(allowlist(&["reporter.internal"]));
            let der = leaf_with_sans(&[dns_san("reporter.internal")]);

            let response = router(CallerAuthLayer::http(policy))
                .oneshot(request("/rpc", Some(der), false))
                .await
                .expect("router responds");

            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(body_of(response).await, "DNS:reporter.internal|true");
        }

        #[tokio::test]
        async fn a_caller_admitted_on_a_token_carries_no_certificate_identity() {
            let policy = CallerAuthPolicy::mtls_or_bearer(allowlist(&["reporter.internal"]));

            let response = router(CallerAuthLayer::http(policy))
                .oneshot(request("/rpc", None, true))
                .await
                .expect("router responds");

            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                body_of(response).await,
                "none",
                "a bearer caller must not appear to have proved a certificate identity"
            );
        }

        #[tokio::test]
        async fn no_certificate_is_401_and_an_unlisted_certificate_is_403() {
            let policy = CallerAuthPolicy::mtls(allowlist(&["reporter.internal"]));

            let unauthenticated = router(CallerAuthLayer::http(policy.clone()))
                .oneshot(request("/rpc", None, false))
                .await
                .expect("router responds");
            assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
            assert!(body_of(unauthenticated)
                .await
                .contains("CLIENT_CERT_REQUIRED"));

            let der = leaf_with_sans(&[dns_san("someone-else.internal")]);
            let unauthorized = router(CallerAuthLayer::http(policy))
                .oneshot(request("/rpc", Some(der), false))
                .await
                .expect("router responds");
            assert_eq!(unauthorized.status(), StatusCode::FORBIDDEN);
            assert!(body_of(unauthorized).await.contains("CALLER_NOT_ALLOWED"));
        }

        #[tokio::test]
        async fn infrastructure_probes_are_exempt() {
            let policy = CallerAuthPolicy::mtls(allowlist(&["reporter.internal"]));

            let response = router(CallerAuthLayer::http(policy))
                .oneshot(request("/health", None, false))
                .await
                .expect("router responds");

            assert_eq!(
                response.status(),
                StatusCode::OK,
                "a kubelet presents no client certificate"
            );
        }

        #[tokio::test]
        async fn a_route_that_merely_starts_with_a_probe_path_is_not_exempt() {
            let policy = CallerAuthPolicy::mtls(allowlist(&["reporter.internal"]));
            let app = Router::new()
                .route("/health-admin", get(|| async { "secrets" }))
                .layer(CallerAuthLayer::http(policy));

            let response = app
                .oneshot(request("/health-admin", None, false))
                .await
                .expect("router responds");

            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "the probe exemption must not extend to routes that share its prefix"
            );
        }

        #[tokio::test]
        async fn configured_public_paths_are_exempt() {
            let policy = CallerAuthPolicy::mtls(allowlist(&["reporter.internal"]))
                .with_public_paths(["/rpc"]);

            let response = router(CallerAuthLayer::http(policy))
                .oneshot(request("/rpc", None, false))
                .await
                .expect("router responds");

            assert_eq!(response.status(), StatusCode::OK);
        }

        #[tokio::test]
        async fn a_request_without_tls_connect_info_is_refused() {
            // A TLS-terminating proxy in front of a plaintext listener leaves no
            // certificate to read. Failing closed is the only safe direction.
            let policy = CallerAuthPolicy::mtls(allowlist(&["reporter.internal"]));
            let request = http::Request::builder()
                .uri("/rpc")
                .body(axum::body::Body::empty())
                .expect("valid request");

            let response = router(CallerAuthLayer::http(policy))
                .oneshot(request)
                .await
                .expect("router responds");

            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }

        #[cfg(feature = "grpc")]
        #[tokio::test]
        async fn grpc_refusals_are_grpc_statuses() {
            let policy = CallerAuthPolicy::mtls(allowlist(&["reporter.internal"]));

            let unauthenticated = router(CallerAuthLayer::grpc(policy.clone()))
                .oneshot(request("/rpc", None, false))
                .await
                .expect("router responds");
            assert_eq!(
                unauthenticated
                    .headers()
                    .get("grpc-status")
                    .and_then(|v| v.to_str().ok()),
                Some("16"),
                "nothing proven is UNAUTHENTICATED"
            );

            let der = leaf_with_sans(&[dns_san("someone-else.internal")]);
            let unauthorized = router(CallerAuthLayer::grpc(policy))
                .oneshot(request("/rpc", Some(der), false))
                .await
                .expect("router responds");
            assert_eq!(
                unauthorized
                    .headers()
                    .get("grpc-status")
                    .and_then(|v| v.to_str().ok()),
                Some("7"),
                "an identity proven but not allowlisted is PERMISSION_DENIED"
            );
        }

        #[cfg(feature = "grpc")]
        #[tokio::test]
        async fn grpc_infrastructure_services_are_exempt() {
            let policy = CallerAuthPolicy::mtls(allowlist(&["reporter.internal"]));
            let app = Router::new()
                .route("/grpc.health.v1.Health/Check", get(|| async { "ok" }))
                .layer(CallerAuthLayer::grpc(policy));

            let response = app
                .oneshot(request("/grpc.health.v1.Health/Check", None, false))
                .await
                .expect("router responds");

            assert_eq!(response.status(), StatusCode::OK);
        }
    }
}
