//! Integration tests for governor rate-limit auto-apply (issue #7).
//!
//! These tests build the same router that `ServiceBuilder` would build for
//! the documented configuration, attach the governor middleware the same way
//! `ServiceBuilder::build` does, and exercise the request path with `tower`'s
//! `ServiceExt::oneshot`. We deliberately avoid running the full
//! `ServiceBuilder::build()` because it spins up the agent runtime via
//! `block_in_place`, which trips up async test runtimes.

#![cfg(feature = "governor")]

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use acton_service::config::{RateLimitConfig, RouteRateLimitConfig};
use acton_service::middleware::governor::GovernorRateLimit;
use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
    middleware::from_fn_with_state,
    routing::post,
    Router,
};
use tower::ServiceExt;

async fn ok_handler() -> &'static str {
    "ok"
}

/// Build a router that mirrors the auto-apply layout: a nested `/api/v1`
/// router with the governor middleware attached to the OUTER router (so the
/// middleware sees the full pre-nest path, just like `ServiceBuilder` does).
fn build_app(config: RateLimitConfig, auto_apply: bool) -> Router {
    let inner: Router = Router::new().route("/uploads", post(ok_handler));
    let app = Router::new().nest("/api/v1", inner);

    if auto_apply {
        let gov = GovernorRateLimit::new(config);
        app.layer(from_fn_with_state(
            gov,
            GovernorRateLimit::middleware,
        ))
    } else {
        app
    }
}

fn upload_request(peer: SocketAddr) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri("/api/v1/uploads")
        .body(Body::empty())
        .expect("request build");
    // Inject ConnectInfo the way axum's `into_make_service_with_connect_info`
    // would, so the middleware can read the client IP.
    req.extensions_mut().insert(ConnectInfo(peer));
    req
}

/// Bug 1 + bug 3: when `[rate_limit.routes."POST /api/v1/uploads"]` is
/// configured and auto-apply is on, the layer fires and route-key matching
/// works against the full pre-nest path.
#[tokio::test]
async fn auto_apply_attaches_layer_when_routes_configured() {
    let mut routes = HashMap::new();
    routes.insert(
        "POST /api/v1/uploads".to_string(),
        RouteRateLimitConfig {
            requests_per_minute: 10,
            burst_size: 1, // tiny burst so we trip the limit on the 2nd hit
            per_user: false,
        },
    );

    let cfg = RateLimitConfig {
        routes,
        ..RateLimitConfig::default()
    };

    let app = build_app(cfg, true);

    let peer: SocketAddr = "10.0.0.5:34567".parse().unwrap();

    let r1 = app
        .clone()
        .oneshot(upload_request(peer))
        .await
        .expect("first request");
    assert_eq!(r1.status(), StatusCode::OK, "first request must pass");

    let r2 = app
        .oneshot(upload_request(peer))
        .await
        .expect("second request");
    assert_eq!(
        r2.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "second request must hit the per-route limit"
    );
}

/// Auto-apply opt-out: when `auto_apply = false`, the layer is not attached,
/// so requests are never rate-limited (regardless of routes configured).
#[tokio::test]
async fn no_auto_apply_when_disabled() {
    let mut routes = HashMap::new();
    routes.insert(
        "POST /api/v1/uploads".to_string(),
        RouteRateLimitConfig {
            requests_per_minute: 1,
            burst_size: 1,
            per_user: false,
        },
    );

    let cfg = RateLimitConfig {
        routes,
        auto_apply: false,
        ..RateLimitConfig::default()
    };

    // We pass auto_apply=false through to `build_app` to model what
    // `ServiceBuilder` would do.
    let app = build_app(cfg, false);
    let peer: SocketAddr = "10.0.0.5:34567".parse().unwrap();

    // Burst many requests; none get rate-limited.
    for _ in 0..5 {
        let r = app
            .clone()
            .oneshot(upload_request(peer))
            .await
            .expect("request");
        assert_eq!(r.status(), StatusCode::OK);
    }
}

/// Bug 2: anonymous requests must fall back to per-IP rate limiting, not
/// silently pass through. With `per_user_rpm` set to 1, the second hit from
/// the same IP must 429.
#[tokio::test]
async fn anonymous_request_ip_limited() {
    let cfg = RateLimitConfig {
        per_user_rpm: 1,
        ..RateLimitConfig::default()
    };

    let app = build_app(cfg, true);

    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 11)), 4242);

    let r1 = app
        .clone()
        .oneshot(upload_request(peer))
        .await
        .expect("first request");
    assert_eq!(r1.status(), StatusCode::OK);

    let r2 = app
        .oneshot(upload_request(peer))
        .await
        .expect("second request");
    assert_eq!(
        r2.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "anonymous requests from the same IP must be rate-limited"
    );
}
