//! gRPC health reuses a single custom configuration and its live dependency state.
#![cfg(feature = "grpc")]

use std::time::Duration;

use acton_service::{config::Config, grpc::server::GrpcServicesBuilder, state::AppState};
use serde::{Deserialize, Serialize};
use tokio::{net::TcpListener, sync::oneshot};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;
use tonic_health::pb::{
    health_check_response::ServingStatus, health_client::HealthClient, HealthCheckRequest,
};

#[derive(Clone, Default, Deserialize, Serialize)]
struct CustomConfig {
    region: String,
}

const DEADLINE: Duration = Duration::from_secs(5);

async fn check_routes(routes: tonic::service::Routes, expected: Option<ServingStatus>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local address");
    let (stop, shutdown) = oneshot::channel();
    let server = tokio::spawn(async move {
        Server::builder()
            .add_routes(routes)
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = shutdown.await;
            })
            .await
    });

    let channel = tonic::transport::Channel::from_shared(format!("http://{addr}"))
        .expect("valid endpoint")
        .connect()
        .await
        .expect("connect to bound listener");
    let mut client = HealthClient::new(channel);
    let response = tokio::time::timeout(
        DEADLINE,
        client.check(HealthCheckRequest {
            service: String::new(),
        }),
    )
    .await
    .expect("health check answers");
    if let Some(expected) = expected {
        assert_eq!(
            response
                .expect("registered health service")
                .into_inner()
                .status,
            expected as i32
        );
        let mut watch = client
            .watch(HealthCheckRequest {
                service: String::new(),
            })
            .await
            .expect("registered health watch")
            .into_inner();
        let first = tokio::time::timeout(DEADLINE, watch.message())
            .await
            .expect("watch answers")
            .expect("valid stream")
            .expect("initial health status");
        assert_eq!(first.status, expected as i32);
        assert!(watch.message().await.expect("stream completes").is_none());
    } else {
        assert_eq!(
            response
                .expect_err("health must not be registered without state")
                .code(),
            tonic::Code::Unimplemented
        );
    }

    stop.send(()).expect("server waiting for shutdown");
    tokio::time::timeout(DEADLINE, server)
        .await
        .expect("server shuts down")
        .expect("server task succeeds")
        .expect("clean shutdown");
}

#[tokio::test]
async fn health_routes_use_a_single_loaded_custom_config() {
    let dir = tempfile::tempdir().expect("temporary config directory");
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "region = \"test-region\"\n[service]\nname = \"custom-grpc\"\n",
    )
    .expect("write config");
    let config = Config::<CustomConfig>::load_from(path.to_str().expect("UTF-8 path"))
        .expect("load custom configuration once");
    assert_eq!(config.custom.region, "test-region");
    let state = AppState::new(config);
    let routes = GrpcServicesBuilder::new().with_health().build(Some(state));
    check_routes(routes, Some(ServingStatus::Serving)).await;
}

#[cfg(feature = "database")]
#[tokio::test]
async fn custom_state_preserves_required_dependency_health() {
    let config = Config {
        database: Some(
            serde_json::from_value(serde_json::json!({ "url": "postgres://unused" }))
                .expect("required database configuration"),
        ),
        ..Config::<CustomConfig>::default()
    };
    // No database pool is installed: the required dependency must fail health.
    let state = AppState::new(config);
    let routes = GrpcServicesBuilder::new().with_health().build(Some(state));
    check_routes(routes, Some(ServingStatus::NotServing)).await;
}

#[tokio::test]
async fn omitted_state_skips_health_registration() {
    let routes = GrpcServicesBuilder::new().with_health().build::<()>(None);
    check_routes(routes, None).await;
}
