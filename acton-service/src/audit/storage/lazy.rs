//! Lazily-resolved audit storage
//!
//! Connection pool agents connect *asynchronously* — [`ServiceBuilder::build`] spawns
//! them and returns before the pool is established (see `agents::pool`, which writes
//! the pool into shared state from a spawned task). Any attempt to read a connected
//! pool at builder time therefore observes `None` and would permanently latch a
//! storage-less audit logger.
//!
//! [`LazyAuditStorage`] closes that gap: it holds the *shared handle* rather than a
//! connected client, and resolves the concrete backend on first use. Schema
//! initialization (append-only DDL) runs exactly once, on that first successful
//! resolution.
//!
//! [`ServiceBuilder::build`]: crate::service_builder::ServiceBuilder::build

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use super::AuditStorage;
use crate::audit::event::AuditEvent;
use crate::error::Error;

/// Shared-handle type used by every pool agent: `Arc<RwLock<Option<Conn>>>`.
pub(crate) type SharedConn<C> = Arc<RwLock<Option<C>>>;

/// A storage backend that can be built from a pool connection and needs one-time
/// schema initialization.
///
/// Implemented by each concrete backend so [`LazyAuditStorage`] can construct and
/// initialize it without knowing the connection type.
#[async_trait]
pub(crate) trait InitializableStorage: AuditStorage + Sized + 'static {
    /// The connection/client type held in the pool agent's shared state.
    type Conn: Clone + Send + Sync + 'static;

    /// Build the backend from a connected client.
    fn from_conn(conn: Self::Conn) -> Self;

    /// Create tables, indexes, and append-only enforcement. Must be idempotent.
    async fn init_schema(&self) -> Result<(), Error>;

    /// Human-readable backend name, used in log messages.
    fn backend_name() -> &'static str;
}

/// Audit storage that resolves its backend once the connection pool is available.
///
/// Before the pool connects, every operation fails with a clear "not yet connected"
/// error, which the audit agent treats as retryable rather than fatal.
pub struct LazyAuditStorage<S: InitializableStorage> {
    shared: SharedConn<S::Conn>,
    resolved: RwLock<Option<Arc<S>>>,
    /// Serializes first-resolution so schema init runs once, not once per caller.
    init_lock: Mutex<()>,
}

impl<S: InitializableStorage> LazyAuditStorage<S> {
    /// Wrap a pool agent's shared connection handle.
    pub(crate) fn new(shared: SharedConn<S::Conn>) -> Self {
        Self {
            shared,
            resolved: RwLock::new(None),
            init_lock: Mutex::new(()),
        }
    }

    /// Return the initialized backend, building it on first call.
    ///
    /// Errors while the pool is still connecting; callers should retry.
    async fn resolve(&self) -> Result<Arc<S>, Error> {
        if let Some(storage) = self.resolved.read().await.clone() {
            return Ok(storage);
        }

        // Serialize construction so concurrent callers don't each run DDL.
        let _guard = self.init_lock.lock().await;

        // Another caller may have resolved while we waited for the lock.
        if let Some(storage) = self.resolved.read().await.clone() {
            return Ok(storage);
        }

        let conn = self.shared.read().await.clone().ok_or_else(|| {
            Error::Internal(format!(
                "audit storage unavailable: the {} connection pool has not finished connecting",
                S::backend_name()
            ))
        })?;

        let storage = Arc::new(S::from_conn(conn));
        storage.init_schema().await?;

        *self.resolved.write().await = Some(storage.clone());
        tracing::info!(
            backend = S::backend_name(),
            "Audit storage attached and schema initialized"
        );

        Ok(storage)
    }
}

#[async_trait]
impl<S: InitializableStorage> AuditStorage for LazyAuditStorage<S> {
    async fn append(&self, event: &AuditEvent) -> Result<(), Error> {
        self.resolve().await?.append(event).await
    }

    async fn latest(&self) -> Result<Option<AuditEvent>, Error> {
        self.resolve().await?.latest().await
    }

    async fn query_range(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<AuditEvent>, Error> {
        self.resolve().await?.query_range(from, to, limit).await
    }

    async fn verify_chain(&self, from_sequence: u64) -> Result<Option<u64>, Error> {
        self.resolve().await?.verify_chain(from_sequence).await
    }

    async fn query_before(
        &self,
        cutoff: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<AuditEvent>, Error> {
        self.resolve().await?.query_before(cutoff, limit).await
    }

    async fn purge_before(&self, cutoff: DateTime<Utc>) -> Result<u64, Error> {
        self.resolve().await?.purge_before(cutoff).await
    }

    async fn ensure_ready(&self) -> Result<(), Error> {
        self.resolve().await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Minimal backend that records how many times its schema was initialized.
    struct MockStorage {
        conn: &'static str,
    }

    static INIT_COUNT: AtomicUsize = AtomicUsize::new(0);

    #[async_trait]
    impl AuditStorage for MockStorage {
        async fn append(&self, _event: &AuditEvent) -> Result<(), Error> {
            Ok(())
        }

        async fn latest(&self) -> Result<Option<AuditEvent>, Error> {
            Ok(None)
        }

        async fn query_range(
            &self,
            _from: DateTime<Utc>,
            _to: DateTime<Utc>,
            _limit: usize,
        ) -> Result<Vec<AuditEvent>, Error> {
            Ok(Vec::new())
        }

        async fn verify_chain(&self, _from_sequence: u64) -> Result<Option<u64>, Error> {
            Ok(None)
        }
    }

    #[async_trait]
    impl InitializableStorage for MockStorage {
        type Conn = &'static str;

        fn from_conn(conn: Self::Conn) -> Self {
            Self { conn }
        }

        async fn init_schema(&self) -> Result<(), Error> {
            INIT_COUNT.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn backend_name() -> &'static str {
            "mock"
        }
    }

    #[tokio::test]
    async fn unconnected_pool_yields_retryable_error_not_silent_success() {
        let shared: SharedConn<&'static str> = Arc::new(RwLock::new(None));
        let storage = LazyAuditStorage::<MockStorage>::new(shared);

        let err = storage.ensure_ready().await.unwrap_err();
        assert!(
            err.to_string().contains("has not finished connecting"),
            "error should identify the pool as still connecting, got: {err}"
        );
    }

    #[tokio::test]
    async fn resolves_once_the_pool_connects() {
        let shared: SharedConn<&'static str> = Arc::new(RwLock::new(None));
        let storage = LazyAuditStorage::<MockStorage>::new(shared.clone());

        assert!(storage.ensure_ready().await.is_err());

        // Simulate the pool agent completing its connection.
        *shared.write().await = Some("connected");

        storage
            .ensure_ready()
            .await
            .expect("storage should resolve after the pool connects");
    }

    #[tokio::test]
    async fn schema_initialized_exactly_once_across_concurrent_callers() {
        INIT_COUNT.store(0, Ordering::SeqCst);
        let shared: SharedConn<&'static str> = Arc::new(RwLock::new(Some("connected")));
        let storage = Arc::new(LazyAuditStorage::<MockStorage>::new(shared));

        let mut handles = Vec::new();
        for _ in 0..16 {
            let s = storage.clone();
            handles.push(tokio::spawn(async move { s.ensure_ready().await }));
        }
        for h in handles {
            h.await.unwrap().expect("resolve should succeed");
        }

        assert_eq!(
            INIT_COUNT.load(Ordering::SeqCst),
            1,
            "schema init must run once even under concurrent first-use"
        );
    }

    #[tokio::test]
    async fn resolved_backend_is_cached_and_reused() {
        let shared: SharedConn<&'static str> = Arc::new(RwLock::new(Some("first")));
        let storage = LazyAuditStorage::<MockStorage>::new(shared.clone());

        storage.ensure_ready().await.unwrap();
        assert_eq!(
            storage.resolved.read().await.as_ref().unwrap().conn,
            "first"
        );

        // A later pool swap must not silently re-point an already-resolved chain.
        *shared.write().await = Some("second");
        storage.ensure_ready().await.unwrap();
        assert_eq!(
            storage.resolved.read().await.as_ref().unwrap().conn,
            "first"
        );
    }
}
