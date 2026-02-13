//! Audit alert hooks
//!
//! Provides a trait for receiving audit storage failure alerts (storage
//! unreachable, storage recovered). Notifications are dispatched via
//! `tokio::spawn` so they never block audit event processing.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Events emitted when audit storage health changes
///
/// Dispatched to [`AuditAlertHook`] handlers via fire-and-forget
/// `tokio::spawn`, so handlers should be lightweight and non-blocking.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AuditAlertEvent {
    /// Storage has been unreachable beyond the configured threshold
    StorageUnreachable {
        /// Wall-clock time of the first failure in the current outage
        first_failure_at: DateTime<Utc>,
        /// Number of consecutive storage failures
        consecutive_failures: u64,
        /// How long storage has been unreachable (seconds)
        unreachable_duration_secs: u64,
        /// Error message from the most recent failure
        last_error: String,
        /// Name of the service experiencing the outage
        service_name: String,
    },
    /// Storage has recovered after a previous alert
    StorageRecovered {
        /// When the outage started
        outage_started_at: DateTime<Utc>,
        /// When the outage was resolved
        recovered_at: DateTime<Utc>,
        /// Total outage duration (seconds)
        outage_duration_secs: u64,
        /// Number of audit events affected during the outage
        events_affected: u64,
        /// Name of the service that recovered
        service_name: String,
    },
}

/// Trait for receiving audit storage failure alerts
///
/// Implement this trait to react to storage health changes (e.g., send
/// webhook notifications, page on-call, emit metrics). Handlers are
/// invoked asynchronously and must not panic.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::audit::{AuditAlertHook, AuditAlertEvent};
///
/// struct SlackNotifier { /* ... */ }
///
/// #[async_trait]
/// impl AuditAlertHook for SlackNotifier {
///     async fn on_alert(&self, event: AuditAlertEvent) {
///         if let AuditAlertEvent::StorageUnreachable { service_name, .. } = event {
///             // post_to_slack(&format!("Audit storage down for {}", service_name)).await;
///         }
///     }
/// }
/// ```
#[async_trait]
pub trait AuditAlertHook: Send + Sync + 'static {
    /// Called when audit storage health changes
    ///
    /// This method is invoked inside `tokio::spawn`, so it will not
    /// block audit event processing. Implementations should handle their
    /// own errors internally (log and continue).
    async fn on_alert(&self, event: AuditAlertEvent);
}
