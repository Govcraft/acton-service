//! Production PostgreSQL pool and audit storage, including verified TLS.
#![cfg(feature = "postgres")]

#[path = "common/revocation.rs"]
mod revocation_contract;

use acton_service::{
    audit::{
        storage::{pg::PgAuditStorage, AuditOrder, AuditQuery, AuditStorage, AuditVerification},
        AuditChain, AuditEvent, AuditEventKind, AuditSeverity,
    },
    config::DatabaseConfig,
};
use chrono::DateTime;
use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair, KeyUsagePurpose};
use std::time::Duration;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{runners::AsyncRunner, CopyTargetOptions, ImageExt},
};

struct ServerIdentity {
    ca_pem: String,
    cert_pem: String,
    key_pem: String,
}

fn server_identity() -> ServerIdentity {
    let ca_key = KeyPair::generate().expect("generate CA key");
    let mut ca_params = CertificateParams::new(Vec::new()).expect("CA parameters");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let ca_cert = ca_params.self_signed(&ca_key).expect("sign CA");
    let server_key = KeyPair::generate().expect("server key");
    let server_params = CertificateParams::new(vec!["localhost".into(), "127.0.0.1".into()])
        .expect("server parameters");
    let server_cert = server_params
        .signed_by(&server_key, &Issuer::new(ca_params, ca_key))
        .expect("sign server certificate");
    ServerIdentity {
        ca_pem: ca_cert.pem(),
        cert_pem: server_cert.pem(),
        key_pem: server_key.serialize_pem(),
    }
}

#[tokio::test]
async fn postgres_audit_over_verified_tls() {
    tokio::time::timeout(Duration::from_secs(120), postgres_fixture())
        .await
        .expect("PostgreSQL integration completes within 120 seconds");
}

async fn postgres_fixture() {
    let identity = server_identity();
    let dir = tempfile::tempdir().expect("certificate directory");
    let ca_path = dir.path().join("ca.pem");
    std::fs::write(&ca_path, identity.ca_pem).expect("write trusted CA");
    let container = Postgres::default()
        .with_tag("16-alpine")
        .with_copy_to("/tmp/server.pem", identity.cert_pem.into_bytes())
        .with_copy_to(
            CopyTargetOptions::new("/tmp/server.key").with_mode(0o600),
            identity.key_pem.into_bytes(),
        )
        .with_cmd([
            "sh",
            "-c",
            "chown postgres:postgres /tmp/server.key /tmp/server.pem; exec docker-entrypoint.sh postgres -c ssl=on -c ssl_cert_file=/tmp/server.pem -c ssl_key_file=/tmp/server.key -c fsync=off",
        ])
        .with_startup_timeout(Duration::from_secs(90))
        .start()
        .await
        .expect("start TLS PostgreSQL");
    let host = container.get_host().await.expect("database host");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("database port");
    let config: DatabaseConfig = serde_json::from_value(serde_json::json!({
        "url": format!("postgres://postgres:postgres@{host}:{port}/postgres?sslmode=verify-full&sslrootcert={}", ca_path.display()),
        "max_connections": 1, "min_connections": 1,
        "max_retries": 0, "connection_timeout_secs": 5
    }))
    .expect("pool configuration");
    let pool = acton_service_postgres::database::create_pool(&(&config).into())
        .await
        .expect("verified TLS pool");
    let revocation = acton_service::middleware::revocation::PgTokenRevocation::new(
        pool.clone(),
        acton_service::middleware::revocation::RevocationNamespace::new("fixture-a")
            .expect("namespace"),
    );
    let isolated = acton_service::middleware::revocation::PgTokenRevocation::new(
        pool.clone(),
        acton_service::middleware::revocation::RevocationNamespace::new("fixture-b")
            .expect("namespace"),
    );
    assert!(
        acton_service::middleware::TokenRevocation::is_revoked(&revocation, "before-migration")
            .await
            .is_err(),
        "missing revocation schema must fail closed"
    );
    revocation_contract::contract(&revocation, &isolated).await;
    assert!(sqlx::query_scalar::<_, bool>(
        "SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()"
    )
    .fetch_one(&pool)
    .await
    .expect("TLS session status"));
    let untrusted_path = dir.path().join("untrusted.pem");
    std::fs::write(&untrusted_path, server_identity().ca_pem).expect("write untrusted CA");
    let mut untrusted = config.clone();
    untrusted.url = config.url.replace(
        &ca_path.display().to_string(),
        &untrusted_path.display().to_string(),
    );
    assert!(
        acton_service_postgres::database::create_pool(&(&untrusted).into())
            .await
            .is_err(),
        "untrusted server certificate must fail"
    );
    // Rules in another schema must not prevent this schema's protections.
    sqlx::query("CREATE SCHEMA audit_shadow")
        .execute(&pool)
        .await
        .expect("create second schema");
    sqlx::query("SET search_path TO audit_shadow")
        .execute(&pool)
        .await
        .expect("select second schema");
    PgAuditStorage::new(pool.clone())
        .initialize()
        .await
        .expect("initialize shadow protections");
    sqlx::query("SET search_path TO public")
        .execute(&pool)
        .await
        .expect("select application schema");
    let storage = PgAuditStorage::new(pool.clone());
    storage.initialize().await.expect("audit schema");
    let mut chain = AuditChain::new("postgres-tls-test".into());
    let mut events = Vec::new();
    for offset in 0..5 {
        let mut event = AuditEvent::new(
            AuditEventKind::HttpRequest,
            AuditSeverity::Informational,
            "postgres-tls-test".into(),
        );
        event.timestamp =
            DateTime::from_timestamp(1_700_000_000 + offset, 123_456_789).expect("fixed timestamp");
        event.metadata =
            Some(serde_json::json!({"actor":"actor_a","schema":"case","entity_id":"case_a"}));
        event.source.request_id = Some("req_a".into());
        event.status_code = Some(403);
        let event = chain.seal(event);
        storage.append(&event).await.expect("persist sealed event");
        let loaded = storage
            .latest()
            .await
            .expect("load event")
            .expect("stored event");
        assert_eq!(loaded.timestamp, event.timestamp);
        assert_eq!(loaded.hash, event.hash);
        events.push(event);
    }
    assert_eq!(
        storage.sequence_bounds().await.expect("sequence bounds"),
        Some((1, 5))
    );
    let mut query = AuditQuery {
        limit: 2,
        through_sequence: Some(4),
        actor: Some("actor_a".into()),
        schema: Some("case".into()),
        entity_id: Some("case_a".into()),
        request_id: Some("req_a".into()),
        status_code: Some(403),
        ..Default::default()
    };
    assert_eq!(
        storage
            .query_filtered(&query)
            .await
            .expect("filtered events")
            .iter()
            .map(|e| e.sequence)
            .collect::<Vec<_>>(),
        vec![4, 3]
    );
    query.cursor = Some(3);
    assert_eq!(
        storage
            .query_filtered(&query)
            .await
            .expect("cursor events")
            .iter()
            .map(|e| e.sequence)
            .collect::<Vec<_>>(),
        vec![2, 1]
    );
    query.order = AuditOrder::OldestFirst;
    assert_eq!(
        storage
            .query_filtered(&query)
            .await
            .expect("ascending events")
            .iter()
            .map(|e| e.sequence)
            .collect::<Vec<_>>(),
        vec![4]
    );
    query.actor = Some("actor_a' OR 1=1 --".into());
    assert!(storage
        .query_filtered(&query)
        .await
        .expect("escaped actor filter")
        .is_empty());
    assert_eq!(
        storage
            .verify_chain_range(2, 4)
            .await
            .expect("bounded verification"),
        AuditVerification::Consistent
    );
    assert_eq!(
        storage
            .verify_chain(1)
            .await
            .expect("complete verification"),
        None
    );
    assert_eq!(
        sqlx::query("UPDATE audit_events SET path = '/blocked' WHERE sequence = 4")
            .execute(&pool)
            .await
            .expect("immutability rule")
            .rows_affected(),
        0
    );
    assert_eq!(
        sqlx::query("DELETE FROM audit_events WHERE sequence = 5")
            .execute(&pool)
            .await
            .expect("deletion rule")
            .rows_affected(),
        0
    );
    assert_eq!(
        storage
            .purge_before(events[2].timestamp)
            .await
            .expect("retention purge"),
        2
    );
    assert!(
        storage.verify_chain(3).await.is_err(),
        "missing predecessor cannot prove integrity"
    );
    assert_eq!(
        storage.verify_chain(4).await.expect("retained suffix"),
        None
    );
    sqlx::query("DROP RULE audit_no_update ON audit_events")
        .execute(&pool)
        .await
        .expect("privileged mutation setup");
    sqlx::query("UPDATE audit_events SET path = '/tampered' WHERE sequence = 4")
        .execute(&pool)
        .await
        .expect("simulate privileged tampering");
    assert_eq!(
        storage.verify_chain(4).await.expect("detect tampering"),
        Some(4)
    );
    pool.close().await;
}
