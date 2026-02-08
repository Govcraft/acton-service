//! Audit logger — thin public API wrapper around the agent handle
//!
//! `AuditLogger` provides a fire-and-forget API for emitting audit events.
//! Events are sent to the `AuditAgent` via `ActorHandle::send()` (non-blocking),
//! so audit logging never blocks request handling.

use acton_reactive::prelude::{ActorHandle, ActorHandleInterface};

use super::config::AuditConfig;
use super::event::{AuditEvent, AuditEventKind, AuditSeverity, AuditSource};

/// Audit logger for emitting events to the audit trail
///
/// This is a thin wrapper around the `AuditAgent`'s actor handle.
/// All methods are fire-and-forget — they send a message to the agent
/// and return immediately without waiting for persistence.
///
/// Available via `state.audit_logger()` when the `audit` feature is enabled.
#[derive(Clone)]
pub struct AuditLogger {
    handle: ActorHandle,
    service_name: String,
    config: AuditConfig,
}

impl AuditLogger {
    /// Create a new audit logger wrapping the given agent handle
    pub fn new(handle: ActorHandle, service_name: String, config: AuditConfig) -> Self {
        Self {
            handle,
            service_name,
            config,
        }
    }

    /// Get the audit configuration
    pub fn config(&self) -> &AuditConfig {
        &self.config
    }

    /// Log an audit event (fire-and-forget)
    pub async fn log(&self, event: AuditEvent) {
        let _ = self.handle.send(event).await;
    }

    /// Log an auth event with source information
    pub async fn log_auth(
        &self,
        kind: AuditEventKind,
        severity: AuditSeverity,
        source: AuditSource,
    ) {
        let event = AuditEvent::new(kind, severity, self.service_name.clone()).with_source(source);
        self.log(event).await;
    }

    /// Log a custom event
    pub async fn log_custom(
        &self,
        name: &str,
        severity: AuditSeverity,
        metadata: Option<serde_json::Value>,
    ) {
        let mut event = AuditEvent::new(
            AuditEventKind::Custom(name.to_string()),
            severity,
            self.service_name.clone(),
        );
        event.metadata = metadata;
        self.log(event).await;
    }

    /// Get the service name
    pub fn service_name(&self) -> &str {
        &self.service_name
    }
}
