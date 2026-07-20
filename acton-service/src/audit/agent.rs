//! Audit agent (acton-reactive actor)
//!
//! The `AuditAgent` owns the BLAKE3 hash chain state and processes events
//! sequentially, guaranteeing correct chain ordering. It persists events to
//! the configured storage backend and dispatches to SIEM exporters.

use acton_reactive::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;

use super::alert::AuditAlertHook;
use super::alert_webhook::WebhookAlertHook;
use super::chain::AuditChain;
use super::config::AuditConfig;
use super::event::AuditEvent;
use super::failure_tracker::FailureTracker;
use super::storage::AuditStorage;
use super::syslog::SyslogSender;

/// Maximum number of events buffered while the hash chain initializes
///
/// Events emitted between `AuditAgent::spawn()` and the arrival of
/// `ChainLoaded` are held here so they can be sealed in order once the chain
/// exists. The cap bounds memory when storage never becomes ready; overflow is
/// reported through the failure tracker rather than dropped silently.
const MAX_PENDING_EVENTS: usize = 1024;

/// State held by the audit agent actor
#[derive(Default)]
pub struct AuditAgentState {
    /// BLAKE3 hash chain state
    pub chain: Option<AuditChain>,
    /// Persistent storage backend (if a DB feature is active)
    pub storage: Option<Arc<dyn AuditStorage>>,
    /// Syslog sender (if configured)
    pub syslog: Option<SyslogSender>,
    /// Audit configuration
    pub config: Option<AuditConfig>,
    /// Storage failure tracker (if alerts are configured)
    pub(crate) failure_tracker: Option<Arc<FailureTracker>>,
    /// Events received before the chain was initialized, in arrival order
    pub(crate) pending: Vec<AuditEvent>,
    /// Channel to the sequential writer task that persists sealed events
    pub(crate) writer: Option<tokio::sync::mpsc::UnboundedSender<AuditEvent>>,
}

// Manual Debug impl since AuditChain and dyn AuditStorage don't impl Debug
impl std::fmt::Debug for AuditAgentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditAgentState")
            .field("chain", &self.chain.is_some())
            .field("storage", &self.storage.is_some())
            .field("syslog", &self.syslog.is_some())
            .field("config", &self.config.is_some())
            .field("failure_tracker", &self.failure_tracker.is_some())
            .field("pending", &self.pending.len())
            .finish()
    }
}

/// Internal message: chain state loaded from storage
///
/// Sent by the spawned task in `after_start` back to the agent.
/// Must be Clone + Debug for ActonMessage.
#[derive(Clone, Debug)]
struct ChainLoaded {
    previous_hash: Option<String>,
    sequence: u64,
    service_name: String,
}

/// Internal message: triggers periodic retention cleanup
#[derive(Clone, Debug)]
struct CleanupTrigger;

/// Batch size for archival + purge operations
const CLEANUP_BATCH_SIZE: usize = 10_000;

/// Audit agent that manages the immutable audit trail
///
/// Follows the same actor pattern as `DatabasePoolAgent`, `RedisPoolAgent`, etc.
/// Spawned during `ServiceBuilder::build()` alongside other pool agents.
pub struct AuditAgent;

impl AuditAgent {
    /// Spawn the audit agent
    ///
    /// The agent loads chain state from storage (if available) in `after_start`,
    /// then processes `AuditEvent` messages sequentially.
    pub async fn spawn(
        runtime: &mut ActorRuntime,
        config: AuditConfig,
        storage: Option<Arc<dyn AuditStorage>>,
        service_name: String,
    ) -> anyhow::Result<ActorHandle> {
        let mut agent = runtime.new_actor::<AuditAgentState>();

        // Set up syslog sender if configured
        let syslog = if config.syslog.transport != "none" {
            match SyslogSender::new(&config.syslog) {
                Ok(sender) => Some(sender),
                Err(e) => {
                    tracing::warn!("Failed to initialize syslog sender: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // Save retention config before moving config into agent model
        let retention_days = config.retention_days;
        let cleanup_interval_hours = config.cleanup_interval_hours;

        // Set up failure tracker if alert hooks are configured
        let failure_tracker = if let Some(ref alert_config) = config.alerts {
            if alert_config.enabled {
                let mut hooks: Vec<Arc<dyn AuditAlertHook>> = Vec::new();
                for wh in &alert_config.webhooks {
                    hooks.push(Arc::new(WebhookAlertHook::new(
                        wh.url.clone(),
                        std::time::Duration::from_secs(wh.timeout_secs),
                        wh.headers.clone(),
                    )));
                }
                Some(Arc::new(FailureTracker::new(
                    hooks,
                    alert_config.threshold_secs,
                    alert_config.cooldown_secs,
                    alert_config.notify_recovery,
                    service_name.clone(),
                )))
            } else {
                None
            }
        } else {
            None
        };

        agent.model.config = Some(config);
        agent.model.storage = storage;
        agent.model.syslog = syslog;
        agent.model.failure_tracker = failure_tracker;
        agent.model.writer = Some(spawn_writer_task(
            agent.model.storage.clone(),
            agent.model.syslog.clone(),
            agent.model.failure_tracker.clone(),
        ));

        // Clone values needed for after_start closure
        let storage_for_start = agent.model.storage.clone();
        let service_name_for_start = service_name.clone();

        // Handle chain initialization (sent from after_start task)
        agent.mutate_on::<ChainLoaded>(|agent, envelope| {
            let msg = envelope.message().clone();
            let chain = if let Some(ref hash) = msg.previous_hash {
                AuditChain::resume(msg.service_name, hash.clone(), msg.sequence)
            } else {
                AuditChain::new(msg.service_name)
            };
            agent.model.chain = Some(chain);
            tracing::info!("Audit chain initialized at sequence {}", msg.sequence);

            // Drain events that arrived before the chain existed, in arrival
            // order, so they are sealed ahead of anything received later.
            let pending = std::mem::take(&mut agent.model.pending);
            if !pending.is_empty() {
                tracing::info!(
                    "Sealing {} audit event(s) buffered during chain initialization",
                    pending.len()
                );
            }
            for event in pending {
                seal_and_dispatch(&mut agent.model, event);
            }

            Reply::ready()
        });

        // Handle incoming audit events
        agent.mutate_on::<AuditEvent>(|agent, envelope| {
            let event = envelope.message().clone();

            if agent.model.chain.is_none() {
                buffer_pending_event(&mut agent.model, event);
            } else {
                seal_and_dispatch(&mut agent.model, event);
            }

            Reply::ready()
        });

        // Handle retention cleanup triggers
        agent.mutate_on::<CleanupTrigger>(|agent, _envelope| {
            let config = agent.model.config.clone();
            let storage = agent.model.storage.clone();

            tokio::spawn(async move {
                if let (Some(config), Some(storage)) = (config, storage) {
                    if let Err(e) = run_cleanup(&config, storage.as_ref()).await {
                        tracing::error!("Audit retention cleanup failed: {}", e);
                    }
                }
            });

            Reply::ready()
        });

        // Load chain state from storage on startup.
        //
        // Every branch below ends in a `ChainLoaded` send — including storage
        // errors and readiness timeouts, which fall back to a fresh chain — so
        // events buffered in the meantime are always drained rather than
        // stranded.
        agent.after_start(move |agent| {
            let storage = storage_for_start.clone();
            let service_name = service_name_for_start.clone();
            let self_handle = agent.handle().clone();

            tokio::spawn(async move {
                let (previous_hash, sequence) = if let Some(ref store) = storage {
                    // Pool agents connect asynchronously, so lazily-resolved storage
                    // is typically not ready yet. Starting the chain at sequence 0
                    // while the backend already holds events would fork the chain and
                    // collide with the stored sequence, so wait for the pool.
                    wait_for_storage(store.as_ref()).await;

                    match store.latest().await {
                        Ok(Some(event)) => {
                            tracing::info!(
                                "Resuming audit chain at sequence {} for {}",
                                event.sequence,
                                service_name
                            );
                            (event.hash, event.sequence)
                        }
                        Ok(None) => {
                            tracing::info!("Starting new audit chain for {}", service_name);
                            (None, 0)
                        }
                        Err(e) => {
                            tracing::error!(
                                "Failed to load audit chain state: {}. Starting fresh.",
                                e
                            );
                            (None, 0)
                        }
                    }
                } else {
                    tracing::info!(
                        "No audit storage configured, starting in-memory chain for {}",
                        service_name
                    );
                    (None, 0)
                };

                self_handle
                    .send(ChainLoaded {
                        previous_hash,
                        sequence,
                        service_name,
                    })
                    .await;
            });

            Reply::ready()
        });

        let handle = agent.start().await;

        // Start periodic cleanup if retention is configured
        if retention_days.is_some() {
            let cleanup_handle = handle.clone();
            let interval_hours = cleanup_interval_hours;
            tokio::spawn(async move {
                let period = std::time::Duration::from_secs(interval_hours as u64 * 3600);
                let mut interval = tokio::time::interval(period);
                // Skip the first immediate tick
                interval.tick().await;
                loop {
                    interval.tick().await;
                    tracing::debug!("Triggering audit retention cleanup");
                    cleanup_handle.send(CleanupTrigger).await;
                }
            });
        }

        Ok(handle)
    }
}

/// Buffer an event received before the hash chain was initialized
///
/// Overflow policy: once [`MAX_PENDING_EVENTS`] events are buffered the
/// *incoming* (newest) event is dropped, preserving the oldest events and
/// therefore the earliest part of the chain. The drop is reported through the
/// failure tracker, so it participates in the same threshold/cooldown alerting
/// used for storage append failures instead of vanishing silently.
fn buffer_pending_event(state: &mut AuditAgentState, event: AuditEvent) {
    if state.pending.len() >= MAX_PENDING_EVENTS {
        let reason = format!(
            "audit chain not initialized and pending buffer is full ({MAX_PENDING_EVENTS} events); \
             dropped event {:?}",
            event.kind
        );
        tracing::error!(
            audit.severity = "ALERT",
            pending = MAX_PENDING_EVENTS,
            "{}",
            reason
        );
        if let Some(ref tracker) = state.failure_tracker {
            tracker.record_failure(&reason);
        }
        return;
    }

    state.pending.push(event);
}

/// Seal an event with the hash chain and hand it to the writer task
///
/// The writer task persists sequentially, so chain order is also write order.
fn seal_and_dispatch(state: &mut AuditAgentState, event: AuditEvent) {
    let Some(chain) = state.chain.as_mut() else {
        // Unreachable in practice: callers check the chain first.
        buffer_pending_event(state, event);
        return;
    };

    let sealed = chain.seal(event);

    let Some(ref writer) = state.writer else {
        tracing::error!("Audit writer task missing, event not persisted");
        return;
    };

    if writer.send(sealed).is_err() {
        let reason = "audit writer task terminated; event not persisted".to_string();
        tracing::error!(audit.severity = "ALERT", "{}", reason);
        if let Some(ref tracker) = state.failure_tracker {
            tracker.record_failure(&reason);
        }
    }
}

/// Spawn the task that persists and exports sealed events in order
///
/// Persistence runs on a single task fed by an unbounded channel, so events are
/// written in the exact order the chain sealed them. Events buffered during
/// chain initialization are enqueued first and therefore land first.
fn spawn_writer_task(
    storage: Option<Arc<dyn AuditStorage>>,
    syslog: Option<SyslogSender>,
    tracker: Option<Arc<FailureTracker>>,
) -> tokio::sync::mpsc::UnboundedSender<AuditEvent> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AuditEvent>();

    tokio::spawn(async move {
        while let Some(sealed_event) = rx.recv().await {
            // Persist to storage
            if let Some(ref store) = storage {
                match store.append(&sealed_event).await {
                    Ok(()) => {
                        if let Some(ref t) = tracker {
                            t.record_success();
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to persist audit event: {}", e);
                        if let Some(ref t) = tracker {
                            t.record_failure(&e.to_string());
                        }
                    }
                }
            }

            // Send to syslog
            if let Some(ref sender) = syslog {
                if let Err(e) = sender.send(&sealed_event).await {
                    tracing::warn!("Failed to send audit event to syslog: {}", e);
                }
            }

            // OTLP export (when observability feature is active)
            #[cfg(feature = "observability")]
            {
                super::otlp::emit_audit_log(&sealed_event);
            }
        }
    });

    tx
}

/// Longest the agent waits for a connection pool before giving up on chain resumption
const STORAGE_READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Interval between storage readiness polls
const STORAGE_READY_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

/// Poll storage until it is usable, bounded by [`STORAGE_READY_TIMEOUT`].
///
/// Backends constructed from an already-connected client return immediately.
/// Lazily-resolved backends become ready once their pool agent finishes
/// connecting. On timeout this logs an error and returns; the caller then starts
/// a fresh chain, which is the pre-existing behaviour for unreachable storage.
async fn wait_for_storage(storage: &dyn AuditStorage) {
    let deadline = tokio::time::Instant::now() + STORAGE_READY_TIMEOUT;

    loop {
        let last_error = match storage.ensure_ready().await {
            Ok(()) => return,
            Err(e) => e,
        };

        if tokio::time::Instant::now() >= deadline {
            tracing::error!(
                "Audit storage not ready after {:?}: {}. Starting a fresh in-memory chain; \
                 persisted events will not be resumed and sequence numbers may collide.",
                STORAGE_READY_TIMEOUT,
                last_error
            );
            return;
        }

        tokio::time::sleep(STORAGE_READY_POLL_INTERVAL).await;
    }
}

/// Run a single retention cleanup cycle
async fn run_cleanup(
    config: &AuditConfig,
    storage: &dyn AuditStorage,
) -> Result<(), crate::error::Error> {
    let retention_days = match config.retention_days {
        Some(days) => days,
        None => return Ok(()),
    };

    let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days as i64);
    let archive_dir = config.archive_path.as_ref().map(PathBuf::from);
    let mut total_purged: u64 = 0;

    loop {
        let events = storage.query_before(cutoff, CLEANUP_BATCH_SIZE).await?;
        if events.is_empty() {
            break;
        }

        let batch_count = events.len();

        // Archive if configured — abort this cycle if archival fails
        if let Some(ref dir) = archive_dir {
            super::archive::archive_events(&events, dir).await?;
        }

        // Purge the batch
        let purged = storage.purge_before(cutoff).await?;
        total_purged += purged;

        tracing::info!(
            "Audit cleanup: purged {} events (batch had {})",
            purged,
            batch_count
        );

        // If we got fewer than the batch size, we're done
        if batch_count < CLEANUP_BATCH_SIZE {
            break;
        }
    }

    if total_purged > 0 {
        tracing::info!(
            "Audit retention cleanup complete: purged {} total events older than {} days",
            total_purged,
            retention_days
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use async_trait::async_trait;

    use super::*;
    use crate::audit::alert::{AuditAlertEvent, AuditAlertHook};
    use crate::audit::config::SyslogConfig;
    use crate::audit::event::{AuditEventKind, AuditSeverity};
    use crate::error::Error;

    /// Storage that records appended events, and reports readiness only after a
    /// delay so events can be emitted while the chain is still initializing.
    struct SlowCapturingStorage {
        events: Mutex<Vec<AuditEvent>>,
        ready_after: tokio::time::Instant,
    }

    impl SlowCapturingStorage {
        fn new(delay: Duration) -> Self {
            Self {
                events: Mutex::new(Vec::new()),
                ready_after: tokio::time::Instant::now() + delay,
            }
        }

        fn kinds(&self) -> Vec<AuditEventKind> {
            self.events
                .lock()
                .expect("events lock")
                .iter()
                .map(|e| e.kind.clone())
                .collect()
        }
    }

    #[async_trait]
    impl AuditStorage for SlowCapturingStorage {
        async fn append(&self, event: &AuditEvent) -> Result<(), Error> {
            self.events.lock().expect("events lock").push(event.clone());
            Ok(())
        }

        async fn latest(&self) -> Result<Option<AuditEvent>, Error> {
            Ok(None)
        }

        async fn query_range(
            &self,
            _from: chrono::DateTime<chrono::Utc>,
            _to: chrono::DateTime<chrono::Utc>,
            _limit: usize,
        ) -> Result<Vec<AuditEvent>, Error> {
            Ok(Vec::new())
        }

        async fn verify_chain(&self, _from_sequence: u64) -> Result<Option<u64>, Error> {
            Ok(None)
        }

        async fn ensure_ready(&self) -> Result<(), Error> {
            if tokio::time::Instant::now() >= self.ready_after {
                Ok(())
            } else {
                Err(Error::Internal("pool still connecting".to_string()))
            }
        }
    }

    fn test_config() -> AuditConfig {
        AuditConfig {
            syslog: SyslogConfig {
                transport: "none".to_string(),
                ..SyslogConfig::default()
            },
            ..AuditConfig::default()
        }
    }

    fn custom_event(name: &str) -> AuditEvent {
        AuditEvent::new(
            AuditEventKind::Custom(name.to_string()),
            AuditSeverity::Informational,
            "test-svc".to_string(),
        )
    }

    /// Events emitted before chain init are persisted, in emission order.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn events_before_chain_init_are_buffered_and_ordered() {
        let mut runtime = ActonApp::launch_async().await;
        let storage = Arc::new(SlowCapturingStorage::new(Duration::from_millis(400)));

        let handle = AuditAgent::spawn(
            &mut runtime,
            test_config(),
            Some(storage.clone() as Arc<dyn AuditStorage>),
            "test-svc".to_string(),
        )
        .await
        .expect("spawn audit agent");

        // Emit immediately — the chain cannot possibly be initialized yet.
        let expected: Vec<String> = (0..10).map(|i| format!("early.{i}")).collect();
        for name in &expected {
            handle.send(custom_event(name)).await;
        }

        // Wait for the drain to complete.
        for _ in 0..100 {
            if storage.events.lock().expect("events lock").len() >= expected.len() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        let kinds = storage.kinds();
        let names: Vec<String> = kinds
            .iter()
            .map(|k| match k {
                AuditEventKind::Custom(n) => n.clone(),
                other => panic!("unexpected kind {other:?}"),
            })
            .collect();
        assert_eq!(names, expected, "all early events persisted in order");

        // Sequence numbers must be contiguous from the chain start.
        let sequences: Vec<u64> = storage
            .events
            .lock()
            .expect("events lock")
            .iter()
            .map(|e| e.sequence)
            .collect();
        assert_eq!(sequences, (1..=expected.len() as u64).collect::<Vec<_>>());
    }

    /// Events emitted after chain init are sealed after buffered ones.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn buffered_events_precede_later_events() {
        let mut runtime = ActonApp::launch_async().await;
        let storage = Arc::new(SlowCapturingStorage::new(Duration::from_millis(300)));

        let handle = AuditAgent::spawn(
            &mut runtime,
            test_config(),
            Some(storage.clone() as Arc<dyn AuditStorage>),
            "test-svc".to_string(),
        )
        .await
        .expect("spawn audit agent");

        handle.send(custom_event("early")).await;

        // Well after chain initialization has completed.
        tokio::time::sleep(Duration::from_millis(900)).await;
        handle.send(custom_event("late")).await;

        for _ in 0..100 {
            if storage.events.lock().expect("events lock").len() >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        let names: Vec<String> = storage
            .kinds()
            .iter()
            .map(|k| match k {
                AuditEventKind::Custom(n) => n.clone(),
                other => panic!("unexpected kind {other:?}"),
            })
            .collect();
        assert_eq!(names, vec!["early".to_string(), "late".to_string()]);
    }

    /// Alert hook that counts storage-unreachable alerts.
    struct CountingHook {
        alerts: AtomicU64,
    }

    #[async_trait]
    impl AuditAlertHook for CountingHook {
        async fn on_alert(&self, event: AuditAlertEvent) {
            if matches!(event, AuditAlertEvent::StorageUnreachable { .. }) {
                self.alerts.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    /// Overflow drops the newest event and reports it to the failure tracker.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pending_overflow_reports_to_failure_tracker() {
        let hook = Arc::new(CountingHook {
            alerts: AtomicU64::new(0),
        });
        let tracker = Arc::new(FailureTracker::new(
            vec![hook.clone()],
            0, // alert immediately
            3600,
            true,
            "test-svc".to_string(),
        ));

        let mut state = AuditAgentState {
            failure_tracker: Some(tracker),
            ..AuditAgentState::default()
        };

        for i in 0..MAX_PENDING_EVENTS {
            buffer_pending_event(&mut state, custom_event(&format!("e{i}")));
        }
        assert_eq!(state.pending.len(), MAX_PENDING_EVENTS);

        buffer_pending_event(&mut state, custom_event("overflow"));

        // Newest is dropped; the oldest buffered events are preserved.
        assert_eq!(state.pending.len(), MAX_PENDING_EVENTS);
        assert_eq!(
            state.pending[0].kind,
            AuditEventKind::Custom("e0".to_string())
        );

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(hook.alerts.load(Ordering::SeqCst), 1);
    }
}
