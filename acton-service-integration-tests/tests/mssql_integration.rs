#![cfg(feature = "mssql")]

use acton_service::{
    accounts::{
        storage::{mssql::MssqlAccountStorage, AccountStorage},
        types::{Account, AccountId, AccountStatus},
    },
    audit::{
        storage::{mssql::MssqlAuditStorage, AuditOrder, AuditQuery, AuditStorage},
        AuditChain, AuditEvent, AuditEventKind, AuditSeverity,
    },
    auth::{
        ApiKey, ApiKeyGenerator, ApiKeyPepper, ApiKeyStorage, KeyRotationStorage,
        MssqlApiKeyStorage, MssqlKeyRotationStorage, MssqlRefreshStorage, RefreshTokenMetadata,
        RefreshTokenStorage,
    },
    config::DatabaseConfig,
    mssql,
};
use chrono::{Duration, Utc};
use testcontainers_modules::{mssql_server::MssqlServer, testcontainers::runners::AsyncRunner};

#[tokio::test]
async fn mssql_backends_initialize_and_accounts_round_trip() {
    // Include a cold download and extraction of the pinned SQL Server image.
    tokio::time::timeout(std::time::Duration::from_secs(300), mssql_fixture())
        .await
        .expect("SQL Server integration completes within five minutes");
}

async fn mssql_fixture() {
    let container = MssqlServer::default()
        .with_accept_eula()
        .start()
        .await
        .expect("start SQL Server container");
    let host = container.get_host().await.expect("container host");
    let port = container
        .get_host_port_ipv4(1433)
        .await
        .expect("container port");
    let config=DatabaseConfig{url:format!("Server=tcp:{host},{port};Database=master;User Id=sa;Password={};TrustServerCertificate=True;",MssqlServer::DEFAULT_SA_PASSWORD),max_connections:5,min_connections:1,connection_timeout_secs:30,max_retries:10,retry_delay_secs:2,optional:false,lazy_init:false,mssql_auth:acton_service::config::MssqlAuthMode::ConnectionString};
    let pool = mssql::create_pool(&config).await.expect("SQL Server pool");
    mssql::health_check(&pool).await.expect("health query");
    let accounts = MssqlAccountStorage::new(pool.clone())
        .await
        .expect("accounts schema");
    let generator = ApiKeyGenerator::new("test", ApiKeyPepper::from_bytes([7; 32]));
    let api_keys = MssqlApiKeyStorage::new(pool.clone(), generator.clone())
        .await
        .expect("api key schema");
    let refresh = MssqlRefreshStorage::new(pool.clone())
        .await
        .expect("refresh schema");
    MssqlKeyRotationStorage::new(pool.clone())
        .initialize()
        .await
        .expect("key rotation schema");
    let audit = MssqlAuditStorage::new(pool.clone());
    audit.initialize().await.expect("audit schema");
    audit_round_trip_and_corruption(&pool, &audit).await;
    let now = Utc::now();
    let account:Account=serde_json::from_value(serde_json::json!({"id":AccountId::new().to_string(),"email":format!("{}@example.test",uuid::Uuid::new_v4()),"username":null,"password_hash":null,"status":AccountStatus::Active,"roles":["user"],"email_verified":true,"email_verified_at":now,"last_login_at":null,"locked_at":null,"locked_reason":null,"disabled_at":null,"disabled_reason":null,"expires_at":null,"password_changed_at":null,"failed_login_count":0,"metadata":{"backend":"mssql"},"created_at":now,"updated_at":now})).expect("account fixture");
    accounts.create(&account).await.expect("create account");
    let loaded = accounts
        .get_by_id(account.id.as_str())
        .await
        .expect("load account")
        .expect("account exists");
    assert_eq!(loaded.email, account.email);
    assert_eq!(loaded.roles, account.roles);
    let (plaintext, key_hash) = generator.generate().expect("generate API key");
    let key = ApiKey {
        id: uuid::Uuid::new_v4().to_string(),
        user_id: account.id.to_string(),
        name: "integration".to_string(),
        prefix: ApiKeyGenerator::key_prefix_for_lookup(&plaintext).expect("lookup prefix"),
        key_hash,
        scopes: vec!["read".to_string()],
        rate_limit: Some(100),
        is_revoked: false,
        last_used_at: None,
        expires_at: None,
        created_at: now,
    };
    api_keys.create(&key).await.expect("create API key");
    assert_eq!(
        api_keys
            .get_by_key(&plaintext)
            .await
            .expect("verify key")
            .expect("key exists")
            .id,
        key.id
    );
    assert!(api_keys
        .get_by_key(&format!("{plaintext}a"))
        .await
        .expect("mismatched key")
        .is_none());
    assert_eq!(
        api_keys
            .get_by_id(&key.id)
            .await
            .expect("load API key")
            .expect("API key exists")
            .scopes,
        key.scopes
    );
    let token_id = uuid::Uuid::new_v4().to_string();
    let metadata = RefreshTokenMetadata::default();
    refresh
        .store(
            &token_id,
            account.id.as_str(),
            "family",
            now + Duration::hours(1),
            &metadata,
        )
        .await
        .expect("store refresh token");
    assert_eq!(
        refresh
            .get(&token_id)
            .await
            .expect("load refresh token")
            .expect("refresh token exists")
            .user_id,
        account.id.as_str()
    );
    assert!(accounts
        .delete(account.id.as_str())
        .await
        .expect("delete account"));
}

async fn audit_round_trip_and_corruption(pool: &mssql::MssqlPool, storage: &MssqlAuditStorage) {
    let mut chain = AuditChain::new("mssql-verification-test".into());
    let mut events = Vec::new();
    for offset in 0..5 {
        let mut event = AuditEvent::new(
            AuditEventKind::AuthTokenValidated,
            AuditSeverity::Notice,
            "mssql-verification-test".into(),
        );
        event.timestamp =
            chrono::DateTime::from_timestamp(1_700_000_000 + offset, 0).expect("fixed timestamp");
        event.metadata =
            Some(serde_json::json!({"actor":"actor_a","schema":"case","entity_id":"case_a"}));
        event.source.request_id = Some("req_a".into());
        event.status_code = Some(403);
        let event = chain.seal(event);
        storage.append(&event).await.expect("persist audit event");
        let loaded = storage
            .latest()
            .await
            .expect("load event")
            .expect("stored event");
        assert_eq!(loaded.hash, event.hash);
        assert_eq!(loaded.timestamp, event.timestamp);
        events.push(event);
    }
    assert_eq!(storage.verify_chain(1).await.expect("complete chain"), None);
    let mut query = AuditQuery {
        limit: 2,
        through_sequence: Some(4),
        actor: Some("actor_a".into()),
        schema: Some("case".into()),
        entity_id: Some("case_a".into()),
        request_id: Some("req_a".into()),
        status_code: Some(403),
        metadata_kinds: Some(vec!["auth.token.validated".into()]),
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
    query.actor = Some("ACTOR_A".into());
    assert!(storage
        .query_filtered(&query)
        .await
        .expect("case sensitive actor")
        .is_empty());
    assert!(acton_service_mssql::mssql::execute(
        pool,
        "UPDATE audit_events SET path = '/blocked' WHERE sequence = 4",
        &[]
    )
    .await
    .is_err());
    assert!(acton_service_mssql::mssql::execute(
        pool,
        "DELETE FROM audit_events WHERE sequence = 5",
        &[]
    )
    .await
    .is_err());
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
    acton_service_mssql::mssql::execute(
        pool,
        "DISABLE TRIGGER audit_no_update ON audit_events",
        &[],
    )
    .await
    .expect("privileged mutation setup");
    acton_service_mssql::mssql::execute(
        pool,
        "UPDATE audit_events SET path = '/tampered' WHERE sequence = 4",
        &[],
    )
    .await
    .expect("privileged tampering");
    assert_eq!(
        storage.verify_chain(4).await.expect("detect tampering"),
        Some(4)
    );
}
