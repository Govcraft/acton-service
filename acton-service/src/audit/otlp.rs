//! OpenTelemetry log export for audit events
//!
//! When both `audit` and `observability` features are enabled, audit events
//! can be emitted as OpenTelemetry log records via the tracing infrastructure
//! when [`AuditConfig::otlp_logs_enabled`](super::config::AuditConfig::otlp_logs_enabled)
//! is enabled.
//!
//! This module uses `tracing::info!` with structured fields that map to
//! OpenTelemetry semantic conventions, so they are automatically exported
//! by the OTLP tracing exporter configured in the `observability` module.

use super::event::AuditEvent;

/// Emit an audit event as an OpenTelemetry log record
///
/// Uses tracing's structured logging which is already wired to the OTLP exporter.
/// The audit agent calls this only when OTLP audit logging is enabled. Calling
/// it directly emits a record to all configured tracing subscribers.
pub fn emit_audit_log(event: &AuditEvent) {
    tracing::info!(
        audit.event.id = %event.id,
        audit.event.kind = %event.kind,
        audit.event.severity = %event.severity,
        audit.event.sequence = event.sequence,
        audit.event.service_name = %event.service_name,
        audit.event.hash = event.hash.as_deref().unwrap_or("-"),
        audit.source.ip = event.source.ip.as_deref().unwrap_or("-"),
        audit.source.subject = event.source.subject.as_deref().unwrap_or("-"),
        audit.source.request_id = event.source.request_id.as_deref().unwrap_or("-"),
        http.method = event.method.as_deref().unwrap_or("-"),
        http.path = event.path.as_deref().unwrap_or("-"),
        http.status_code = event.status_code.unwrap_or(0),
        http.duration_ms = event.duration_ms.unwrap_or(0),
        "audit.event"
    );
}
