#![cfg(feature = "surrealdb")]

use acton_service::{
    audit::{
        storage::{surrealdb_impl::SurrealAuditStorage, AuditStorage, AuditVerification},
        AuditChain, AuditEvent, AuditEventKind, AuditSeverity,
    },
    config::SurrealDbConfig,
};
use std::sync::Arc;
use testcontainers_modules::{
    surrealdb::SurrealDb,
    testcontainers::{runners::AsyncRunner, ImageExt},
};

#[tokio::test]
async fn provisioned_surreal_authentication_and_audit_integrity() {
    // The module's default is 2.2; pin the server to the production SDK generation.
    let container = SurrealDb::default()
        .with_tag("v3.0.5")
        .with_startup_timeout(std::time::Duration::from_secs(60))
        .start()
        .await
        .expect("start provisioned authenticated SurrealDB server");
    let host = container.get_host().await.expect("container host");
    let port = container
        .get_host_port_ipv4(8000)
        .await
        .expect("SurrealDB port");
    let config = SurrealDbConfig {
        url: format!("ws://{host}:{port}"),
        namespace: "audit_integration".into(),
        database: "audit_integration".into(),
        username: Some("root".into()),
        password: Some("root".into()),
        max_retries: 0,
        retry_delay_secs: 1,
        optional: false,
        lazy_init: false,
    };
    let mut wrong_password = config.clone();
    wrong_password.password = Some("invalid-password".into());
    assert!(acton_service_surrealdb::create_client(&wrong_password)
        .await
        .is_err());
    let client = Arc::new(
        acton_service_surrealdb::create_client(&config)
            .await
            .expect("authenticate provisioned root through production helper"),
    );
    let storage = SurrealAuditStorage::new(client.clone());
    storage.initialize().await.expect("initialize schema");
    storage
        .initialize()
        .await
        .expect("idempotent initialization");
    let mut chain = AuditChain::new("surreal-integration".into());
    let mut events = Vec::new();
    for revision in 1..=3 {
        let mut event = AuditEvent::new(
            AuditEventKind::HttpRequest,
            AuditSeverity::Informational,
            "surreal-integration".into(),
        );
        event.metadata = Some(serde_json::json!({"schema":"project","revision":revision}));
        let event = chain.seal(event);
        storage.append(&event).await.expect("persist audit event");
        events.push(event);
    }
    assert_eq!(
        serde_json::to_value(storage.query_sequence(1, 3, 10).await.expect("read chain"))
            .expect("serialize persisted events"),
        serde_json::to_value(events).expect("serialize original events")
    );
    assert_eq!(
        storage
            .verify_chain_range(1, 3)
            .await
            .expect("verify chain"),
        AuditVerification::Consistent
    );
    assert_eq!(
        storage.verify_chain(1).await.expect("verify all events"),
        None
    );
    // Root deliberately bypasses table permissions to simulate privileged tampering.
    client
        .query("UPDATE audit_events SET metadata = '{\"tampered\":true}' WHERE sequence = 2")
        .await
        .expect("submit corruption")
        .check()
        .expect("complete deliberate privileged corruption");
    assert_eq!(
        storage.verify_chain(1).await.expect("detect corruption"),
        Some(2)
    );
}
