//! A call with no configured readers must leave initialization retryable.

#![cfg(all(feature = "otel-metrics", not(feature = "prometheus-metrics")))]

use acton_service::{
    config::{Config, OtlpConfig},
    observability,
};

#[tokio::test]
async fn disabled_metrics_can_be_initialized_by_a_later_service() {
    let mut config = Config::<()>::default();
    observability::init_meter_provider(&config).expect("unconfigured metrics are harmless");
    assert!(observability::METER_PROVIDER.get().is_none());
    config.otlp = Some(OtlpConfig {
        enabled: true,
        endpoint: "http://127.0.0.1:4317".to_string(),
        service_name: None,
    });
    observability::init_meter_provider(&config).expect("later configured initialization succeeds");
    assert!(observability::METER_PROVIDER.get().is_some());
    observability::shutdown_tracing();
    observability::init_meter_provider(&config).expect("shutdown does not replace the provider");
}
