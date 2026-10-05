//! An application can own tracing before it constructs an acton service.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use acton_service::{config::Config, observability, prelude::ServiceBuilder};
use tracing_subscriber::{layer::SubscriberExt, Layer};

struct CaptureEvents(Arc<AtomicUsize>);

impl<S: tracing::Subscriber> Layer<S> for CaptureEvents {
    fn on_event(&self, _: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn application_subscriber_survives_tracing_and_service_initialization() {
    let events = Arc::new(AtomicUsize::new(0));
    tracing::subscriber::set_global_default(
        tracing_subscriber::registry().with(CaptureEvents(Arc::clone(&events))),
    )
    .expect("application installs its own subscriber first");

    let mut config = Config::<()>::default();
    config.service.name = "external-tracing-owner".to_string();
    #[cfg(feature = "observability")]
    {
        opentelemetry::global::set_text_map_propagator(
            opentelemetry_sdk::propagation::BaggagePropagator::new(),
        );
        config.otlp = Some(acton_service::config::OtlpConfig {
            enabled: true,
            endpoint: "http://127.0.0.1:4317".to_string(),
            service_name: None,
        });
    }

    observability::init_tracing(&config).expect("external tracing is supported");
    observability::init_basic_tracing();
    let service = ServiceBuilder::new().with_config(config).build();
    let before = events.load(Ordering::SeqCst);
    tracing::error!("application capture remains active after service build");
    assert_eq!(events.load(Ordering::SeqCst), before + 1);

    #[cfg(feature = "observability")]
    {
        use opentelemetry::trace::{Span, Tracer};
        let fields = opentelemetry::global::get_text_map_propagator(|propagator| {
            propagator.fields().map(str::to_owned).collect::<Vec<_>>()
        });
        assert_eq!(fields, ["baggage"], "application propagator is preserved");
        let span = opentelemetry::global::tracer("application").start("probe");
        assert!(
            !span.is_recording(),
            "acton must not install its rejected OTLP provider"
        );
    }
    drop(service);
}
