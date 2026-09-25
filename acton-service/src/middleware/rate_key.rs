//! Who a rate-limited request is counted as.
//!
//! Both limiters, the in-memory [`GovernorRateLimit`] and the Redis-backed
//! [`RateLimit`], ask one [`RateClassifier`] which bucket a request belongs to
//! before they count it. The classifier answers with a [`RateKey`]:
//!
//! - [`RateKey::Exempt`]: the request is not counted at all;
//! - [`RateKey::Key`]: it is counted in the named bucket, under the per-user or
//!   per-client quota its [`RateClass`] selects;
//! - [`RateKey::Anonymous`]: it is counted as an anonymous caller from its
//!   client address.
//!
//! [`ClaimsClassifier`] is the default and keeps the limiters' long-standing
//! behavior: token claims select a user or client bucket, and a request without
//! claims is anonymous. A service whose callers identify themselves some other
//! way (a verified client certificate, an API credential its own middleware
//! checks later in the stack) installs its own classifier with
//! [`ServiceBuilder::with_rate_classifier`], or with `with_classifier` on either
//! limiter when it wires the middleware by hand.
//!
//! [`RateLimitConfig::exempt_paths`] is applied before any classifier runs, so
//! probes stay exempt whatever the classifier decides.
//!
//! Where a classifier runs is where its limiter sits. The governor limiter
//! that [`ServiceBuilder`](crate::service_builder::ServiceBuilder) applies
//! itself runs after token authentication and Cedar authorization, just before
//! the handler:
//!
//! - the [`Claims`] a classifier reads there were verified by token
//!   authentication;
//! - a request that authentication or Cedar rejected never reaches the
//!   limiter, so it is not counted.
//!
//! A limiter wired by hand runs wherever it is layered, and sees claims only
//! if token authentication ran before it.
//!
//! Either way a classifier runs on every request that reaches its limiter, so
//! it must be cheap. It must also key a request by an identity only once that
//! identity is verified, by token authentication ahead of it or by the
//! classifier itself for a credential no earlier layer checks: keying by an
//! unverified credential lets a caller open a fresh bucket per request by
//! sending a fresh made-up value.
//!
//! [`GovernorRateLimit`]: crate::middleware::governor::GovernorRateLimit
//! [`RateLimit`]: crate::middleware::RateLimit
//! [`ServiceBuilder::with_rate_classifier`]: crate::service_builder::ServiceBuilder::with_rate_classifier
//! [`RateLimitConfig::exempt_paths`]: crate::config::RateLimitConfig::exempt_paths

use std::net::IpAddr;
#[cfg(any(feature = "governor", feature = "cache"))]
use std::{fmt, sync::Arc};

use axum::http::{Extensions, HeaderMap, Method};

use super::token::Claims;

/// Which configured quota a keyed bucket is held to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RateClass {
    /// [`RateLimitConfig::per_user_rpm`](crate::config::RateLimitConfig::per_user_rpm).
    User,
    /// [`RateLimitConfig::per_client_rpm`](crate::config::RateLimitConfig::per_client_rpm).
    Client,
}

/// How the limiters count one request.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RateKey {
    /// Not counted, and never refused by a limiter.
    Exempt,
    /// Counted in the bucket named `id`, under the quota of `class`.
    ///
    /// Requests with equal ids share one bucket. The id is the caller's
    /// identity as the classifier verified it, namespaced however the
    /// classifier likes (the default uses `user:…`, `client:…` and
    /// `unknown:…`).
    Key {
        /// The bucket's name.
        id: String,
        /// The quota the bucket is held to.
        class: RateClass,
    },
    /// Counted as an anonymous caller from this client address, or from an
    /// unknown address when none resolved.
    Anonymous(Option<IpAddr>),
}

impl RateKey {
    /// A bucket held to the per-user quota.
    pub fn user(id: impl Into<String>) -> Self {
        Self::Key {
            id: id.into(),
            class: RateClass::User,
        }
    }

    /// A bucket held to the per-client quota.
    pub fn client(id: impl Into<String>) -> Self {
        Self::Key {
            id: id.into(),
            class: RateClass::Client,
        }
    }
}

/// What a [`RateClassifier`] may read about a request.
///
/// Borrowed from the request as the limiter sees it. The client address is
/// resolved by the limiter under its own
/// [`trust_forwarded_headers`](crate::config::RateLimitConfig::trust_forwarded_headers)
/// policy, so a classifier that falls back to [`RateKey::Anonymous`] can use it
/// as is.
#[derive(Debug, Clone, Copy)]
pub struct RateRequest<'a> {
    method: &'a Method,
    path: &'a str,
    headers: &'a HeaderMap,
    extensions: &'a Extensions,
    client_ip: Option<IpAddr>,
}

impl<'a> RateRequest<'a> {
    /// Describes a request for a classifier.
    pub fn new(
        method: &'a Method,
        path: &'a str,
        headers: &'a HeaderMap,
        extensions: &'a Extensions,
        client_ip: Option<IpAddr>,
    ) -> Self {
        Self {
            method,
            path,
            headers,
            extensions,
            client_ip,
        }
    }

    /// The request method.
    pub fn method(&self) -> &'a Method {
        self.method
    }

    /// The full request path, before any nested router strips a prefix.
    pub fn path(&self) -> &'a str {
        self.path
    }

    /// The request headers.
    pub fn headers(&self) -> &'a HeaderMap {
        self.headers
    }

    /// The request extensions: connect info (`ConnectInfo<SocketAddr>`, or
    /// `ConnectInfo<TlsConnectInfo>` on a directly terminated TLS listener,
    /// which carries any verified client certificate chain), and the
    /// [`Claims`] token authentication attached, if it ran first.
    pub fn extensions(&self) -> &'a Extensions {
        self.extensions
    }

    /// The token claims attached to the request, if any.
    pub fn claims(&self) -> Option<&'a Claims> {
        self.extensions.get::<Claims>()
    }

    /// The client address the limiter resolved, if any.
    pub fn client_ip(&self) -> Option<IpAddr> {
        self.client_ip
    }
}

/// Decides which bucket a request is counted in. See the [module
/// documentation](self).
///
/// Any `Fn(&RateRequest<'_>) -> RateKey` that is `Send + Sync + 'static` is a
/// classifier.
pub trait RateClassifier: Send + Sync + 'static {
    /// The bucket `request` is counted in.
    fn classify(&self, request: &RateRequest<'_>) -> RateKey;
}

impl<F> RateClassifier for F
where
    F: Fn(&RateRequest<'_>) -> RateKey + Send + Sync + 'static,
{
    fn classify(&self, request: &RateRequest<'_>) -> RateKey {
        self(request)
    }
}

/// The default classifier: token claims pick the bucket, and a request without
/// claims is anonymous. See [`claims_rate_key`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClaimsClassifier;

impl RateClassifier for ClaimsClassifier {
    fn classify(&self, request: &RateRequest<'_>) -> RateKey {
        claims_rate_key(request.claims(), request.client_ip())
    }
}

/// The default classification, as a pure function.
///
/// - A `user:` subject is counted per user (`user:<sub>`, per-user quota).
/// - A `client:` subject is counted per client (`client:<sub>`, per-client
///   quota).
/// - Any other subject is counted under the per-user quota as
///   `unknown:<sub>`.
/// - No claims: anonymous, from `client_ip`.
pub fn claims_rate_key(claims: Option<&Claims>, client_ip: Option<IpAddr>) -> RateKey {
    match claims {
        Some(claims) if claims.is_user() => RateKey::user(format!("user:{}", claims.sub)),
        Some(claims) if claims.is_client() => RateKey::client(format!("client:{}", claims.sub)),
        Some(claims) => RateKey::user(format!("unknown:{}", claims.sub)),
        None => RateKey::Anonymous(client_ip),
    }
}

/// A shared classifier, as the limiters hold it.
#[cfg(any(feature = "governor", feature = "cache"))]
#[derive(Clone)]
pub(crate) struct SharedClassifier(Arc<dyn RateClassifier>);

#[cfg(any(feature = "governor", feature = "cache"))]
impl SharedClassifier {
    pub(crate) fn new(classifier: impl RateClassifier) -> Self {
        Self(Arc::new(classifier))
    }

    #[cfg(feature = "governor")]
    pub(crate) fn from_arc(classifier: Arc<dyn RateClassifier>) -> Self {
        Self(classifier)
    }

    pub(crate) fn classify(&self, request: &RateRequest<'_>) -> RateKey {
        self.0.classify(request)
    }
}

#[cfg(any(feature = "governor", feature = "cache"))]
impl Default for SharedClassifier {
    fn default() -> Self {
        Self::new(ClaimsClassifier)
    }
}

#[cfg(any(feature = "governor", feature = "cache"))]
impl fmt::Debug for SharedClassifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SharedClassifier")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(sub: &str) -> Claims {
        Claims {
            sub: sub.to_string(),
            email: None,
            username: None,
            roles: Vec::new(),
            perms: Vec::new(),
            exp: 0,
            iat: None,
            jti: None,
            iss: None,
            aud: None,
            custom: Default::default(),
        }
    }

    const IP: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(203, 0, 113, 7));

    #[test]
    fn user_claims_key_a_per_user_bucket() {
        assert_eq!(
            claims_rate_key(Some(&claims("user:alice")), Some(IP)),
            RateKey::user("user:user:alice")
        );
    }

    #[test]
    fn client_claims_key_a_per_client_bucket() {
        assert_eq!(
            claims_rate_key(Some(&claims("client:svc")), Some(IP)),
            RateKey::client("client:client:svc")
        );
    }

    #[test]
    fn other_claims_key_an_unknown_bucket_at_the_user_quota() {
        assert_eq!(
            claims_rate_key(Some(&claims("robot")), Some(IP)),
            RateKey::user("unknown:robot")
        );
    }

    #[test]
    fn no_claims_is_anonymous_from_the_client_address() {
        assert_eq!(
            claims_rate_key(None, Some(IP)),
            RateKey::Anonymous(Some(IP))
        );
        assert_eq!(claims_rate_key(None, None), RateKey::Anonymous(None));
    }

    #[test]
    fn the_default_classifier_reads_claims_from_the_extensions() {
        let method = Method::GET;
        let headers = HeaderMap::new();
        let mut extensions = Extensions::new();
        let anonymous = RateRequest::new(&method, "/x", &headers, &extensions, Some(IP));
        assert_eq!(
            ClaimsClassifier.classify(&anonymous),
            RateKey::Anonymous(Some(IP))
        );

        extensions.insert(claims("user:alice"));
        let signed_in = RateRequest::new(&method, "/x", &headers, &extensions, Some(IP));
        assert_eq!(
            ClaimsClassifier.classify(&signed_in),
            RateKey::user("user:user:alice")
        );
    }

    #[test]
    fn a_closure_is_a_classifier() {
        fn admin_exempt(request: &RateRequest<'_>) -> RateKey {
            if request.path() == "/admin" {
                RateKey::Exempt
            } else {
                RateKey::Anonymous(request.client_ip())
            }
        }
        let classifier: Box<dyn RateClassifier> = Box::new(admin_exempt);
        let method = Method::GET;
        let headers = HeaderMap::new();
        let extensions = Extensions::new();
        assert_eq!(
            classifier.classify(&RateRequest::new(
                &method,
                "/admin",
                &headers,
                &extensions,
                None
            )),
            RateKey::Exempt
        );
        assert_eq!(
            classifier.classify(&RateRequest::new(
                &method,
                "/other",
                &headers,
                &extensions,
                Some(IP)
            )),
            RateKey::Anonymous(Some(IP))
        );
    }
}
