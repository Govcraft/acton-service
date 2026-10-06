//! Audit tracing export is opt-in, while persistence remains enabled.

#![cfg(all(feature = "audit", feature = "observability"))]

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use acton_reactive::prelude::ActonApp;
use acton_service::{
    audit::{AuditAgent, AuditConfig, AuditEvent, AuditEventKind, AuditSeverity, AuditStorage},
    error::Error,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tracing_subscriber::{layer::SubscriberExt, Layer};

struct CaptureAuditRecords(Arc<AtomicUsize>);

impl<S: tracing::Subscriber> Layer<S> for CaptureAuditRecords {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        if event.metadata().fields().field("audit.event.id").is_some() {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}

struct CapturingStorage(tokio::sync::watch::Sender<Option<AuditEvent>>);

#[async_trait]
impl AuditStorage for CapturingStorage {
    async fn append(&self, event: &AuditEvent) -> Result<(), Error> {
        self.0.send_replace(Some(event.clone()));
        Ok(())
    }

    async fn latest(&self) -> Result<Option<AuditEvent>, Error> {
        Ok(None)
    }

    async fn query_range(
        &self,
        _from: DateTime<Utc>,
        _to: DateTime<Utc>,
        _limit: usize,
    ) -> Result<Vec<AuditEvent>, Error> {
        Ok(Vec::new())
    }

    async fn verify_chain(&self, _from_sequence: u64) -> Result<Option<u64>, Error> {
        Ok(None)
    }
}

// A current-thread runtime makes the storage notification a deterministic
// barrier: append returns immediately, and with syslog disabled the writer
// emits its tracing record before yielding. Only then can this test resume.
#[tokio::test]
async fn otlp_flag_controls_audit_tracing_without_disabling_persistence() {
    let records = Arc::new(AtomicUsize::new(0));
    tracing::subscriber::set_global_default(
        tracing_subscriber::registry().with(CaptureAuditRecords(Arc::clone(&records))),
    )
    .expect("install capture subscriber in isolated integration test executable");

    let mut runtime = ActonApp::launch_async().await;
    for (enabled, expected_records) in [(false, 0), (true, 1)] {
        let storage = Arc::new(CapturingStorage(tokio::sync::watch::Sender::new(None)));
        let mut persisted = storage.0.subscribe();
        let mut config = AuditConfig::default();
        assert!(
            !config.otlp_logs_enabled,
            "export remains disabled by default"
        );
        config.otlp_logs_enabled = enabled;
        config.syslog.transport = "none".to_string();
        let handle = AuditAgent::spawn(
            &mut runtime,
            config,
            Some(storage),
            "audit-export-regression".to_string(),
        )
        .await
        .expect("spawn real audit agent");

        let event = AuditEvent::new(
            AuditEventKind::Custom("export.flag".to_string()),
            AuditSeverity::Informational,
            "audit-export-regression".to_string(),
        );
        let expected_id = event.id.clone();
        handle.send(event).await;
        tokio::time::timeout(Duration::from_secs(5), persisted.wait_for(Option::is_some))
            .await
            .expect("audit event reaches storage")
            .expect("storage notification remains open");

        let persisted_event = persisted.borrow().clone().expect("event was persisted");
        assert_eq!(persisted_event.id, expected_id);
        assert_eq!(persisted_event.sequence, 1);
        assert!(persisted_event.hash.is_some(), "persisted event is sealed");
        assert_eq!(
            records.load(Ordering::SeqCst),
            expected_records,
            "audit.event tracing record count with otlp_logs_enabled={enabled}"
        );
    }
    runtime
        .shutdown_all()
        .await
        .expect("shut down audit actors");
}
