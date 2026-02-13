//! Audit storage failure tracking
//!
//! Tracks consecutive storage failures and dispatches alert hooks when
//! configurable thresholds are exceeded. Recovery events are dispatched
//! when storage comes back online after an alert.

use chrono::{DateTime, Utc};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::alert::{AuditAlertEvent, AuditAlertHook};

/// Internal state protected by a `std::sync::Mutex`
///
/// The mutex is never held across `.await` points — all async dispatch
/// happens after the lock is dropped.
struct TrackerState {
    /// Monotonic timestamp of the first failure in the current outage
    first_failure_at: Option<Instant>,
    /// Wall-clock time of the first failure (for alert payloads)
    first_failure_wall: Option<DateTime<Utc>>,
    /// Number of consecutive storage failures
    consecutive_failures: u64,
    /// Error message from the most recent failure
    last_error: String,
    /// When the last alert was dispatched (monotonic, for cooldown)
    last_alert_at: Option<Instant>,
    /// Whether an alert is currently active (used for recovery detection)
    alert_active: bool,
    /// Number of events affected during the current outage
    events_affected_during_outage: u64,
}

/// Tracks audit storage failures and dispatches alert hooks
pub(crate) struct FailureTracker {
    state: Mutex<TrackerState>,
    hooks: Vec<Arc<dyn AuditAlertHook>>,
    threshold: Duration,
    cooldown: Duration,
    notify_recovery: bool,
    service_name: String,
}

impl FailureTracker {
    /// Create a new failure tracker
    ///
    /// # Arguments
    ///
    /// * `hooks` — Alert hooks to dispatch to
    /// * `threshold_secs` — Seconds of continuous failure before alerting
    /// * `cooldown_secs` — Minimum seconds between repeated alerts
    /// * `notify_recovery` — Whether to dispatch recovery events
    /// * `service_name` — Service name for alert payloads
    pub(crate) fn new(
        hooks: Vec<Arc<dyn AuditAlertHook>>,
        threshold_secs: u64,
        cooldown_secs: u64,
        notify_recovery: bool,
        service_name: String,
    ) -> Self {
        Self {
            state: Mutex::new(TrackerState {
                first_failure_at: None,
                first_failure_wall: None,
                consecutive_failures: 0,
                last_error: String::new(),
                last_alert_at: None,
                alert_active: false,
                events_affected_during_outage: 0,
            }),
            hooks,
            threshold: Duration::from_secs(threshold_secs),
            cooldown: Duration::from_secs(cooldown_secs),
            notify_recovery,
            service_name,
        }
    }

    /// Record a storage failure
    ///
    /// Increments failure counters and dispatches an alert if the threshold
    /// is exceeded and the cooldown has elapsed.
    pub(crate) fn record_failure(&self, error: &str) {
        let alert_event = {
            let mut state = self.state.lock().unwrap();
            let now = Instant::now();

            // Initialize outage tracking on first failure
            if state.first_failure_at.is_none() {
                state.first_failure_at = Some(now);
                state.first_failure_wall = Some(Utc::now());
            }

            state.consecutive_failures += 1;
            state.events_affected_during_outage += 1;
            state.last_error = error.to_string();

            // Check if we should alert
            let elapsed = now.duration_since(state.first_failure_at.unwrap());
            if elapsed >= self.threshold {
                let cooldown_ok = state
                    .last_alert_at
                    .map(|last| now.duration_since(last) >= self.cooldown)
                    .unwrap_or(true);

                if cooldown_ok {
                    state.last_alert_at = Some(now);
                    state.alert_active = true;

                    Some(AuditAlertEvent::StorageUnreachable {
                        first_failure_at: state.first_failure_wall.unwrap(),
                        consecutive_failures: state.consecutive_failures,
                        unreachable_duration_secs: elapsed.as_secs(),
                        last_error: state.last_error.clone(),
                        service_name: self.service_name.clone(),
                    })
                } else {
                    None
                }
            } else {
                None
            }
        }; // lock dropped here

        if let Some(event) = alert_event {
            tracing::error!(
                alert = true,
                audit.severity = "ALERT",
                service_name = %self.service_name,
                consecutive_failures = %event.consecutive_failures_count(),
                "Audit storage unreachable"
            );
            self.dispatch(event);
        }
    }

    /// Record a successful storage operation
    ///
    /// Resets failure counters and dispatches a recovery event if an alert
    /// was previously active.
    pub(crate) fn record_success(&self) {
        let recovery_event = {
            let mut state = self.state.lock().unwrap();

            let event = if state.alert_active && self.notify_recovery {
                let now = Utc::now();
                let outage_started = state.first_failure_wall.unwrap();
                let outage_duration = (now - outage_started).num_seconds().max(0) as u64;

                Some(AuditAlertEvent::StorageRecovered {
                    outage_started_at: outage_started,
                    recovered_at: now,
                    outage_duration_secs: outage_duration,
                    events_affected: state.events_affected_during_outage,
                    service_name: self.service_name.clone(),
                })
            } else {
                None
            };

            // Reset all state
            state.first_failure_at = None;
            state.first_failure_wall = None;
            state.consecutive_failures = 0;
            state.last_error.clear();
            state.alert_active = false;
            state.events_affected_during_outage = 0;

            event
        }; // lock dropped here

        if let Some(event) = recovery_event {
            tracing::warn!(
                audit.severity = "NOTICE",
                service_name = %self.service_name,
                "Audit storage recovered"
            );
            self.dispatch(event);
        }
    }

    /// Dispatch an event to all hooks via `tokio::spawn`
    fn dispatch(&self, event: AuditAlertEvent) {
        for hook in &self.hooks {
            let hook = Arc::clone(hook);
            let event = event.clone();
            tokio::spawn(async move {
                hook.on_alert(event).await;
            });
        }
    }
}

/// Helper to extract consecutive_failures from the event for logging
trait ConsecutiveFailuresCount {
    fn consecutive_failures_count(&self) -> u64;
}

impl ConsecutiveFailuresCount for AuditAlertEvent {
    fn consecutive_failures_count(&self) -> u64 {
        match self {
            AuditAlertEvent::StorageUnreachable {
                consecutive_failures,
                ..
            } => *consecutive_failures,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Mock hook that counts alert invocations
    struct MockHook {
        unreachable_count: AtomicU64,
        recovered_count: AtomicU64,
        last_events_affected: AtomicU64,
    }

    impl MockHook {
        fn new() -> Self {
            Self {
                unreachable_count: AtomicU64::new(0),
                recovered_count: AtomicU64::new(0),
                last_events_affected: AtomicU64::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl AuditAlertHook for MockHook {
        async fn on_alert(&self, event: AuditAlertEvent) {
            match event {
                AuditAlertEvent::StorageUnreachable { .. } => {
                    self.unreachable_count.fetch_add(1, Ordering::SeqCst);
                }
                AuditAlertEvent::StorageRecovered {
                    events_affected, ..
                } => {
                    self.recovered_count.fetch_add(1, Ordering::SeqCst);
                    self.last_events_affected
                        .store(events_affected, Ordering::SeqCst);
                }
            }
        }
    }

    #[tokio::test]
    async fn threshold_triggers_alert() {
        let hook = Arc::new(MockHook::new());
        let tracker = FailureTracker::new(
            vec![hook.clone()],
            0, // threshold_secs = 0 → immediate alert
            3600,
            true,
            "test-service".to_string(),
        );

        tracker.record_failure("connection refused");

        // Give tokio::spawn a moment to execute
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(hook.unreachable_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn no_alert_before_threshold() {
        let hook = Arc::new(MockHook::new());
        let tracker = FailureTracker::new(
            vec![hook.clone()],
            60, // threshold_secs = 60
            3600,
            true,
            "test-service".to_string(),
        );

        // Failures within the threshold window should not trigger
        tracker.record_failure("connection refused");
        tracker.record_failure("connection refused");

        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(hook.unreachable_count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn cooldown_prevents_duplicates() {
        let hook = Arc::new(MockHook::new());
        let tracker = FailureTracker::new(
            vec![hook.clone()],
            0,    // threshold_secs = 0 → immediate
            3600, // cooldown_secs = 1 hour
            true,
            "test-service".to_string(),
        );

        tracker.record_failure("error 1");
        tracker.record_failure("error 2");
        tracker.record_failure("error 3");

        tokio::time::sleep(Duration::from_millis(50)).await;

        // Only the first should trigger due to cooldown
        assert_eq!(hook.unreachable_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn recovery_after_alert() {
        let hook = Arc::new(MockHook::new());
        let tracker = FailureTracker::new(
            vec![hook.clone()],
            0, // immediate alert
            3600,
            true, // notify_recovery
            "test-service".to_string(),
        );

        tracker.record_failure("connection refused");
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(hook.unreachable_count.load(Ordering::SeqCst), 1);

        tracker.record_success();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(hook.recovered_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn no_recovery_without_alert() {
        let hook = Arc::new(MockHook::new());
        let tracker = FailureTracker::new(
            vec![hook.clone()],
            60, // high threshold — no alert will fire
            3600,
            true,
            "test-service".to_string(),
        );

        tracker.record_failure("transient error");
        tracker.record_success();

        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(hook.unreachable_count.load(Ordering::SeqCst), 0);
        assert_eq!(hook.recovered_count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn events_affected_count() {
        let hook = Arc::new(MockHook::new());
        let tracker = FailureTracker::new(
            vec![hook.clone()],
            0, // immediate alert
            3600,
            true,
            "test-service".to_string(),
        );

        for _ in 0..5 {
            tracker.record_failure("error");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;

        tracker.record_success();
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(hook.last_events_affected.load(Ordering::SeqCst), 5);
    }

    #[tokio::test]
    async fn multiple_webhooks() {
        let hook1 = Arc::new(MockHook::new());
        let hook2 = Arc::new(MockHook::new());
        let tracker = FailureTracker::new(
            vec![hook1.clone(), hook2.clone()],
            0,
            3600,
            true,
            "test-service".to_string(),
        );

        tracker.record_failure("error");
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(hook1.unreachable_count.load(Ordering::SeqCst), 1);
        assert_eq!(hook2.unreachable_count.load(Ordering::SeqCst), 1);
    }
}
