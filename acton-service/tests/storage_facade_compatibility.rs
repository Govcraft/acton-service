#![cfg(feature = "audit")]

use acton_service::audit::storage::AuditQuery;
use acton_service::error::{Error, Result};

#[test]
fn audit_query_keeps_the_service_error_type() {
    let query = AuditQuery {
        limit: 0,
        ..AuditQuery::default()
    };
    let result: Result<()> = query.validate();
    assert!(matches!(result, Err(Error::Internal(_))));
}

#[test]
fn adapter_errors_keep_service_messages_and_categories() {
    let error: Error =
        acton_service_core::StorageError::Internal("storage unavailable".into()).into();
    assert!(matches!(error, Error::Internal(message) if message == "storage unavailable"));
    #[cfg(any(
        feature = "database",
        feature = "mssql",
        feature = "turso",
        feature = "surrealdb"
    ))]
    {
        use acton_service_core::{DatabaseError, DatabaseErrorKind, DatabaseOperation};
        let error: Error = acton_service_core::StorageError::Database(
            DatabaseError::timeout(DatabaseOperation::Query, "storage timeout")
                .add_context("audit_events"),
        )
        .into();
        assert!(
            matches!(error, Error::Database(error) if error.kind == DatabaseErrorKind::Timeout && error.operation == DatabaseOperation::Query && error.context.as_deref() == Some("audit_events"))
        );
    }
}

#[cfg(feature = "turso")]
#[tokio::test]
async fn turso_facade_preserves_initialization_and_storage_trait_objects() {
    use acton_service::audit::{
        storage::{turso::TursoAuditStorage, AuditStorage},
        AuditChain, AuditEvent, AuditEventKind, AuditSeverity,
    };
    use std::sync::Arc;

    let file = tempfile::NamedTempFile::new().unwrap();
    let database = libsql::Builder::new_local(file.path())
        .build()
        .await
        .unwrap();
    let storage = TursoAuditStorage::new(Arc::new(database));
    let initialized: Result<()> = storage.initialize().await;
    initialized.unwrap();
    let storage: Arc<dyn AuditStorage> = Arc::new(storage);
    let mut chain = AuditChain::new("facade-compatibility".into());
    let event = chain.seal(AuditEvent::new(
        AuditEventKind::HttpRequest,
        AuditSeverity::Informational,
        "facade-compatibility".into(),
    ));
    storage.append(&event).await.unwrap();
    assert_eq!(storage.latest().await.unwrap().unwrap().id, event.id);
    assert_eq!(storage.verify_chain(1).await.unwrap(), None);
}
