//! Server-side state for the SAML login flow.
//!
//! Two pieces of state outlive a single request:
//!
//! - the **pending login**: the `AuthnRequest` this service issued, kept until
//!   the identity provider's response arrives so `InResponseTo` can be paired
//!   and unsolicited responses refused;
//! - the **replay set**: response and assertion identifiers already accepted,
//!   kept until the assertion would have expired anyway, so a captured
//!   response cannot be presented twice.
//!
//! Both are behind traits so a single-process deployment can use
//! [`InMemorySamlStore`] while a horizontally scaled one shares
//! [`RedisSamlStore`] (feature `cache`), the same split as
//! [`OAuthStateManager`](crate::auth::OAuthStateManager).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::Error;

/// A started login awaiting the identity provider's response.
///
/// This is the persistable form of `saml_rs::PendingAuthnRequest`; it carries
/// no secrets and is safe to serialize into a session or cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingLogin {
    /// The `AuthnRequest/@ID`, which the response must echo in `InResponseTo`.
    pub request_id: String,
    /// The `RelayState` sent with the request, if any.
    pub relay_state: PendingRelayState,
    /// The identity provider the request was addressed to.
    pub idp_entity_id: String,
    /// Binding the response is expected on (`post` or `simple-sign`).
    pub response_binding: String,
    /// Binding the request went out on, when known.
    pub request_binding: Option<String>,
    /// Assertion Consumer Service URL the response must be delivered to.
    pub acs_url: String,
    /// Binding of the ACS endpoint.
    pub acs_binding: String,
    /// ACS index advertised in metadata, if any.
    pub acs_index: Option<u16>,
    /// Whether the ACS is the metadata default.
    pub acs_is_default: bool,
    /// RFC 3339 instant the request was issued.
    pub issued_at: Option<String>,
    /// RFC 3339 instant after which the request is no longer honoured.
    pub expires_at: Option<String>,
}

/// `RelayState` as it was sent: absent, present-but-empty, or a value.
///
/// SAML treats the three differently and the response must match exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PendingRelayState {
    /// No `RelayState` parameter was sent.
    Absent,
    /// `RelayState` was sent with an empty value.
    Empty,
    /// `RelayState` was sent with this value.
    Value(String),
}

/// Where started logins wait for their response.
#[async_trait]
pub trait SamlPendingStore: Send + Sync {
    /// Remember a started login for at most `ttl`.
    async fn put(&self, login: &PendingLogin, ttl: Duration) -> Result<(), Error>;

    /// Remove and return the login with this request ID, if it is still
    /// pending. A login is handed out at most once.
    async fn take(&self, request_id: &str) -> Result<Option<PendingLogin>, Error>;
}

/// Where accepted response and assertion identifiers are remembered.
#[async_trait]
pub trait SamlReplayStore: Send + Sync {
    /// Record `key` for `retain_for`, returning `true` if it was not already
    /// present. Implementations must make the check and the insert atomic.
    async fn insert_if_absent(&self, key: &str, retain_for: Duration) -> Result<bool, Error>;
}

/// Single-process store backed by two maps.
///
/// Suitable for one replica. Entries expire lazily on access, so an idle
/// process holds at most what its last busy period left behind.
#[derive(Debug, Default)]
pub struct InMemorySamlStore {
    pending: Mutex<HashMap<String, (PendingLogin, Instant)>>,
    replay: Mutex<HashMap<String, Instant>>,
}

impl InMemorySamlStore {
    /// Create an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        // A poisoned lock only means a panic elsewhere mid-insert; the maps
        // hold plain values, so the data is still consistent.
        mutex
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[async_trait]
impl SamlPendingStore for InMemorySamlStore {
    async fn put(&self, login: &PendingLogin, ttl: Duration) -> Result<(), Error> {
        let now = Instant::now();
        let mut pending = Self::lock(&self.pending);
        pending.retain(|_, (_, deadline)| *deadline > now);
        pending.insert(login.request_id.clone(), (login.clone(), now + ttl));
        Ok(())
    }

    async fn take(&self, request_id: &str) -> Result<Option<PendingLogin>, Error> {
        let now = Instant::now();
        let mut pending = Self::lock(&self.pending);
        pending.retain(|_, (_, deadline)| *deadline > now);
        Ok(pending.remove(request_id).map(|(login, _)| login))
    }
}

#[async_trait]
impl SamlReplayStore for InMemorySamlStore {
    async fn insert_if_absent(&self, key: &str, retain_for: Duration) -> Result<bool, Error> {
        let now = Instant::now();
        let mut replay = Self::lock(&self.replay);
        replay.retain(|_, deadline| *deadline > now);
        if replay.contains_key(key) {
            return Ok(false);
        }
        replay.insert(key.to_owned(), now + retain_for);
        Ok(true)
    }
}

#[cfg(feature = "cache")]
mod redis_impl {
    use super::*;
    use deadpool_redis::redis::AsyncCommands;
    use deadpool_redis::Pool as RedisPool;

    /// Redis-backed store shared across replicas.
    ///
    /// Pending logins are `SET … EX` and consumed with `GETDEL`; replay keys
    /// use `SET … NX EX`, so both operations are atomic on the server.
    #[derive(Clone)]
    pub struct RedisSamlStore {
        pool: RedisPool,
        key_prefix: String,
    }

    impl RedisSamlStore {
        /// Create a store using the `saml:` key prefix.
        #[must_use]
        pub fn new(pool: RedisPool) -> Self {
            Self::with_prefix(pool, "saml:")
        }

        /// Create a store with a custom key prefix.
        #[must_use]
        pub fn with_prefix(pool: RedisPool, prefix: impl Into<String>) -> Self {
            Self {
                pool,
                key_prefix: prefix.into(),
            }
        }

        fn pending_key(&self, request_id: &str) -> String {
            format!("{}pending:{request_id}", self.key_prefix)
        }

        fn replay_key(&self, key: &str) -> String {
            format!("{}replay:{key}", self.key_prefix)
        }

        async fn connection(&self) -> Result<deadpool_redis::Connection, Error> {
            self.pool.get().await.map_err(|error| {
                Error::Internal(format!("Failed to get Redis connection: {error}"))
            })
        }
    }

    #[async_trait]
    impl SamlPendingStore for RedisSamlStore {
        async fn put(&self, login: &PendingLogin, ttl: Duration) -> Result<(), Error> {
            let json = serde_json::to_string(login).map_err(|error| {
                Error::Internal(format!("Failed to serialize pending SAML login: {error}"))
            })?;
            let mut conn = self.connection().await?;
            conn.set_ex::<_, _, ()>(
                self.pending_key(&login.request_id),
                json,
                ttl.as_secs().max(1),
            )
            .await
            .map_err(|error| {
                Error::Internal(format!("Failed to store pending SAML login: {error}"))
            })
        }

        async fn take(&self, request_id: &str) -> Result<Option<PendingLogin>, Error> {
            let mut conn = self.connection().await?;
            let json: Option<String> =
                conn.get_del(self.pending_key(request_id))
                    .await
                    .map_err(|error| {
                        Error::Internal(format!("Failed to fetch pending SAML login: {error}"))
                    })?;
            json.map(|json| {
                serde_json::from_str(&json).map_err(|error| {
                    Error::Internal(format!("Failed to deserialize pending SAML login: {error}"))
                })
            })
            .transpose()
        }
    }

    #[async_trait]
    impl SamlReplayStore for RedisSamlStore {
        async fn insert_if_absent(&self, key: &str, retain_for: Duration) -> Result<bool, Error> {
            let mut conn = self.connection().await?;
            let stored: Option<String> = deadpool_redis::redis::cmd("SET")
                .arg(self.replay_key(key))
                .arg(1)
                .arg("NX")
                .arg("EX")
                .arg(retain_for.as_secs().max(1))
                .query_async(&mut conn)
                .await
                .map_err(|error| {
                    Error::Internal(format!("Failed to record SAML replay key: {error}"))
                })?;
            Ok(stored.is_some())
        }
    }
}

#[cfg(feature = "cache")]
pub use redis_impl::RedisSamlStore;

#[cfg(test)]
mod tests {
    use super::*;

    fn login(id: &str) -> PendingLogin {
        PendingLogin {
            request_id: id.to_owned(),
            relay_state: PendingRelayState::Absent,
            idp_entity_id: "https://idp.test".to_owned(),
            response_binding: "post".to_owned(),
            request_binding: Some("redirect".to_owned()),
            acs_url: "https://sp.test/acs".to_owned(),
            acs_binding: "post".to_owned(),
            acs_index: None,
            acs_is_default: true,
            issued_at: None,
            expires_at: None,
        }
    }

    #[tokio::test]
    async fn pending_login_is_taken_once() {
        let store = InMemorySamlStore::new();
        store
            .put(&login("r1"), Duration::from_secs(60))
            .await
            .unwrap();
        assert_eq!(store.take("r1").await.unwrap(), Some(login("r1")));
        assert_eq!(store.take("r1").await.unwrap(), None);
    }

    #[tokio::test]
    async fn pending_login_expires() {
        let store = InMemorySamlStore::new();
        store.put(&login("r1"), Duration::ZERO).await.unwrap();
        assert_eq!(store.take("r1").await.unwrap(), None);
    }

    #[tokio::test]
    async fn replay_key_is_inserted_once() {
        let store = InMemorySamlStore::new();
        assert!(store
            .insert_if_absent("a", Duration::from_secs(60))
            .await
            .unwrap());
        assert!(!store
            .insert_if_absent("a", Duration::from_secs(60))
            .await
            .unwrap());
        assert!(store
            .insert_if_absent("b", Duration::from_secs(60))
            .await
            .unwrap());
    }

    #[test]
    fn pending_login_round_trips_through_json() {
        let mut original = login("r1");
        original.relay_state = PendingRelayState::Value("/home".to_owned());
        let json = serde_json::to_string(&original).unwrap();
        let parsed: PendingLogin = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original);
    }
}
