#![cfg(feature = "turso")]

use acton_service::middleware::{
    revocation::{RevocationNamespace, TursoTokenRevocation},
    TokenRevocation,
};
use std::sync::Arc;
#[path = "../../acton-service-integration-tests/tests/common/revocation.rs"]
mod revocation_contract;

#[tokio::test]
async fn persistent_revocation_contract_and_missing_schema_fail_closed() {
    let directory = tempfile::tempdir().unwrap();
    let db = Arc::new(
        libsql::Builder::new_local(directory.path().join("revocation.db"))
            .build()
            .await
            .expect("local database"),
    );
    let storage = TursoTokenRevocation::new(db.clone(), RevocationNamespace::new("a").unwrap());
    assert!(
        storage.is_revoked("token").await.is_err(),
        "missing migration must fail closed"
    );
    let isolated = TursoTokenRevocation::new(db, RevocationNamespace::new("b").unwrap());
    revocation_contract::contract(&storage, &isolated).await;
    assert!(storage.revoke("overflow", u64::MAX).await.is_err());
}
