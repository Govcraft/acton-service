//! Tests that authenticated `Claims` placed into request extensions by
//! upstream middleware (e.g. PASETO, JWT) make it through to GraphQL
//! resolvers via the `GraphQLContextExt::claims` accessor.
//!
//! Rather than spin up a full PASETO key pair, we inject claims with a thin
//! Axum middleware. That isolates the contract under test ("the GraphQL
//! transport propagates request extensions into the resolver context") from
//! the specifics of token format and validation.

#![cfg(feature = "graphql")]

use acton_service::graphql::{mount, GraphQLContextExt, VersionedGraphQLBuilder};
use acton_service::middleware::Claims;
use acton_service::prelude::*;
use async_graphql::{Context, EmptyMutation, EmptySubscription, Object, Schema};
use axum::{
    body::{to_bytes, Body},
    extract::Request,
    http::StatusCode,
    middleware::Next,
};
use tower::ServiceExt;

struct Query;

#[Object]
impl Query {
    async fn whoami(&self, ctx: &Context<'_>) -> String {
        ctx.claims()
            .map(|c| c.sub.clone())
            .unwrap_or_else(|| "anonymous".to_string())
    }

    async fn require_user(&self, ctx: &Context<'_>) -> async_graphql::Result<String> {
        Ok(ctx.require_claims()?.sub.clone())
    }
}

fn build_app_with_claims(claims: Option<Claims>) -> axum::Router<()> {
    let schema = Schema::build(Query, EmptyMutation, EmptySubscription).finish();
    let graphql = VersionedGraphQLBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, schema)
        .build();
    let app = mount::build_router(
        graphql,
        None,
        #[cfg(feature = "graphql-cedar")]
        None,
    )
    .unwrap();
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

fn sample_claims() -> Claims {
    Claims {
        sub: "user:42".to_string(),
        email: Some("u@example.com".into()),
        username: Some("u".into()),
        roles: vec!["editor".into()],
        perms: vec![],
        exp: 0,
        iat: None,
        jti: None,
        iss: None,
        aud: None,
        custom: Default::default(),
    }
}

async fn post(app: axum::Router<()>, q: &str) -> serde_json::Value {
    let req = axum::http::Request::builder()
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
async fn claims_reach_resolver() {
    let app = build_app_with_claims(Some(sample_claims()));
    let json = post(app, "{ whoami }").await;
    assert_eq!(json["data"]["whoami"], "user:42");
}

#[tokio::test]
async fn anonymous_request_has_no_claims() {
    let app = build_app_with_claims(None);
    let json = post(app, "{ whoami }").await;
    assert_eq!(json["data"]["whoami"], "anonymous");
}

#[tokio::test]
async fn require_claims_errors_when_anonymous() {
    let app = build_app_with_claims(None);
    let json = post(app, "{ requireUser }").await;
    assert!(
        json["errors"].is_array(),
        "expected unauthorized error, got: {}",
        json
    );
    let msg = json["errors"][0]["message"].as_str().unwrap_or_default();
    assert!(
        msg.to_lowercase().contains("unauth"),
        "error message should mention unauth, got: {}",
        msg
    );
}
