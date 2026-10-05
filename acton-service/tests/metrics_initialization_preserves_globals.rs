//! Sequential initialization must leave local, global, and scrape providers aligned.

#![cfg(feature = "prometheus-metrics")]

use acton_service::{config::Config, observability};

#[test]
fn first_metrics_identity_and_both_meter_apis_survive_a_second_initialization() {
    let mut config = Config::<()>::default();
    config.service.name = "first-metrics-owner".to_string();
    observability::init_meter_provider(&config).expect("first initialization succeeds");
    let registry = observability::PROMETHEUS_REGISTRY
        .get()
        .expect("registry installed");
    let original = registry as *const _;
    let first = opentelemetry::global::meter("first")
        .u64_counter("first_initialization")
        .build();
    first.add(2, &[]);

    config.service.name = "discarded-metrics-owner".to_string();
    observability::init_meter_provider(&config).expect("second initialization is a no-op");
    assert_eq!(
        original,
        observability::PROMETHEUS_REGISTRY
            .get()
            .expect("same registry") as *const _
    );
    let global = opentelemetry::global::meter("after_second")
        .u64_counter("global_after_second")
        .build();
    global.add(3, &[]);
    observability::get_meter()
        .expect("local provider remains available")
        .u64_counter("local_after_second")
        .build()
        .add(5, &[]);
    first.add(7, &[]);

    let families = registry.gather();
    for (name, expected) in [
        ("first_initialization_total", 9.0),
        ("global_after_second_total", 3.0),
        ("local_after_second_total", 5.0),
    ] {
        let family = families
            .iter()
            .find(|family| family.name() == name)
            .unwrap_or_else(|| panic!("{name} must be visible in the served registry"));
        let value: f64 = family
            .get_metric()
            .iter()
            .map(|metric| metric.get_counter().value())
            .sum();
        assert_eq!(value, expected, "{name} must retain every observation");
    }
    let target = families
        .iter()
        .find(|family| family.name() == "target_info")
        .expect("service identity is exported");
    let names: Vec<_> = target
        .get_metric()
        .iter()
        .flat_map(|metric| metric.get_label())
        .filter(|label| label.name() == "service_name")
        .map(|label| label.value())
        .collect();
    assert_eq!(names, ["first-metrics-owner"]);
}
