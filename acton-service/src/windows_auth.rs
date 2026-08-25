//! Windows/Active Directory identity forwarded by a mutually authenticated proxy.
//!
//! This module does not trust identity headers on their own. A request must
//! first carry a [`crate::caller_auth::CallerIdentity`] created
//! from a verified, allowlisted client certificate, and that certificate must
//! identify one of the explicitly configured Windows-auth proxies.
//! The proxy is responsible for completing and validating the Negotiate
//! exchange before asserting a principal.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use axum::extract::{Request, State};
use axum::http::header::HeaderName;
use axum::middleware::Next;
use axum::response::Response;
use serde::{Deserialize, Serialize};

use crate::caller_auth::{CallerAllowlist, CallerIdentity};
use crate::error::{Error, Result};
use crate::middleware::token::Claims;

const DEFAULT_IDENTITY_HEADER: &str = "x-windows-user";
const DEFAULT_GROUPS_HEADER: &str = "x-windows-groups";
const MAX_PRINCIPAL_LEN: usize = 512;
const MAX_GROUP_LEN: usize = 512;
const MAX_GROUPS: usize = 256;

/// Configuration for Windows identities forwarded by an mTLS proxy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsAuthConfig {
    /// Certificate SANs of proxies allowed to assert Windows identities.
    pub trusted_proxies: Vec<String>,
    /// Header containing `DOMAIN\user`, `user@domain`, or a local username.
    #[serde(default = "default_identity_header")]
    pub identity_header: String,
    /// Optional comma-separated Active Directory group header.
    #[serde(default = "default_groups_header")]
    pub groups_header: String,
    /// Exact AD-group-to-application-role mappings.
    #[serde(default)]
    pub group_roles: BTreeMap<String, String>,
}

impl WindowsAuthConfig {
    /// Validate the configuration and build middleware state.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty/invalid proxy allowlist, invalid header
    /// names, or malformed group-to-role mappings.
    pub fn to_layer(&self) -> std::result::Result<WindowsAuthLayer, WindowsAuthConfigError> {
        let trusted_proxies = CallerAllowlist::from_entries(&self.trusted_proxies)
            .map_err(|error| WindowsAuthConfigError::TrustedProxy(error.to_string()))?;
        let identity_header = parse_header_name("identity_header", &self.identity_header)?;
        let groups_header = parse_header_name("groups_header", &self.groups_header)?;

        for (group, role) in &self.group_roles {
            WindowsGroup::new(group.clone())
                .map_err(|error| WindowsAuthConfigError::InvalidGroupMapping(error.to_string()))?;
            validate_role(role)?;
        }

        Ok(WindowsAuthLayer {
            trusted_proxies,
            identity_header,
            groups_header,
            group_roles: self.group_roles.clone(),
        })
    }
}

fn default_identity_header() -> String {
    DEFAULT_IDENTITY_HEADER.to_owned()
}

fn default_groups_header() -> String {
    DEFAULT_GROUPS_HEADER.to_owned()
}

fn parse_header_name(
    field: &'static str,
    value: &str,
) -> std::result::Result<HeaderName, WindowsAuthConfigError> {
    value
        .parse()
        .map_err(|_| WindowsAuthConfigError::InvalidHeader {
            field,
            value: value.to_owned(),
        })
}

fn validate_role(role: &str) -> std::result::Result<(), WindowsAuthConfigError> {
    if role.is_empty() || role.chars().any(char::is_whitespace) {
        return Err(WindowsAuthConfigError::InvalidRole(role.to_owned()));
    }
    Ok(())
}

/// Invalid trusted-proxy Windows authentication configuration.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WindowsAuthConfigError {
    /// The trusted proxy allowlist is invalid.
    #[error("invalid Windows-auth trusted proxy allowlist: {0}")]
    TrustedProxy(String),
    /// A configured header name is invalid.
    #[error("[caller_auth.windows].{field} '{value}' is not a valid HTTP header name")]
    InvalidHeader {
        /// Configuration field.
        field: &'static str,
        /// Invalid value.
        value: String,
    },
    /// A group mapping contains an invalid group.
    #[error("invalid Windows-auth group mapping: {0}")]
    InvalidGroupMapping(String),
    /// A role is empty or contains whitespace.
    #[error("Windows-auth role '{0}' must be non-empty and contain no whitespace")]
    InvalidRole(String),
    /// The outer mode cannot safely replace bearer auth with a proxy identity.
    #[error("[caller_auth.windows] requires [caller_auth].mode = 'mtls-or-bearer'")]
    RequiresMtlsOrBearer,
    /// A trusted proxy is absent from the outer certificate allowlist.
    #[error("Windows-auth proxy '{0}' is not present in [caller_auth].allowlist")]
    ProxyNotAllowed(String),
}

/// A validated Windows principal name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowsPrincipal(String);

impl WindowsPrincipal {
    /// Validate a Windows principal.
    ///
    /// # Errors
    ///
    /// Rejects empty, overlong, surrounding-whitespace, control-character,
    /// comma, and path-like values.
    pub fn new(value: impl Into<String>) -> std::result::Result<Self, WindowsIdentityError> {
        let value = value.into();
        validate_identity_value("principal", &value, MAX_PRINCIPAL_LEN)?;
        if value.contains(',') || value.contains('/') || value == "." || value == ".." {
            return Err(WindowsIdentityError::InvalidPrincipal(value));
        }
        let valid_qualified_form = if value.contains('\\') {
            let mut parts = value.split('\\');
            matches!((parts.next(), parts.next(), parts.next()), (Some(domain), Some(user), None) if !domain.is_empty() && !user.is_empty())
        } else if value.contains('@') {
            let mut parts = value.split('@');
            matches!((parts.next(), parts.next(), parts.next()), (Some(user), Some(domain), None) if !user.is_empty() && !domain.is_empty())
        } else {
            true
        };
        if !valid_qualified_form {
            return Err(WindowsIdentityError::InvalidPrincipal(value));
        }
        Ok(Self(value))
    }

    /// The exact principal asserted by the trusted proxy.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WindowsPrincipal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A validated Active Directory group name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowsGroup(String);

impl WindowsGroup {
    /// Validate an Active Directory group name.
    ///
    /// # Errors
    ///
    /// Rejects empty, overlong, surrounding-whitespace, control-character,
    /// and comma-containing values.
    pub fn new(value: impl Into<String>) -> std::result::Result<Self, WindowsIdentityError> {
        let value = value.into();
        validate_identity_value("group", &value, MAX_GROUP_LEN)?;
        if value.contains(',') {
            return Err(WindowsIdentityError::InvalidGroup(value));
        }
        Ok(Self(value))
    }

    /// The exact group name asserted by the trusted proxy.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WindowsGroup {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

fn validate_identity_value(
    kind: &'static str,
    value: &str,
    maximum: usize,
) -> std::result::Result<(), WindowsIdentityError> {
    if value.is_empty() {
        return Err(WindowsIdentityError::Empty(kind));
    }
    if value.len() > maximum {
        return Err(WindowsIdentityError::TooLong {
            kind,
            length: value.len(),
            maximum,
        });
    }
    if value.trim() != value || value.chars().any(char::is_control) {
        return Err(WindowsIdentityError::IllegalCharacter(kind));
    }
    Ok(())
}

/// A Windows identity established by a trusted proxy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsIdentity {
    principal: WindowsPrincipal,
    groups: Vec<WindowsGroup>,
}

impl WindowsIdentity {
    /// The authenticated domain principal.
    #[must_use]
    pub fn principal(&self) -> &WindowsPrincipal {
        &self.principal
    }

    /// Active Directory groups asserted for the principal.
    #[must_use]
    pub fn groups(&self) -> &[WindowsGroup] {
        &self.groups
    }
}

/// Invalid identity data received from a trusted proxy.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WindowsIdentityError {
    /// A required value was empty.
    #[error("Windows {0} is empty")]
    Empty(&'static str),
    /// A value exceeded its defensive size limit.
    #[error("Windows {kind} is {length} bytes, over the {maximum}-byte limit")]
    TooLong {
        /// Kind of identity value.
        kind: &'static str,
        /// Actual length.
        length: usize,
        /// Maximum accepted length.
        maximum: usize,
    },
    /// A value contains surrounding whitespace or control characters.
    #[error("Windows {0} contains illegal whitespace or control characters")]
    IllegalCharacter(&'static str),
    /// A principal has an ambiguous or unsafe shape.
    #[error("Windows principal '{0}' has an invalid form")]
    InvalidPrincipal(String),
    /// A group contains the configured delimiter.
    #[error("Windows group '{0}' contains a comma")]
    InvalidGroup(String),
    /// Too many groups were forwarded.
    #[error("Windows identity has {count} groups, over the {maximum}-group limit")]
    TooManyGroups {
        /// Actual group count.
        count: usize,
        /// Maximum accepted count.
        maximum: usize,
    },
}

/// Middleware state for trusted-proxy Windows authentication.
#[derive(Debug, Clone)]
pub struct WindowsAuthLayer {
    trusted_proxies: CallerAllowlist,
    identity_header: HeaderName,
    groups_header: HeaderName,
    group_roles: BTreeMap<String, String>,
}

impl WindowsAuthLayer {
    /// Check that this layer is safely nested inside the caller-auth policy.
    ///
    /// # Errors
    ///
    /// Windows proxy authentication requires `mtls-or-bearer`, and every
    /// trusted proxy must also occur in the outer certificate allowlist.
    pub fn validate_caller_policy(
        &self,
        policy: &crate::caller_auth::CallerAuthPolicy,
    ) -> std::result::Result<(), WindowsAuthConfigError> {
        if policy.mode() != crate::caller_auth::CallerAuthMode::MtlsOrBearer {
            return Err(WindowsAuthConfigError::RequiresMtlsOrBearer);
        }
        let Some(outer) = policy.allowlist() else {
            return Err(WindowsAuthConfigError::RequiresMtlsOrBearer);
        };
        for proxy in &self.trusted_proxies {
            if !outer.iter().any(|allowed| allowed == proxy) {
                return Err(WindowsAuthConfigError::ProxyNotAllowed(proxy.to_string()));
            }
        }
        Ok(())
    }

    /// Authenticate a forwarded Windows identity when the request came from a
    /// configured, mutually authenticated proxy.
    pub async fn middleware(
        State(auth): State<Self>,
        mut request: Request,
        next: Next,
    ) -> Result<Response> {
        let trusted = request
            .extensions()
            .get::<CallerIdentity>()
            .is_some_and(|caller| auth.trusted_proxies.iter().any(|san| san == caller.san()));

        if !trusted {
            // Never let application code accidentally consume an unverified
            // identity assertion, even on a valid bearer-authenticated request.
            auth.strip_identity_headers(request.headers_mut());
            return Ok(next.run(request).await);
        }

        let identity = auth.identity_from_headers(request.headers())?;
        let claims = auth.claims(&identity);

        auth.strip_identity_headers(request.headers_mut());
        request.extensions_mut().insert(claims);
        request.extensions_mut().insert(identity);

        Ok(next.run(request).await)
    }

    fn identity_from_headers(
        &self,
        headers: &http::HeaderMap,
    ) -> std::result::Result<WindowsIdentity, Error> {
        let principal = headers
            .get(&self.identity_header)
            .ok_or_else(|| {
                Error::Unauthorized(
                    "trusted Windows-auth proxy omitted the identity header".to_owned(),
                )
            })?
            .to_str()
            .map_err(|_| {
                Error::Unauthorized("Windows identity header is not valid text".to_owned())
            })?;
        let principal = WindowsPrincipal::new(principal.to_owned())
            .map_err(|error| Error::Unauthorized(error.to_string()))?;

        let groups = match headers.get(&self.groups_header) {
            None => Vec::new(),
            Some(value) => {
                let value = value.to_str().map_err(|_| {
                    Error::Unauthorized("Windows groups header is not valid text".to_owned())
                })?;
                parse_groups(value).map_err(|error| Error::Unauthorized(error.to_string()))?
            }
        };

        Ok(WindowsIdentity { principal, groups })
    }

    fn claims(&self, identity: &WindowsIdentity) -> Claims {
        let roles: BTreeSet<String> = identity
            .groups
            .iter()
            .filter_map(|group| self.group_roles.get(group.as_str()).cloned())
            .collect();
        let group_names: Vec<&str> = identity.groups.iter().map(WindowsGroup::as_str).collect();
        let mut custom = HashMap::new();
        custom.insert(
            "authentication_method".to_owned(),
            serde_json::Value::String("windows".to_owned()),
        );
        custom.insert("windows_groups".to_owned(), serde_json::json!(group_names));

        Claims {
            sub: format!("windows:{}", identity.principal),
            email: identity
                .principal
                .as_str()
                .contains('@')
                .then(|| identity.principal.to_string()),
            username: Some(identity.principal.to_string()),
            roles: roles.into_iter().collect(),
            perms: Vec::new(),
            exp: i64::MAX,
            iat: Some(chrono::Utc::now().timestamp()),
            jti: None,
            iss: Some("windows-auth-proxy".to_owned()),
            aud: None,
            custom,
        }
    }

    fn strip_identity_headers(&self, headers: &mut http::HeaderMap) {
        headers.remove(&self.identity_header);
        headers.remove(&self.groups_header);
    }
}

fn parse_groups(value: &str) -> std::result::Result<Vec<WindowsGroup>, WindowsIdentityError> {
    if value.is_empty() {
        return Ok(Vec::new());
    }
    let entries: Vec<&str> = value.split(',').collect();
    if entries.len() > MAX_GROUPS {
        return Err(WindowsIdentityError::TooManyGroups {
            count: entries.len(),
            maximum: MAX_GROUPS,
        });
    }

    entries
        .into_iter()
        .map(|entry| WindowsGroup::new(entry.trim().to_owned()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn principal_accepts_domain_and_upn_forms() {
        assert!(WindowsPrincipal::new("CONTOSO\\alice").is_ok());
        assert!(WindowsPrincipal::new("alice@contoso.example").is_ok());
    }

    #[test]
    fn principal_rejects_ambiguous_values() {
        assert!(WindowsPrincipal::new(" alice").is_err());
        assert!(WindowsPrincipal::new("alice,bob").is_err());
        assert!(WindowsPrincipal::new("../alice").is_err());
        assert!(WindowsPrincipal::new("CONTOSO\\").is_err());
        assert!(WindowsPrincipal::new("alice@@contoso.example").is_err());
    }

    #[test]
    fn groups_are_trimmed_and_deduplicated_by_role_mapping() {
        let groups = parse_groups("CONTOSO\\Admins, CONTOSO\\Readers");
        assert_eq!(groups.map(|items| items.len()), Ok(2));
    }

    #[test]
    fn config_rejects_invalid_header_names() {
        let config = WindowsAuthConfig {
            trusted_proxies: vec!["proxy.internal".to_owned()],
            identity_header: "not a header".to_owned(),
            groups_header: default_groups_header(),
            group_roles: BTreeMap::new(),
        };
        assert!(matches!(
            config.to_layer(),
            Err(WindowsAuthConfigError::InvalidHeader { .. })
        ));
    }

    #[test]
    fn proxy_must_be_in_outer_mtls_or_bearer_allowlist() {
        let config = WindowsAuthConfig {
            trusted_proxies: vec!["proxy.internal".to_owned()],
            identity_header: default_identity_header(),
            groups_header: default_groups_header(),
            group_roles: BTreeMap::new(),
        };
        let layer = config.to_layer().expect("valid Windows auth config");
        let allowed =
            CallerAllowlist::from_entries(["proxy.internal"]).expect("valid outer allowlist");
        assert!(layer
            .validate_caller_policy(&crate::caller_auth::CallerAuthPolicy::mtls_or_bearer(
                allowed
            ))
            .is_ok());

        let other =
            CallerAllowlist::from_entries(["other.internal"]).expect("valid outer allowlist");
        assert!(matches!(
            layer.validate_caller_policy(&crate::caller_auth::CallerAuthPolicy::mtls_or_bearer(
                other
            )),
            Err(WindowsAuthConfigError::ProxyNotAllowed(_))
        ));
    }

    #[test]
    fn windows_groups_map_to_unified_claim_roles() {
        let mut group_roles = BTreeMap::new();
        group_roles.insert("CONTOSO\\Platform Admins".to_owned(), "admin".to_owned());
        group_roles.insert("CONTOSO\\Readers".to_owned(), "reader".to_owned());
        let layer = WindowsAuthConfig {
            trusted_proxies: vec!["proxy.internal".to_owned()],
            identity_header: default_identity_header(),
            groups_header: default_groups_header(),
            group_roles,
        }
        .to_layer()
        .expect("valid Windows auth config");
        let identity = WindowsIdentity {
            principal: WindowsPrincipal::new("alice@contoso.example").expect("valid principal"),
            groups: parse_groups("CONTOSO\\Readers,CONTOSO\\Platform Admins,CONTOSO\\Other")
                .expect("valid groups"),
        };

        let claims = layer.claims(&identity);
        assert_eq!(claims.sub, "windows:alice@contoso.example");
        assert_eq!(claims.email.as_deref(), Some("alice@contoso.example"));
        assert_eq!(claims.roles, ["admin", "reader"]);
        assert_eq!(
            claims
                .custom_claim("authentication_method")
                .and_then(serde_json::Value::as_str),
            Some("windows")
        );
    }

    #[test]
    fn missing_identity_from_a_trusted_proxy_fails_closed() {
        let layer = WindowsAuthConfig {
            trusted_proxies: vec!["proxy.internal".to_owned()],
            identity_header: default_identity_header(),
            groups_header: default_groups_header(),
            group_roles: BTreeMap::new(),
        }
        .to_layer()
        .expect("valid Windows auth config");

        assert!(matches!(
            layer.identity_from_headers(&http::HeaderMap::new()),
            Err(Error::Unauthorized(_))
        ));
    }

    #[test]
    fn forwarded_identity_headers_are_removed_before_application_code() {
        let layer = WindowsAuthConfig {
            trusted_proxies: vec!["proxy.internal".to_owned()],
            identity_header: default_identity_header(),
            groups_header: default_groups_header(),
            group_roles: BTreeMap::new(),
        }
        .to_layer()
        .expect("valid Windows auth config");
        let mut headers = http::HeaderMap::new();
        headers.insert(
            DEFAULT_IDENTITY_HEADER,
            "CONTOSO\\alice".parse().expect("header"),
        );
        headers.insert(
            DEFAULT_GROUPS_HEADER,
            "CONTOSO\\Readers".parse().expect("header"),
        );

        layer.strip_identity_headers(&mut headers);

        assert!(!headers.contains_key(DEFAULT_IDENTITY_HEADER));
        assert!(!headers.contains_key(DEFAULT_GROUPS_HEADER));
    }
}
