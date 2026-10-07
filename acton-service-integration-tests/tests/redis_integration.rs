#![cfg(feature = "cache")]

use acton_service::middleware::{revocation::RedisTokenRevocation, TokenRevocation};
use testcontainers_modules::{
    redis::Redis,
    testcontainers::{core::ExecCommand, runners::AsyncRunner, ImageExt},
};
#[path = "common/revocation.rs"]
mod revocation_contract;

#[tokio::test(flavor = "multi_thread")]
async fn provisioned_redis_revocation_contract() {
    tokio::time::timeout(std::time::Duration::from_secs(90), fixture())
        .await
        .expect("Redis fixture completes in 90 seconds");
}
async fn fixture() {
    let container = Redis::default()
        .with_tag("7.4-alpine")
        .with_startup_timeout(std::time::Duration::from_secs(60))
        .start()
        .await
        .expect("start Redis");
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(6379).await.unwrap();
    let config = acton_service::config::Config::<()> {
        redis: Some(
            serde_json::from_value(
                serde_json::json!({"url":format!("redis://{host}:{port}"),"max_retries":0,"lazy_init":false}),
            )
            .unwrap(),
        ),
        ..Default::default()
    };
    let state = acton_service::state::AppState::builder()
        .config(config.clone())
        .build()
        .await
        .expect("production Redis pool");
    let pool = state.redis().await.expect("Redis pool available");
    let key = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(key.path(), [42_u8; 32]).unwrap();
    let configured = |namespace: &str| {
        let config = acton_service::config::Config {
            token: Some(acton_service::config::TokenConfig::Paseto(
                acton_service::config::PasetoConfig {
                    key_path: key.path().into(),
                    ..Default::default()
                },
            )),
            revocation: Some(acton_service::config::RevocationConfig {
                backend: acton_service::config::RevocationBackend::Redis,
                namespace: namespace.into(),
            }),
            ..config.clone()
        };
        acton_service::prelude::ServiceBuilder::new()
            .with_config(config)
            .with_state(state.clone())
            .try_build()
            .unwrap()
    };
    let short = configured("a");
    let colon = configured("a:b");
    let short = short.state().token_revocation().unwrap();
    let colon = colon.state().token_revocation().unwrap();
    short.revoke("b:x", 3600).await.unwrap();
    assert!(
        !colon.is_revoked("x").await.unwrap(),
        "namespace and jti delimiters must not collide"
    );
    short.revoke_subject("user:x", 100).await.unwrap();
    assert_eq!(colon.subject_not_before("user:x").await.unwrap(), None);
    let storage = RedisTokenRevocation::with_prefix(pool.clone(), "fixture-a:");
    let isolated = RedisTokenRevocation::with_prefix(pool, "fixture-b:");
    revocation_contract::contract(&storage, &isolated).await;
    let mut seeded = container
        .exec(ExecCommand::new([
            "redis-cli",
            "SET",
            "fixture-a:precision",
            "1",
            "PX",
            "2600",
        ]))
        .await
        .unwrap();
    assert_eq!(
        String::from_utf8(seeded.stdout_to_vec().await.unwrap())
            .unwrap()
            .trim(),
        "OK"
    );
    let started = std::time::Instant::now();
    storage.revoke("precision", 3).await.unwrap();
    let mut result = container
        .exec(ExecCommand::new([
            "redis-cli",
            "PTTL",
            "fixture-a:precision",
        ]))
        .await
        .unwrap();
    let remaining = String::from_utf8(result.stdout_to_vec().await.unwrap())
        .unwrap()
        .trim()
        .parse::<i64>()
        .unwrap();
    let elapsed = i64::try_from(started.elapsed().as_millis()).unwrap();
    assert!(remaining>=3000-elapsed-25 && remaining<=3000,"requested lifetime must extend the marker in milliseconds: remaining={remaining}, elapsed={elapsed}");
    assert!(
        storage.revoke("subject-cutoffs", 3600).await.is_err(),
        "reserved hash key must never be replaced by a token marker"
    );
    container.stop().await.expect("stop Redis fixture");
    assert!(
        storage.is_revoked("revocation-token").await.is_err(),
        "connection failure must not be mistaken for an unrevoked token"
    );
}
