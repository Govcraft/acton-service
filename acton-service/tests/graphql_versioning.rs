//! Tests that each registered API version is reachable only at its own path
//! and that the schema returned at each path matches the one registered for
//! that version.

#![cfg(feature = "graphql")]

use acton_service::graphql::{mount, VersionedGraphQLBuilder};
use acton_service::prelude::*;
use async_graphql::{EmptyMutation, EmptySubscription, Object, Schema};
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use tower::ServiceExt;

struct V1Query;
#[Object]
impl V1Query {
    async fn version(&self) -> &'static str {
        "v1"
    }
}

struct V2Query;
#[Object]
impl V2Query {
    async fn version(&self) -> &'static str {
        "v2"
    }
    async fn v2_only(&self) -> bool {
        true
    }
}

fn build_app() -> axum::Router<()> {
    let v1 = Schema::build(V1Query, EmptyMutation, EmptySubscription).finish();
    let v2 = Schema::build(V2Query, EmptyMutation, EmptySubscription).finish();
    let graphql = VersionedGraphQLBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, v1)
        .add_version(ApiVersion::V2, v2)
        .build();
    mount::build_router(
        graphql,
        None,
        #[cfg(feature = "graphql-cedar")]
        None,
    )
    .unwrap()
}

async fn query(app: axum::Router<()>, path: &str, q: &str) -> (StatusCode, serde_json::Value) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(format!(r#"{{"query":{:?}}}"#, q)))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let body = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    let json = if body.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null)
    };
    (status, json)
}

#[tokio::test]
async fn v1_endpoint_returns_v1_schema() {
    let (status, json) = query(build_app(), "/api/v1/graphql", "{ version }").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["data"]["version"], "v1");
}

#[tokio::test]
async fn v2_endpoint_returns_v2_schema() {
    let (status, json) = query(build_app(), "/api/v2/graphql", "{ version v2Only }").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["data"]["version"], "v2");
    assert_eq!(json["data"]["v2Only"], true);
}

#[tokio::test]
async fn v1_does_not_expose_v2_fields() {
    // v2Only is not defined on V1Query so this must fail validation.
    let (status, json) = query(build_app(), "/api/v1/graphql", "{ v2Only }").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        json["errors"].is_array(),
        "expected validation errors for v2-only field on v1 endpoint, got: {}",
        json
    );
}

#[tokio::test]
async fn base_path_is_applied() {
    let v1 = Schema::build(V1Query, EmptyMutation, EmptySubscription).finish();
    let graphql = VersionedGraphQLBuilder::new()
        .add_version(ApiVersion::V1, v1)
        .build();
    let app = mount::build_router(
        graphql,
        None,
        #[cfg(feature = "graphql-cedar")]
        None,
    )
    .unwrap();

    // Without base_path, endpoint is /v1/graphql.
    let (status, _) = query(app.clone(), "/v1/graphql", "{ version }").await;
    assert_eq!(status, StatusCode::OK);

    // /api/v1/graphql should not exist.
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/graphql")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"query":"{ version }"}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deprecated_version_emits_headers() {
    use acton_service::versioning::DeprecationInfo;
    let v1 = Schema::build(V1Query, EmptyMutation, EmptySubscription).finish();
    let v2 = Schema::build(V2Query, EmptyMutation, EmptySubscription).finish();
    let deprecation = DeprecationInfo::new(ApiVersion::V1, ApiVersion::V2)
        .with_sunset_date("2027-01-01T00:00:00Z")
        .with_message("upgrade to v2");

    let graphql = VersionedGraphQLBuilder::new()
        .with_base_path("/api")
        .add_version_deprecated(ApiVersion::V1, v1, deprecation)
        .add_version(ApiVersion::V2, v2)
        .build();
    let app = mount::build_router(
        graphql,
        None,
        #[cfg(feature = "graphql-cedar")]
        None,
    )
    .unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/graphql")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"query":"{ version }"}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let headers = resp.headers();
    assert!(
        headers.contains_key("deprecation"),
        "expected Deprecation header on deprecated GraphQL version"
    );
    assert_eq!(
        headers.get("sunset").map(|v| v.to_str().unwrap()),
        Some("2027-01-01T00:00:00Z")
    );
}
