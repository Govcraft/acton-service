//! Integration tests for attaching resilience middleware to an axum `Router`.
//!
//! These cover the gap reported in issue #33: the circuit breaker and bulkhead
//! layers could not be attached to a `Router` at all. Beyond compiling, we assert
//! the breaker actually *opens*, since an inbound axum route is infallible and a
//! default (`Err`-only) failure classifier would silently never trip.
//!
//! Following the convention in `governor_integration.rs`, routers are driven with
//! `ServiceExt::oneshot` rather than a full server bind.

#![cfg(feature = "resilience")]

use std::time::Duration;

use acton_service::middleware::resilience::{apply_resilience, ResilienceConfig};
use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::get,
    Router,
};
use tower::ServiceExt;

/// Config with a tiny sliding window so a couple of requests can trip the breaker.
fn trip_fast_config() -> ResilienceConfig {
    let mut config = ResilienceConfig::new()
        .with_circuit_breaker(true)
        .with_circuit_breaker_threshold(0.5)
        // Bulkhead off: this suite isolates circuit-breaker behavior.
        .with_bulkhead(false);
    config.circuit_breaker_min_requests = 2;
    config.circuit_breaker_wait_duration = Duration::from_secs(60);
    config
}

async fn status_of(app: &Router, path: &str) -> StatusCode {
    app.clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router is infallible after apply_resilience")
        .status()
}

#[tokio::test]
async fn healthy_router_passes_requests_through() {
    let app = apply_resilience(
        Router::new().route("/", get(|| async { "ok" })),
        &ResilienceConfig::default(),
    );

    for _ in 0..5 {
        assert_eq!(status_of(&app, "/").await, StatusCode::OK);
    }
}

#[tokio::test]
async fn breaker_opens_after_sustained_5xx() {
    let app = apply_resilience(
        Router::new().route(
            "/boom",
            get(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
        ),
        &trip_fast_config(),
    );

    // Fill the window with failures. These are the handler's own 500s.
    for _ in 0..2 {
        assert_eq!(
            status_of(&app, "/boom").await,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    // Once open, the breaker sheds load itself: 503, not the handler's 500.
    assert_eq!(
        status_of(&app, "/boom").await,
        StatusCode::SERVICE_UNAVAILABLE,
        "circuit breaker did not open after sustained 5xx -- the failure \
         classifier is likely counting only `Err`, which an infallible \
         inbound route never produces"
    );
}

#[tokio::test]
async fn breaker_ignores_4xx_client_errors() {
    let app = apply_resilience(
        Router::new().route("/bad", get(|| async { StatusCode::BAD_REQUEST })),
        &trip_fast_config(),
    );

    // A client's malformed requests are not the service failing; the circuit
    // must stay closed no matter how many arrive.
    for _ in 0..6 {
        assert_eq!(status_of(&app, "/bad").await, StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn disabling_both_patterns_leaves_router_untouched() {
    let config = ResilienceConfig::new()
        .with_circuit_breaker(false)
        .with_bulkhead(false);
    let app = apply_resilience(
        Router::new().route("/boom", get(|| async { StatusCode::INTERNAL_SERVER_ERROR })),
        &config,
    );

    // With the breaker disabled, 5xx never turns into 503 no matter how many.
    for _ in 0..5 {
        assert_eq!(
            status_of(&app, "/boom").await,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}

#[tokio::test]
async fn bulkhead_alone_attaches_to_router() {
    let config = ResilienceConfig::new()
        .with_circuit_breaker(false)
        .with_bulkhead(true)
        .with_bulkhead_max_concurrent(4);
    let app = apply_resilience(Router::new().route("/", get(|| async { "ok" })), &config);

    assert_eq!(status_of(&app, "/").await, StatusCode::OK);
}

/// Issue #32: `[middleware.resilience]` was parsed but never reached a layer.
/// This asserts the TOML-facing config bridges to a working breaker, so the
/// wiring cannot silently regress to a no-op again.
#[tokio::test]
async fn toml_config_bridges_to_a_live_breaker() {
    let toml_config = acton_service::config::ResilienceConfig {
        circuit_breaker_enabled: true,
        circuit_breaker_threshold: 0.5,
        circuit_breaker_min_requests: 2,
        circuit_breaker_wait_secs: 60,
        bulkhead_enabled: false,
        bulkhead_max_concurrent: 100,
        bulkhead_max_wait_ms: 5_000,
    };

    let middleware_config = ResilienceConfig::from(&toml_config);
    assert_eq!(middleware_config.circuit_breaker_min_requests, 2);
    assert_eq!(
        middleware_config.circuit_breaker_wait_duration,
        Duration::from_secs(60)
    );

    let app = apply_resilience(
        Router::new().route("/boom", get(|| async { StatusCode::INTERNAL_SERVER_ERROR })),
        &middleware_config,
    );

    for _ in 0..2 {
        assert_eq!(
            status_of(&app, "/boom").await,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
    assert_eq!(
        status_of(&app, "/boom").await,
        StatusCode::SERVICE_UNAVAILABLE,
        "config from [middleware.resilience] did not reach a live circuit breaker"
    );
}
