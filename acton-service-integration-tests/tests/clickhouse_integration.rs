#![cfg(feature = "clickhouse")]

use acton_service::{
    audit::{
        storage::{clickhouse_impl::ClickHouseAuditStorage, AuditStorage, AuditVerification},
        AuditChain, AuditEvent, AuditEventKind, AuditSeverity,
    },
    config::ClickHouseConfig,
};
use chrono::DateTime;
use testcontainers_modules::{
    clickhouse::ClickHouse,
    testcontainers::{runners::AsyncRunner, ImageExt},
};

#[tokio::test]
async fn clickhouse_audit_persists_and_detects_corruption() {
    let container = ClickHouse::default()
        .with_startup_timeout(std::time::Duration::from_secs(60))
        .start()
        .await
        .expect("start ClickHouse container");
    let host = container.get_host().await.expect("container host");
    let port = container
        .get_host_port_ipv4(8123)
        .await
        .expect("ClickHouse HTTP port");
    let config = ClickHouseConfig {
        url: format!("http://{host}:{port}"),
        database: "default".into(),
        username: None,
        password: None,
        max_retries: 0,
        retry_delay_secs: 1,
        optional: false,
        lazy_init: false,
    };
    let client = acton_service_clickhouse::create_client(&config)
        .await
        .expect("connect via production client helper");
    let storage = ClickHouseAuditStorage::new(client.clone());
    storage.initialize().await.expect("initialize audit schema");
    storage
        .initialize()
        .await
        .expect("idempotent initialization");
    let mut chain = AuditChain::new("clickhouse-integration".into());
    let mut events = Vec::new();
    for offset in 0..3 {
        let mut event = AuditEvent::new(
            AuditEventKind::HttpRequest,
            AuditSeverity::Informational,
            "clickhouse-integration".into(),
        );
        event.timestamp = DateTime::from_timestamp(1_700_000_000 + offset, 123_000_000)
            .expect("valid test timestamp");
        event.metadata = Some(serde_json::json!({"schema":"project","nested":{"revision":offset}}));
        event.path = Some("/projects".into());
        event.status_code = Some(200);
        let event = chain.seal(event);
        storage.append(&event).await.expect("persist sealed event");
        events.push(event);
    }
    let persisted = storage.query_sequence(1, 3, 10).await.expect("read chain");
    assert_eq!(
        serde_json::to_value(&persisted).expect("serialize persisted events"),
        serde_json::to_value(&events).expect("serialize original events")
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
    assert_eq!(
        storage
            .latest()
            .await
            .expect("latest event")
            .expect("nonempty chain")
            .sequence,
        3
    );
    client
        .query("ALTER TABLE audit_events UPDATE metadata = '{\"tampered\":true}' WHERE sequence = 2 SETTINGS mutations_sync = 1")
        .execute()
        .await
        .expect("complete deliberate privileged corruption");
    assert_eq!(
        storage.verify_chain(1).await.expect("detect corruption"),
        Some(2)
    );
}
