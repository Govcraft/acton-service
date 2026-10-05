//! Racing services still use one provider and one first-successful resource.

#![cfg(feature = "prometheus-metrics")]

use acton_service::{config::Config, observability};
use std::sync::{Arc, Barrier};

#[test]
fn concurrent_initializers_share_the_exported_registry() {
    let barrier = Arc::new(Barrier::new(8));
    std::thread::scope(|scope| {
        for index in 0..8 {
            let barrier = Arc::clone(&barrier);
            scope.spawn(move || {
                let mut config = Config::<()>::default();
                config.service.name = format!("metrics-racer-{index}");
                barrier.wait();
                observability::init_meter_provider(&config)
                    .expect("racing initialization succeeds");
                opentelemetry::global::meter("racing")
                    .u64_counter("racing_initializations")
                    .build()
                    .add(1, &[]);
            });
        }
    });
    let families = observability::PROMETHEUS_REGISTRY
        .get()
        .expect("one registry installed")
        .gather();
    let counter = families
        .iter()
        .find(|family| family.name() == "racing_initializations_total")
        .expect("every global meter uses the served registry");
    let total: f64 = counter
        .get_metric()
        .iter()
        .map(|metric| metric.get_counter().value())
        .sum();
    assert_eq!(total, 8.0);
    let target = families
        .iter()
        .find(|family| family.name() == "target_info")
        .expect("one resource is exported");
    assert_eq!(target.get_metric().len(), 1);
    assert!(
        target.get_metric()[0]
            .get_label()
            .iter()
            .any(|label| label.name() == "service_name"
                && label.value().starts_with("metrics-racer-"))
    );
}
