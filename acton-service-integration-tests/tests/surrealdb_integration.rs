#![cfg(feature = "surrealdb")]

use acton_service::{
    audit::{
        storage::{
            surrealdb_impl::SurrealAuditStorage, AuditOrder, AuditQuery, AuditStorage,
            AuditVerification,
        },
        AuditChain, AuditEvent, AuditEventKind, AuditSeverity, AuditSource,
    },
    config::SurrealDbConfig,
};
use chrono::DateTime;
use std::sync::Arc;
use testcontainers_modules::{
    surrealdb::SurrealDb,
    testcontainers::{runners::AsyncRunner, ImageExt},
};

#[tokio::test]
async fn provisioned_surreal_authentication_and_audit_integrity() {
    // Validate the remote SDK against an older authenticated server.
    // The fixture sets SURREAL_PATH=memory, so all storage is ephemeral.
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
    let http_config = SurrealDbConfig {
        url: format!("http://{host}:{port}"),
        ..config.clone()
    };
    let http_client = acton_service_surrealdb::create_client(&http_config)
        .await
        .expect("authenticate provisioned root through HTTP");
    let answer: Option<i64> = http_client
        .query("RETURN 42;")
        .await
        .expect("execute HTTP query")
        .take(0)
        .expect("decode HTTP query response");
    assert_eq!(answer, Some(42));
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
    assert_audit_paging_verification_and_retention(client.clone()).await;
    assert_legacy_archives_round_trip(client).await;
}

async fn assert_audit_paging_verification_and_retention(
    client: Arc<acton_service_surrealdb::SurrealClient>,
) {
    client.use_db("audit_paging").await.unwrap();
    let storage = SurrealAuditStorage::new(client);
    storage.initialize().await.unwrap();
    storage.initialize().await.unwrap();
    let mut chain = AuditChain::new("surreal-test".into());
    let mut events = Vec::new();
    for offset in 0..5 {
        let mut event = AuditEvent::new(
            AuditEventKind::HttpRequest,
            AuditSeverity::Informational,
            "surreal-test".into(),
        );
        event.timestamp = DateTime::from_timestamp(1_700_000_000 + offset, 123_456_789).unwrap();
        event.metadata = Some(
            serde_json::json!({"schema":"case","action":"read","nested":{"value":3,"tags":["one","two"],"optional":null}}),
        );
        event.source = AuditSource {
            ip: Some("127.0.0.1".into()),
            user_agent: Some("regression-client/1".into()),
            subject: Some("user_test".into()),
            request_id: Some("req_test".into()),
        };
        event.method = Some("PATCH".into());
        event.path = Some("/entities/project/one?revision=2".into());
        event.status_code = Some(200);
        event.duration_ms = Some(123);
        event.kind = AuditEventKind::Custom("project.updated".into());
        if offset == 0 {
            event.id = "dca93650-9d2c-4ca8-a00f-79a63467c187".parse().unwrap();
        }
        if offset == 4 {
            event.kind = AuditEventKind::AuthTokenValidated;
        }
        let event = chain.seal(event);
        storage.append(&event).await.unwrap();
        events.push(event);
    }
    assert!(
        storage.append(&events[0]).await.is_err(),
        "duplicate record statement errors must reach the caller"
    );
    assert_eq!(
        storage
            .query_filtered(&Default::default())
            .await
            .unwrap()
            .iter()
            .map(|e| e.sequence)
            .collect::<Vec<_>>(),
        vec![5, 4, 3, 2, 1]
    );
    let mut q = AuditQuery {
        limit: 2,
        through_sequence: Some(4),
        actor: Some("user_test".into()),
        request_id: Some("req_test".into()),
        from: Some(events[0].timestamp),
        to: Some(events[4].timestamp),
        ..Default::default()
    };
    let first = storage.query_filtered(&q).await.unwrap();
    assert_eq!(
        first.iter().map(|e| e.sequence).collect::<Vec<_>>(),
        vec![4, 3]
    );
    q.cursor = Some(3);
    assert_eq!(
        storage
            .query_filtered(&q)
            .await
            .unwrap()
            .iter()
            .map(|e| e.sequence)
            .collect::<Vec<_>>(),
        vec![2, 1]
    );
    q.order = AuditOrder::OldestFirst;
    assert_eq!(
        storage
            .query_filtered(&q)
            .await
            .unwrap()
            .iter()
            .map(|e| e.sequence)
            .collect::<Vec<_>>(),
        vec![4]
    );
    q.actor = None;
    q.schema = Some("case".into());
    q.metadata_kinds = Some(vec![]);
    assert!(storage.query_filtered(&q).await.unwrap().is_empty());
    q.metadata_kinds = Some(vec!["custom.project.updated".into()]);
    assert_eq!(storage.query_filtered(&q).await.unwrap().len(), 1);
    q.schema = Some("does-not-exist".into());
    assert!(storage.query_filtered(&q).await.unwrap().is_empty());
    let last = storage.latest().await.unwrap().unwrap();
    assert_eq!(last.id, events[4].id);
    assert_eq!(last.timestamp, events[4].timestamp);
    assert_eq!(last.metadata, events[4].metadata);
    assert_eq!(
        serde_json::to_value(&last).unwrap(),
        serde_json::to_value(&events[4]).unwrap()
    );
    assert_eq!(storage.sequence_bounds().await.unwrap(), Some((1, 5)));
    assert_eq!(
        storage
            .query_sequence(2, 4, 2)
            .await
            .unwrap()
            .iter()
            .map(|e| e.sequence)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
    assert_eq!(
        storage.verify_chain_range(2, 4).await.unwrap(),
        AuditVerification::Consistent
    );
    assert_eq!(storage.verify_chain(1).await.unwrap(), None);
    assert_eq!(
        storage
            .query_before(events[2].timestamp, 10)
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(storage.purge_before(events[2].timestamp).await.unwrap(), 2);
    assert_eq!(storage.sequence_bounds().await.unwrap(), Some((3, 5)));
    assert_eq!(
        storage.verify_chain_range(3, 5).await.unwrap(),
        AuditVerification::Incomplete
    );
    assert_eq!(
        storage.verify_chain_range(4, 5).await.unwrap(),
        AuditVerification::Consistent
    );
}
async fn assert_legacy_archives_round_trip(client: Arc<acton_service_surrealdb::SurrealClient>) {
    for (database, archived) in [
        (
            "legacy_v1",
            include_str!("../../acton-service-surrealdb/src/fixtures/legacy-v1.json"),
        ),
        (
            "legacy_v2",
            include_str!("../../acton-service-surrealdb/src/fixtures/legacy-v2.json"),
        ),
    ] {
        client.use_db(database).await.unwrap();
        let storage = SurrealAuditStorage::new(client.clone());
        storage.initialize().await.unwrap();
        let legacy: AuditEvent = serde_json::from_str(archived).unwrap();
        storage.append(&legacy).await.unwrap();
        let restored = storage.latest().await.unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(&restored).unwrap(),
            serde_json::to_value(&legacy).unwrap()
        );
        let mut chain =
            AuditChain::resume(legacy.service_name.clone(), legacy.hash.clone().unwrap(), 1);
        let modern = chain.seal(AuditEvent::new(
            AuditEventKind::HttpRequest,
            AuditSeverity::Informational,
            legacy.service_name,
        ));
        assert_eq!(modern.id.as_uuid().get_version_num(), 7);
        storage.append(&modern).await.unwrap();
        assert_eq!(
            storage.verify_chain_range(1, 2).await.unwrap(),
            AuditVerification::Consistent
        );
    }
}
