//! Audit storage backend selection
//!
//! Chooses which [`AuditStorage`] implementation to attach to the audit agent based
//! on the enabled database features and the pool handles the builder created.
//!
//! Selection returns a *lazily-resolved* backend rather than a connected one. Pool
//! agents connect asynchronously, so an eager check would find every pool empty at
//! builder time and permanently disable audit persistence — the bug behind issue #34.

use std::sync::Arc;

use super::storage::AuditStorage;

/// Shared connection handles produced by the builder's pool agents.
///
/// Fields are feature-gated to mirror the builder's own gating; a `None` field means
/// that backend's pool agent was not spawned for this service.
#[derive(Default)]
pub(crate) struct AuditStorageHandles {
    #[cfg(feature = "database")]
    pub db_pool: Option<crate::agents::SharedDbPool>,
    #[cfg(feature = "turso")]
    pub turso_db: Option<crate::agents::SharedTursoDb>,
    #[cfg(feature = "surrealdb")]
    pub surrealdb_client: Option<crate::agents::SharedSurrealDb>,
    #[cfg(feature = "clickhouse")]
    pub clickhouse_client: Option<crate::agents::SharedClickHouseClient>,
}

/// Select the audit storage backend for the configured database.
///
/// Backends are tried in the same priority order the key-rotation wiring uses:
/// PostgreSQL, Turso, SurrealDB, then ClickHouse. Returns `None` only when no
/// database feature is enabled or no pool agent was spawned, in which case the
/// audit agent runs with an in-memory chain plus syslog/tracing export.
#[allow(unused_variables, unused_mut)]
pub(crate) fn select_audit_storage(
    handles: &AuditStorageHandles,
) -> Option<Arc<dyn AuditStorage>> {
    #[allow(unused_mut)]
    let mut storage: Option<Arc<dyn AuditStorage>> = None;

    #[cfg(feature = "database")]
    if storage.is_none() {
        if let Some(ref shared) = handles.db_pool {
            storage = Some(Arc::new(super::storage::lazy::LazyAuditStorage::<
                super::storage::pg::PgAuditStorage,
            >::new(shared.clone())));
            tracing::debug!("Audit persistence will use PostgreSQL storage");
        }
    }

    #[cfg(feature = "turso")]
    if storage.is_none() {
        if let Some(ref shared) = handles.turso_db {
            storage = Some(Arc::new(super::storage::lazy::LazyAuditStorage::<
                super::storage::turso::TursoAuditStorage,
            >::new(shared.clone())));
            tracing::debug!("Audit persistence will use Turso storage");
        }
    }

    #[cfg(feature = "surrealdb")]
    if storage.is_none() {
        if let Some(ref shared) = handles.surrealdb_client {
            storage = Some(Arc::new(super::storage::lazy::LazyAuditStorage::<
                super::storage::surrealdb_impl::SurrealAuditStorage,
            >::new(shared.clone())));
            tracing::debug!("Audit persistence will use SurrealDB storage");
        }
    }

    #[cfg(feature = "clickhouse")]
    if storage.is_none() {
        if let Some(ref shared) = handles.clickhouse_client {
            storage = Some(Arc::new(super::storage::lazy::LazyAuditStorage::<
                super::storage::clickhouse_impl::ClickHouseAuditStorage,
            >::new(shared.clone())));
            tracing::debug!("Audit persistence will use ClickHouse storage");
        }
    }

    if storage.is_none() {
        tracing::warn!(
            "Audit logging is enabled but no database pool is configured; events will be \
             exported to tracing/syslog only and will not be persisted"
        );
    }

    storage
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_pools_yields_no_storage() {
        let handles = AuditStorageHandles::default();
        assert!(
            select_audit_storage(&handles).is_none(),
            "without any pool handle there is nothing to persist to"
        );
    }

    /// Regression test for issue #34: the builder previously hardcoded
    /// `storage = None`, so a service with `audit` + a database feature silently
    /// got tracing-only auditing. Storage must be attached even though the pool
    /// has not connected yet — that is the whole point of resolving lazily.
    #[cfg(feature = "database")]
    #[test]
    fn database_pool_attaches_storage_before_the_pool_connects() {
        let handles = AuditStorageHandles {
            // Exactly what the builder holds at audit-init time: allocated, unconnected.
            db_pool: Some(Arc::new(tokio::sync::RwLock::new(None))),
            #[cfg(feature = "turso")]
            turso_db: None,
            #[cfg(feature = "surrealdb")]
            surrealdb_client: None,
            #[cfg(feature = "clickhouse")]
            clickhouse_client: None,
        };

        assert!(
            select_audit_storage(&handles).is_some(),
            "audit storage must be attached from an unconnected pool handle"
        );
    }

    /// The selected backend must stay unresolved (and therefore retryable) rather
    /// than erroring out permanently while the pool is still connecting.
    #[cfg(feature = "database")]
    #[tokio::test]
    async fn selected_storage_is_retryable_until_the_pool_connects() {
        let shared: crate::agents::SharedDbPool = Arc::new(tokio::sync::RwLock::new(None));
        let handles = AuditStorageHandles {
            db_pool: Some(shared),
            #[cfg(feature = "turso")]
            turso_db: None,
            #[cfg(feature = "surrealdb")]
            surrealdb_client: None,
            #[cfg(feature = "clickhouse")]
            clickhouse_client: None,
        };

        let storage = select_audit_storage(&handles).expect("storage should be selected");
        let err = storage.ensure_ready().await.unwrap_err();
        assert!(
            err.to_string().contains("PostgreSQL"),
            "error should name the backend awaiting connection, got: {err}"
        );
    }
}
