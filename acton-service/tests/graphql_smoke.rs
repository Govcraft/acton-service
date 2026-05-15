//! Integration smoke test for the GraphQL transport.
//!
//! Builds a router using the same `build_router` entry-point that
//! `ServiceBuilder::build` calls when GraphQL is registered, then hits it with
//! `tower::ServiceExt::oneshot` to verify:
//!
//!   * POST returns the expected JSON for a registered query
//!   * GET serves GraphiQL (HTML)
//!   * unregistered versions 404

#![cfg(feature = "graphql")]

use acton_service::graphql::{mount, VersionedGraphQLBuilder};
use acton_service::prelude::*;
use async_graphql::{EmptyMutation, EmptySubscription, Object, Schema};
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use tower::ServiceExt;

struct Query;

#[Object]
impl Query {
    async fn ping(&self) -> &'static str {
        "pong"
    }
}

fn build_app() -> axum::Router<()> {
    let schema = Schema::build(Query, EmptyMutation, EmptySubscription).finish();
    let graphql = VersionedGraphQLBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, schema)
        .build();
    mount::build_router(
        graphql,
        None,
        #[cfg(feature = "graphql-cedar")]
        None,
    )
    .expect("graphql router")
}

#[tokio::test]
async fn post_executes_query() {
    let app = build_app();
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/graphql")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"query":"{ ping }"}"#))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"]["ping"], "pong");
}

#[tokio::test]
async fn get_serves_graphiql() {
    let app = build_app();
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/graphql")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), 256 * 1024).await.unwrap();
    let html = std::str::from_utf8(&body).unwrap();
    assert!(
        html.contains("GraphiQL") || html.contains("graphiql"),
        "GET should serve GraphiQL HTML, got: {}",
        &html[..html.len().min(200)]
    );
}

#[tokio::test]
async fn unregistered_version_returns_404() {
    let app = build_app();
    let req = Request::builder()
        .method("POST")
        .uri("/api/v2/graphql")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"query":"{ ping }"}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn graphiql_disabled_blocks_get() {
    use acton_service::config::GraphQLConfig;
    let schema = Schema::build(Query, EmptyMutation, EmptySubscription).finish();
    let graphql = VersionedGraphQLBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, schema)
        .build();
    let cfg = GraphQLConfig {
        graphiql_enabled: false,
        ..GraphQLConfig::default()
    };
    let app = mount::build_router(
        graphql,
        Some(&cfg),
        #[cfg(feature = "graphql-cedar")]
        None,
    )
    .expect("router");
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/graphql")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::METHOD_NOT_ALLOWED,
        "GraphiQL should not be served when graphiql_enabled = false"
    );
}
