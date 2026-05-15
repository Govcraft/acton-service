//! Tests that resolver-level Cedar authorization gates field access using the
//! same `CedarAuthz` instance the HTTP middleware would use.

#![cfg(feature = "graphql-cedar")]

use std::io::Write;

use acton_service::config::CedarConfig;
use acton_service::graphql::{mount, CedarResolverCheck, VersionedGraphQLBuilder};
use acton_service::middleware::cedar::CedarAuthz;
use acton_service::middleware::Claims;
use acton_service::prelude::*;
use async_graphql::{Context, EmptyMutation, EmptySubscription, Object, Schema};
use axum::{
    body::{to_bytes, Body},
    extract::Request,
    http::{Request as HttpRequest, StatusCode},
    middleware::Next,
};
use tower::ServiceExt;

struct Query;

#[Object]
impl Query {
    async fn document(&self, ctx: &Context<'_>, id: String) -> async_graphql::Result<String> {
        CedarResolverCheck::for_context(ctx)?
            .with_action("readDocument")
            .with_resource_type("Document")
            .with_resource_id(&id)
            .authorize()
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(format!("Document {} contents", id))
    }
}

const POLICY: &str = r#"
permit(
    principal == User::"user:reader",
    action == Action::"readDocument",
    resource
);

forbid(
    principal == User::"user:blocked",
    action,
    resource
);
"#;

fn write_policy_file() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("policies.cedar");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(POLICY.as_bytes()).unwrap();
    (dir, path)
}

async fn build_cedar() -> (CedarAuthz, tempfile::TempDir) {
    let (dir, path) = write_policy_file();
    let cfg = CedarConfig {
        enabled: true,
        policy_path: path,
        hot_reload: false,
        hot_reload_interval_secs: 60,
        cache_enabled: false,
        cache_ttl_secs: 60,
        fail_open: false,
    };
    let cedar = CedarAuthz::from_config(cfg).await.unwrap();
    (cedar, dir)
}

fn build_app(cedar: CedarAuthz, claims: Option<Claims>) -> axum::Router<()> {
    let schema = Schema::build(Query, EmptyMutation, EmptySubscription).finish();
    let graphql = VersionedGraphQLBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, schema)
        .build();
    let app = mount::build_router(graphql, None, Some(cedar)).unwrap();

    if let Some(c) = claims {
        app.layer(axum::middleware::from_fn(
            move |mut req: Request, next: Next| {
                let claims = c.clone();
                async move {
                    req.extensions_mut().insert(claims);
                    next.run(req).await
                }
            },
        ))
    } else {
        app
    }
}

fn make_claims(sub: &str) -> Claims {
    Claims {
        sub: sub.to_string(),
        email: None,
        username: None,
        roles: vec![],
        perms: vec![],
        exp: 0,
        iat: None,
        jti: None,
        iss: None,
        aud: None,
        custom: Default::default(),
    }
}

async fn graphql_post(app: axum::Router<()>, q: &str) -> serde_json::Value {
    let req = HttpRequest::builder()
        .method("POST")
        .uri("/api/v1/graphql")
        .header("content-type", "application/json")
        .body(Body::from(format!(r#"{{"query":{:?}}}"#, q)))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn allowed_principal_reaches_document() {
    let (cedar, _dir) = build_cedar().await;
    let app = build_app(cedar, Some(make_claims("user:reader")));
    let json = graphql_post(app, r#"{ document(id: "abc") }"#).await;
    assert_eq!(json["data"]["document"], "Document abc contents");
}

#[tokio::test]
async fn denied_principal_is_forbidden() {
    let (cedar, _dir) = build_cedar().await;
    let app = build_app(cedar, Some(make_claims("user:blocked")));
    let json = graphql_post(app, r#"{ document(id: "abc") }"#).await;
    assert!(
        json["errors"].is_array(),
        "expected resolver error, got: {}",
        json
    );
    let msg = json["errors"][0]["message"].as_str().unwrap_or_default();
    assert!(
        msg.to_lowercase().contains("forbidden") || msg.to_lowercase().contains("access denied"),
        "expected forbidden message, got: {}",
        msg
    );
}

#[tokio::test]
async fn unauthenticated_request_is_rejected() {
    let (cedar, _dir) = build_cedar().await;
    let app = build_app(cedar, None);
    let json = graphql_post(app, r#"{ document(id: "abc") }"#).await;
    assert!(
        json["errors"].is_array(),
        "expected unauthenticated error, got: {}",
        json
    );
    let msg = json["errors"][0]["message"].as_str().unwrap_or_default();
    assert!(
        msg.to_lowercase().contains("authentic") || msg.to_lowercase().contains("anonym"),
        "expected auth-related error message, got: {}",
        msg
    );
}
