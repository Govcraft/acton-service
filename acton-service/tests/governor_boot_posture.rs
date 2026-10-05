//! The actual builder reports the effective governor posture once when enabled.

#![cfg(feature = "governor")]

use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex},
};

use acton_service::{
    config::{Config, RateLimitConfig, RouteRateLimitConfig},
    middleware::{RateKey, RateRequest},
    prelude::ServiceBuilder,
};
use serde_json::Value;
use tracing::{
    field::{Field, Visit},
    Level,
};
use tracing_subscriber::{
    layer::{Context, SubscriberExt},
    Layer,
};

#[derive(Default)]
struct Fields(BTreeMap<String, Value>);

impl Visit for Fields {
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name().to_string(), value.into());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.0.insert(field.name().to_string(), value.into());
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().to_string(), value.into());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let text = format!("{value:?}");
        let value = serde_json::from_str(&text).unwrap_or(Value::String(text));
        self.0.insert(field.name().to_string(), value);
    }
}

type Posture = (Level, BTreeMap<String, Value>);
struct CapturePosture(Arc<Mutex<Vec<Posture>>>);

impl<S: tracing::Subscriber> Layer<S> for CapturePosture {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        if fields.0.contains_key("classifier") && fields.0.contains_key("per_user_rpm") {
            self.0
                .lock()
                .expect("capture lock")
                .push((*event.metadata().level(), fields.0));
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_builder_emits_effective_posture_only_when_auto_apply_is_enabled() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    tracing::subscriber::set_global_default(
        tracing_subscriber::registry().with(CapturePosture(Arc::clone(&captured))),
    )
    .expect("application subscriber is installed before building");

    let mut config = Config::<()> {
        rate_limit: RateLimitConfig {
            auto_apply: false,
            per_user_rpm: 80,
            per_client_rpm: 30,
            anonymous_rpm: Some(17),
            anonymous_burst: Some(4),
            trust_forwarded_headers: true,
            exempt_paths: vec!["/health".into(), "/ready".into(), "/internal/probe".into()],
            ..Default::default()
        },
        ..Default::default()
    };
    config.rate_limit.routes.insert(
        "POST /api/v1/uploads".into(),
        RouteRateLimitConfig {
            requests_per_minute: 9,
            burst_size: 2,
            per_user: true,
        },
    );
    let disabled = ServiceBuilder::new().with_config(config.clone()).build();
    assert!(
        captured.lock().expect("capture lock").is_empty(),
        "disabled auto-apply must not advertise an active governor"
    );
    drop(disabled);

    config.rate_limit.auto_apply = true;
    let enabled = ServiceBuilder::new().with_config(config.clone()).build();
    let events = captured.lock().expect("capture lock").clone();
    assert_eq!(
        events.len(),
        1,
        "enabled builder emits exactly one posture event"
    );
    let (level, fields) = &events[0];
    assert_eq!(*level, Level::INFO);
    for (name, expected) in [
        ("per_user_rpm", 80),
        ("per_user_burst", 8),
        ("per_client_rpm", 30),
        ("per_client_burst", 3),
        ("anonymous_rpm", 17),
        ("anonymous_burst", 4),
        ("route_overrides", 1),
    ] {
        assert_eq!(fields.get(name), Some(&Value::from(expected)), "{name}");
    }
    assert_eq!(fields.get("classifier"), Some(&Value::from("claims-or-ip")));
    assert_eq!(
        fields.get("trust_forwarded_headers"),
        Some(&Value::from(true))
    );
    assert_eq!(
        fields.get("exempt_paths"),
        Some(&serde_json::json!(["/health", "/ready", "/internal/probe"]))
    );
    drop(enabled);

    let custom = ServiceBuilder::new()
        .with_config(config)
        .with_rate_classifier(|_: &RateRequest<'_>| RateKey::Exempt)
        .build();
    let events = captured.lock().expect("capture lock").clone();
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].1.get("classifier"), Some(&Value::from("custom")));
    drop(custom);
}
