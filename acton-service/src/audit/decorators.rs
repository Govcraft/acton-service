//! Audit-emitting decorators for auth storage backends.
//!
//! The framework declares audit event kinds for token refresh, API-key
//! lifecycle, and OAuth callbacks, but the flows they describe run through
//! storage traits ([`RefreshTokenStorage`], [`ApiKeyStorage`]) and the
//! [`OAuthProvider`] trait — implemented once per backend and constructed by
//! the application, not by `ServiceBuilder`. Emitting inside each backend
//! would duplicate the same lines four times per family, so emission lives in
//! these wrappers instead: wrap whatever you construct, hand it the
//! [`AuditLogger`] from `AppState::audit_logger()`, and every backend emits
//! the same events (issue #16).
//!
//! ```rust,ignore
//! let storage = RedisRefreshStorage::new(pool);
//! let storage = AuditedRefreshStorage::new(storage, logger.clone());
//! ```
//!
//! All emissions honor `[audit] audit_auth_events` like the built-in auth
//! middleware events.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::auth::api_keys::{ApiKey, ApiKeyStorage};
#[cfg(feature = "oauth")]
use crate::auth::oauth::provider::{OAuthProvider, OAuthTokens, OAuthUserInfo};
use crate::auth::tokens::refresh::{
    RefreshTokenData, RefreshTokenMetadata, RefreshTokenStorage,
};
use crate::error::Error;

use super::event::{AuditEvent, AuditEventKind, AuditSeverity, AuditSource};
use super::logger::AuditLogger;

/// Server-side operations have no request context; the source carries only
/// the subject (and, for refresh rotation, whatever the stored token metadata
/// captured at issuance).
fn subject_source(subject: Option<String>) -> AuditSource {
    AuditSource {
        ip: None,
        user_agent: None,
        subject,
        request_id: None,
    }
}

async fn emit(
    logger: &AuditLogger,
    kind: AuditEventKind,
    severity: AuditSeverity,
    source: AuditSource,
    metadata: serde_json::Value,
) {
    if !logger.config().audit_auth_events {
        return;
    }
    let event = AuditEvent::new(kind, severity, logger.service_name().to_string())
        .with_source(source)
        .with_metadata(metadata);
    logger.log(event).await;
}

/// [`RefreshTokenStorage`] wrapper that emits `AuthTokenRefresh` on every
/// successful rotation.
///
/// Only `rotate` emits: `store` is initial issuance (part of login, already
/// audited by the auth middleware) and revocations have their own event kinds.
pub struct AuditedRefreshStorage<S> {
    inner: S,
    logger: AuditLogger,
}

impl<S: RefreshTokenStorage> AuditedRefreshStorage<S> {
    /// Wrap a refresh-token storage backend.
    pub fn new(inner: S, logger: AuditLogger) -> Self {
        Self { inner, logger }
    }
}

#[async_trait]
impl<S: RefreshTokenStorage> RefreshTokenStorage for AuditedRefreshStorage<S> {
    async fn store(
        &self,
        token_id: &str,
        user_id: &str,
        family_id: &str,
        expires_at: DateTime<Utc>,
        metadata: &RefreshTokenMetadata,
    ) -> Result<(), Error> {
        self.inner
            .store(token_id, user_id, family_id, expires_at, metadata)
            .await
    }

    async fn get(&self, token_id: &str) -> Result<Option<RefreshTokenData>, Error> {
        self.inner.get(token_id).await
    }

    async fn revoke(&self, token_id: &str) -> Result<(), Error> {
        self.inner.revoke(token_id).await
    }

    async fn revoke_family(&self, family_id: &str) -> Result<u64, Error> {
        self.inner.revoke_family(family_id).await
    }

    async fn revoke_all_for_user(&self, user_id: &str) -> Result<u64, Error> {
        self.inner.revoke_all_for_user(user_id).await
    }

    async fn rotate(
        &self,
        old_token_id: &str,
        new_token_id: &str,
        user_id: &str,
        family_id: &str,
        expires_at: DateTime<Utc>,
        metadata: &RefreshTokenMetadata,
    ) -> Result<(), Error> {
        self.inner
            .rotate(
                old_token_id,
                new_token_id,
                user_id,
                family_id,
                expires_at,
                metadata,
            )
            .await?;

        let source = AuditSource {
            ip: metadata.ip_address.clone(),
            user_agent: metadata.user_agent.clone(),
            subject: Some(user_id.to_string()),
            request_id: None,
        };
        emit(
            &self.logger,
            AuditEventKind::AuthTokenRefresh,
            AuditSeverity::Notice,
            source,
            serde_json::json!({
                "family_id": family_id,
                "old_token_id": old_token_id,
                "new_token_id": new_token_id,
            }),
        )
        .await;
        Ok(())
    }

    async fn cleanup_expired(&self) -> Result<u64, Error> {
        self.inner.cleanup_expired().await
    }
}

/// [`ApiKeyStorage`] wrapper that emits `AuthApiKeyCreated` and
/// `AuthApiKeyRevoked` on the respective successful operations.
///
/// Event metadata never includes the key material or its hash — only the ID,
/// display name, prefix, and scopes.
pub struct AuditedApiKeyStorage<S> {
    inner: S,
    logger: AuditLogger,
}

impl<S: ApiKeyStorage> AuditedApiKeyStorage<S> {
    /// Wrap an API-key storage backend.
    pub fn new(inner: S, logger: AuditLogger) -> Self {
        Self { inner, logger }
    }
}

#[async_trait]
impl<S: ApiKeyStorage> ApiKeyStorage for AuditedApiKeyStorage<S> {
    async fn get_by_key(&self, key: &str) -> Result<Option<ApiKey>, Error> {
        self.inner.get_by_key(key).await
    }

    async fn get_by_prefix(&self, prefix: &str) -> Result<Option<ApiKey>, Error> {
        self.inner.get_by_prefix(prefix).await
    }

    async fn get_by_id(&self, id: &str) -> Result<Option<ApiKey>, Error> {
        self.inner.get_by_id(id).await
    }

    async fn create(&self, key: &ApiKey) -> Result<(), Error> {
        self.inner.create(key).await?;

        emit(
            &self.logger,
            AuditEventKind::AuthApiKeyCreated,
            AuditSeverity::Notice,
            subject_source(Some(key.user_id.clone())),
            serde_json::json!({
                "key_id": key.id,
                "name": key.name,
                "prefix": key.prefix,
                "scopes": key.scopes,
            }),
        )
        .await;
        Ok(())
    }

    async fn update_last_used(&self, id: &str) -> Result<(), Error> {
        self.inner.update_last_used(id).await
    }

    async fn revoke(&self, id: &str) -> Result<(), Error> {
        self.inner.revoke(id).await?;

        emit(
            &self.logger,
            AuditEventKind::AuthApiKeyRevoked,
            AuditSeverity::Notice,
            subject_source(None),
            serde_json::json!({ "key_id": id }),
        )
        .await;
        Ok(())
    }

    async fn list_by_user(&self, user_id: &str) -> Result<Vec<ApiKey>, Error> {
        self.inner.list_by_user(user_id).await
    }

    async fn delete(&self, id: &str) -> Result<(), Error> {
        self.inner.delete(id).await
    }
}

/// [`OAuthProvider`] wrapper that emits `AuthOAuthCallback` when an
/// authorization code is exchanged — the framework-visible moment of the
/// OAuth callback — on both success and failure.
#[cfg(feature = "oauth")]
pub struct AuditedOAuthProvider<P> {
    inner: P,
    logger: AuditLogger,
}

#[cfg(feature = "oauth")]
impl<P: OAuthProvider> AuditedOAuthProvider<P> {
    /// Wrap an OAuth provider.
    pub fn new(inner: P, logger: AuditLogger) -> Self {
        Self { inner, logger }
    }
}

#[cfg(feature = "oauth")]
#[async_trait]
impl<P: OAuthProvider> OAuthProvider for AuditedOAuthProvider<P> {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn authorization_url(&self, state: &str, scopes: &[String]) -> String {
        self.inner.authorization_url(state, scopes)
    }

    async fn exchange_code(&self, code: &str) -> Result<OAuthTokens, Error> {
        match self.inner.exchange_code(code).await {
            Ok(tokens) => {
                emit(
                    &self.logger,
                    AuditEventKind::AuthOAuthCallback,
                    AuditSeverity::Notice,
                    subject_source(None),
                    serde_json::json!({
                        "provider": self.inner.name(),
                        "outcome": "success",
                    }),
                )
                .await;
                Ok(tokens)
            }
            Err(e) => {
                emit(
                    &self.logger,
                    AuditEventKind::AuthOAuthCallback,
                    AuditSeverity::Warning,
                    subject_source(None),
                    serde_json::json!({
                        "provider": self.inner.name(),
                        "outcome": "failure",
                        "error": e.to_string(),
                    }),
                )
                .await;
                Err(e)
            }
        }
    }

    async fn get_user_info(&self, access_token: &str) -> Result<OAuthUserInfo, Error> {
        self.inner.get_user_info(access_token).await
    }

    async fn refresh_token(&self, refresh_token: &str) -> Result<OAuthTokens, Error> {
        self.inner.refresh_token(refresh_token).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use acton_reactive::prelude::*;
    use async_trait::async_trait;
    use chrono::Utc;

    use super::*;
    use crate::audit::agent::AuditAgent;
    use crate::audit::config::{AuditConfig, SyslogConfig};
    use crate::audit::storage::AuditStorage;

    /// AuditStorage that records every appended event for assertions.
    #[derive(Default)]
    struct CapturingAuditStorage {
        events: Mutex<Vec<AuditEvent>>,
    }

    #[async_trait]
    impl AuditStorage for CapturingAuditStorage {
        async fn append(&self, event: &AuditEvent) -> Result<(), Error> {
            self.events.lock().expect("events lock").push(event.clone());
            Ok(())
        }

        async fn latest(&self) -> Result<Option<AuditEvent>, Error> {
            Ok(None)
        }

        async fn query_range(
            &self,
            _from: chrono::DateTime<Utc>,
            _to: chrono::DateTime<Utc>,
            _limit: usize,
        ) -> Result<Vec<AuditEvent>, Error> {
            Ok(Vec::new())
        }

        async fn verify_chain(&self, _from_sequence: u64) -> Result<Option<u64>, Error> {
            Ok(None)
        }
    }

    /// Spawn a real audit agent backed by the capturing storage. The runtime
    /// must stay alive for the agent to process events, so it is returned
    /// alongside the logger.
    async fn capturing_logger() -> (ActorRuntime, Arc<CapturingAuditStorage>, AuditLogger) {
        let mut runtime = ActonApp::launch_async().await;
        let storage = Arc::new(CapturingAuditStorage::default());
        let config = AuditConfig {
            syslog: SyslogConfig {
                transport: "none".to_string(),
                ..SyslogConfig::default()
            },
            ..AuditConfig::default()
        };
        let handle = AuditAgent::spawn(
            &mut runtime,
            config.clone(),
            Some(storage.clone() as Arc<dyn AuditStorage>),
            "test-svc".to_string(),
        )
        .await
        .expect("spawn audit agent");
        let logger = AuditLogger::new(handle, "test-svc".to_string(), config);

        // The chain initializes asynchronously (after_start -> ChainLoaded) and
        // the agent drops events that arrive before it finishes, so probe with
        // sentinel events until one lands, then discard the sentinels.
        for _ in 0..100 {
            logger
                .log_custom("test.chain-probe", AuditSeverity::Informational, None)
                .await;
            tokio::time::sleep(Duration::from_millis(20)).await;
            if !storage.events.lock().expect("events lock").is_empty() {
                break;
            }
        }
        storage.events.lock().expect("events lock").clear();

        (runtime, storage, logger)
    }

    /// Fire-and-forget emission: poll until the expected kind shows up.
    async fn wait_for_event(
        storage: &CapturingAuditStorage,
        kind: &AuditEventKind,
    ) -> Option<AuditEvent> {
        for _ in 0..50 {
            if let Some(event) = storage
                .events
                .lock()
                .expect("events lock")
                .iter()
                .find(|e| &e.kind == kind)
                .cloned()
            {
                return Some(event);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        None
    }

    #[derive(Default)]
    struct NoopRefreshStorage;

    #[async_trait]
    impl RefreshTokenStorage for NoopRefreshStorage {
        async fn store(
            &self,
            _token_id: &str,
            _user_id: &str,
            _family_id: &str,
            _expires_at: chrono::DateTime<Utc>,
            _metadata: &RefreshTokenMetadata,
        ) -> Result<(), Error> {
            Ok(())
        }

        async fn get(&self, _token_id: &str) -> Result<Option<RefreshTokenData>, Error> {
            Ok(None)
        }

        async fn revoke(&self, _token_id: &str) -> Result<(), Error> {
            Ok(())
        }

        async fn revoke_family(&self, _family_id: &str) -> Result<u64, Error> {
            Ok(0)
        }

        async fn revoke_all_for_user(&self, _user_id: &str) -> Result<u64, Error> {
            Ok(0)
        }

        async fn rotate(
            &self,
            _old_token_id: &str,
            _new_token_id: &str,
            _user_id: &str,
            _family_id: &str,
            _expires_at: chrono::DateTime<Utc>,
            _metadata: &RefreshTokenMetadata,
        ) -> Result<(), Error> {
            Ok(())
        }

        async fn cleanup_expired(&self) -> Result<u64, Error> {
            Ok(0)
        }
    }

    /// Refresh storage whose rotate always fails, to prove no event is
    /// emitted for failed rotations.
    #[derive(Default)]
    struct FailingRefreshStorage;

    #[async_trait]
    impl RefreshTokenStorage for FailingRefreshStorage {
        async fn store(
            &self,
            _token_id: &str,
            _user_id: &str,
            _family_id: &str,
            _expires_at: chrono::DateTime<Utc>,
            _metadata: &RefreshTokenMetadata,
        ) -> Result<(), Error> {
            Ok(())
        }

        async fn get(&self, _token_id: &str) -> Result<Option<RefreshTokenData>, Error> {
            Ok(None)
        }

        async fn revoke(&self, _token_id: &str) -> Result<(), Error> {
            Ok(())
        }

        async fn revoke_family(&self, _family_id: &str) -> Result<u64, Error> {
            Ok(0)
        }

        async fn revoke_all_for_user(&self, _user_id: &str) -> Result<u64, Error> {
            Ok(0)
        }

        async fn rotate(
            &self,
            _old_token_id: &str,
            _new_token_id: &str,
            _user_id: &str,
            _family_id: &str,
            _expires_at: chrono::DateTime<Utc>,
            _metadata: &RefreshTokenMetadata,
        ) -> Result<(), Error> {
            Err(Error::Internal("rotation failed".to_string()))
        }

        async fn cleanup_expired(&self) -> Result<u64, Error> {
            Ok(0)
        }
    }

    #[derive(Default)]
    struct NoopApiKeyStorage;

    #[async_trait]
    impl ApiKeyStorage for NoopApiKeyStorage {
        async fn get_by_key(&self, _key: &str) -> Result<Option<ApiKey>, Error> {
            Ok(None)
        }

        async fn get_by_prefix(&self, _prefix: &str) -> Result<Option<ApiKey>, Error> {
            Ok(None)
        }

        async fn get_by_id(&self, _id: &str) -> Result<Option<ApiKey>, Error> {
            Ok(None)
        }

        async fn create(&self, _key: &ApiKey) -> Result<(), Error> {
            Ok(())
        }

        async fn update_last_used(&self, _id: &str) -> Result<(), Error> {
            Ok(())
        }

        async fn revoke(&self, _id: &str) -> Result<(), Error> {
            Ok(())
        }

        async fn list_by_user(&self, _user_id: &str) -> Result<Vec<ApiKey>, Error> {
            Ok(Vec::new())
        }

        async fn delete(&self, _id: &str) -> Result<(), Error> {
            Ok(())
        }
    }

    fn test_api_key() -> ApiKey {
        ApiKey {
            id: "key_123".to_string(),
            user_id: "user_9".to_string(),
            name: "ci deploy key".to_string(),
            prefix: "sk_test".to_string(),
            key_hash: "not-logged".to_string(),
            scopes: vec!["deploy".to_string()],
            rate_limit: None,
            is_revoked: false,
            last_used_at: None,
            expires_at: None,
            created_at: Utc::now(),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rotate_emits_auth_token_refresh_with_stored_metadata() {
        let (_runtime, storage, logger) = capturing_logger().await;
        let wrapped = AuditedRefreshStorage::new(NoopRefreshStorage, logger);

        let metadata = RefreshTokenMetadata {
            ip_address: Some("203.0.113.5".to_string()),
            user_agent: Some("acton-test/1.0".to_string()),
            ..RefreshTokenMetadata::default()
        };
        wrapped
            .rotate("old_jti", "new_jti", "user_9", "fam_1", Utc::now(), &metadata)
            .await
            .expect("rotate");

        let event = wait_for_event(&storage, &AuditEventKind::AuthTokenRefresh)
            .await
            .expect("AuthTokenRefresh emitted");
        assert_eq!(event.source.subject.as_deref(), Some("user_9"));
        assert_eq!(event.source.ip.as_deref(), Some("203.0.113.5"));
        let metadata = event.metadata.expect("metadata attached");
        assert_eq!(metadata["family_id"], "fam_1");
        assert_eq!(metadata["old_token_id"], "old_jti");
        assert_eq!(metadata["new_token_id"], "new_jti");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn failed_rotate_emits_nothing() {
        let (_runtime, storage, logger) = capturing_logger().await;
        let wrapped = AuditedRefreshStorage::new(FailingRefreshStorage, logger);

        let result = wrapped
            .rotate(
                "old_jti",
                "new_jti",
                "user_9",
                "fam_1",
                Utc::now(),
                &RefreshTokenMetadata::default(),
            )
            .await;
        assert!(result.is_err(), "inner failure must propagate");

        // Give the fire-and-forget path a moment to (incorrectly) deliver.
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            storage.events.lock().expect("events lock").is_empty(),
            "failed rotation must not be audited as a refresh"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn api_key_create_and_revoke_emit_without_key_material() {
        let (_runtime, storage, logger) = capturing_logger().await;
        let wrapped = AuditedApiKeyStorage::new(NoopApiKeyStorage, logger);

        wrapped.create(&test_api_key()).await.expect("create");
        let created = wait_for_event(&storage, &AuditEventKind::AuthApiKeyCreated)
            .await
            .expect("AuthApiKeyCreated emitted");
        let metadata = created.metadata.expect("metadata attached");
        assert_eq!(metadata["key_id"], "key_123");
        assert!(
            !metadata.to_string().contains("not-logged"),
            "key hash must never reach the audit log"
        );
        assert_eq!(created.source.subject.as_deref(), Some("user_9"));

        wrapped.revoke("key_123").await.expect("revoke");
        let revoked = wait_for_event(&storage, &AuditEventKind::AuthApiKeyRevoked)
            .await
            .expect("AuthApiKeyRevoked emitted");
        assert_eq!(revoked.metadata.expect("metadata")["key_id"], "key_123");
    }

    #[cfg(feature = "oauth")]
    mod oauth_tests {
        use super::*;
        use crate::auth::oauth::provider::{OAuthProvider, OAuthTokens, OAuthUserInfo};

        struct FailingProvider;

        #[async_trait]
        impl OAuthProvider for FailingProvider {
            fn name(&self) -> &str {
                "test-provider"
            }

            fn authorization_url(&self, _state: &str, _scopes: &[String]) -> String {
                "https://example.com/auth".to_string()
            }

            async fn exchange_code(&self, _code: &str) -> Result<OAuthTokens, Error> {
                Err(Error::Internal("exchange rejected".to_string()))
            }

            async fn get_user_info(&self, _access_token: &str) -> Result<OAuthUserInfo, Error> {
                Err(Error::Internal("unused".to_string()))
            }

            async fn refresh_token(&self, _refresh_token: &str) -> Result<OAuthTokens, Error> {
                Err(Error::Internal("unused".to_string()))
            }
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn failed_code_exchange_emits_oauth_callback_warning() {
            let (_runtime, storage, logger) = capturing_logger().await;
            let wrapped = AuditedOAuthProvider::new(FailingProvider, logger);

            let result = wrapped.exchange_code("bad-code").await;
            assert!(result.is_err(), "inner failure must propagate");

            let event = wait_for_event(&storage, &AuditEventKind::AuthOAuthCallback)
                .await
                .expect("AuthOAuthCallback emitted on failure");
            assert_eq!(event.severity, AuditSeverity::Warning);
            let metadata = event.metadata.expect("metadata attached");
            assert_eq!(metadata["provider"], "test-provider");
            assert_eq!(metadata["outcome"], "failure");
        }
    }
}
