//! Audit event types
//!
//! Core types for representing audit trail events including authentication,
//! HTTP requests, and custom application events.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A single audit trail event
///
/// Events are sealed by [`AuditChain`](super::AuditChain) with BLAKE3 hash chaining
/// before being persisted, providing tamper detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Unique event identifier
    pub id: Uuid,
    /// When the event occurred
    pub timestamp: DateTime<Utc>,
    /// Category of the event
    pub kind: AuditEventKind,
    /// Severity level (maps to syslog severity 0-7)
    pub severity: AuditSeverity,
    /// Source information (IP, user agent, subject, request ID)
    pub source: AuditSource,
    /// HTTP method (if applicable)
    pub method: Option<String>,
    /// Request path (if applicable)
    pub path: Option<String>,
    /// HTTP status code (if applicable)
    pub status_code: Option<u16>,
    /// Request duration in milliseconds (if applicable)
    pub duration_ms: Option<u64>,
    /// Name of the service that generated this event
    pub service_name: String,
    /// Additional structured metadata
    pub metadata: Option<serde_json::Value>,
    /// BLAKE3 hash of this event (set by AuditChain::seal)
    pub hash: Option<String>,
    /// Hash of the previous event in the chain
    pub previous_hash: Option<String>,
    /// Monotonically increasing sequence number
    pub sequence: u64,
}

impl AuditEvent {
    /// Create a new audit event with the given kind and severity
    pub fn new(kind: AuditEventKind, severity: AuditSeverity, service_name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            kind,
            severity,
            source: AuditSource::default(),
            method: None,
            path: None,
            status_code: None,
            duration_ms: None,
            service_name,
            metadata: None,
            hash: None,
            previous_hash: None,
            sequence: 0,
        }
    }

    /// Set the source information
    pub fn with_source(mut self, source: AuditSource) -> Self {
        self.source = source;
        self
    }

    /// Set HTTP request details
    pub fn with_http(
        mut self,
        method: String,
        path: String,
        status_code: Option<u16>,
        duration_ms: Option<u64>,
    ) -> Self {
        self.method = Some(method);
        self.path = Some(path);
        self.status_code = status_code;
        self.duration_ms = duration_ms;
        self
    }

    /// Set additional metadata
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// Categories of audit events
///
/// Auth events are automatically emitted when the `auth` feature is active.
/// HTTP events come from the audit middleware. Custom events are user-defined.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuditEventKind {
    /// Successful authentication
    AuthLoginSuccess,
    /// Failed authentication attempt
    AuthLoginFailed,
    /// User logout
    AuthLogout,
    /// Token refresh
    AuthTokenRefresh,
    /// Token revocation
    AuthTokenRevoked,
    /// Password changed
    AuthPasswordChanged,
    /// API key created
    AuthApiKeyCreated,
    /// API key revoked
    AuthApiKeyRevoked,
    /// OAuth callback processed
    AuthOAuthCallback,
    /// Permission denied
    AuthPermissionDenied,
    /// Account locked due to repeated login failures (requires `login-lockout` feature)
    #[cfg(feature = "login-lockout")]
    AuthAccountLocked,
    /// Account unlocked (requires `login-lockout` feature)
    #[cfg(feature = "login-lockout")]
    AuthAccountUnlocked,
    /// HTTP request (from audit middleware)
    HttpRequest,
    /// HTTP request denied (rate limit, auth failure, etc.)
    HttpRequestDenied,
    /// Application-defined event
    Custom(String),
}

impl std::fmt::Display for AuditEventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AuthLoginSuccess => write!(f, "auth.login.success"),
            Self::AuthLoginFailed => write!(f, "auth.login.failed"),
            Self::AuthLogout => write!(f, "auth.logout"),
            Self::AuthTokenRefresh => write!(f, "auth.token.refresh"),
            Self::AuthTokenRevoked => write!(f, "auth.token.revoked"),
            Self::AuthPasswordChanged => write!(f, "auth.password.changed"),
            Self::AuthApiKeyCreated => write!(f, "auth.apikey.created"),
            Self::AuthApiKeyRevoked => write!(f, "auth.apikey.revoked"),
            Self::AuthOAuthCallback => write!(f, "auth.oauth.callback"),
            Self::AuthPermissionDenied => write!(f, "auth.permission.denied"),
            #[cfg(feature = "login-lockout")]
            Self::AuthAccountLocked => write!(f, "auth.account.locked"),
            #[cfg(feature = "login-lockout")]
            Self::AuthAccountUnlocked => write!(f, "auth.account.unlocked"),
            Self::HttpRequest => write!(f, "http.request"),
            Self::HttpRequestDenied => write!(f, "http.request.denied"),
            Self::Custom(name) => write!(f, "custom.{}", name),
        }
    }
}

/// Audit event severity levels
///
/// Maps directly to syslog severity values (RFC 5424).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum AuditSeverity {
    /// System is unusable (syslog 0)
    Emergency = 0,
    /// Action must be taken immediately (syslog 1)
    Alert = 1,
    /// Critical conditions (syslog 2)
    Critical = 2,
    /// Error conditions (syslog 3)
    Error = 3,
    /// Warning conditions (syslog 4)
    Warning = 4,
    /// Normal but significant condition (syslog 5)
    Notice = 5,
    /// Informational messages (syslog 6)
    Informational = 6,
    /// Debug-level messages (syslog 7)
    Debug = 7,
}

impl AuditSeverity {
    /// Get the numeric syslog severity value (0-7)
    pub fn as_syslog_severity(&self) -> u8 {
        *self as u8
    }
}

impl std::fmt::Display for AuditSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Emergency => write!(f, "EMERGENCY"),
            Self::Alert => write!(f, "ALERT"),
            Self::Critical => write!(f, "CRITICAL"),
            Self::Error => write!(f, "ERROR"),
            Self::Warning => write!(f, "WARNING"),
            Self::Notice => write!(f, "NOTICE"),
            Self::Informational => write!(f, "INFO"),
            Self::Debug => write!(f, "DEBUG"),
        }
    }
}

/// Source information for an audit event
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditSource {
    /// Client IP address
    pub ip: Option<String>,
    /// User agent string
    pub user_agent: Option<String>,
    /// Authenticated subject (user ID, service account, etc.)
    pub subject: Option<String>,
    /// Request ID for correlation
    pub request_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_event_new() {
        let event = AuditEvent::new(
            AuditEventKind::AuthLoginSuccess,
            AuditSeverity::Informational,
            "test-service".to_string(),
        );
        assert_eq!(event.kind, AuditEventKind::AuthLoginSuccess);
        assert_eq!(event.service_name, "test-service");
        assert!(event.hash.is_none());
        assert_eq!(event.sequence, 0);
    }

    #[test]
    fn test_audit_event_with_http() {
        let event = AuditEvent::new(
            AuditEventKind::HttpRequest,
            AuditSeverity::Informational,
            "test-service".to_string(),
        )
        .with_http("GET".into(), "/api/v1/users".into(), Some(200), Some(42));

        assert_eq!(event.method, Some("GET".to_string()));
        assert_eq!(event.path, Some("/api/v1/users".to_string()));
        assert_eq!(event.status_code, Some(200));
        assert_eq!(event.duration_ms, Some(42));
    }

    #[test]
    fn test_audit_event_kind_display() {
        assert_eq!(
            AuditEventKind::AuthLoginSuccess.to_string(),
            "auth.login.success"
        );
        assert_eq!(AuditEventKind::HttpRequest.to_string(), "http.request");
        assert_eq!(
            AuditEventKind::Custom("user.delete".to_string()).to_string(),
            "custom.user.delete"
        );
    }

    #[test]
    fn test_audit_severity_syslog_value() {
        assert_eq!(AuditSeverity::Emergency.as_syslog_severity(), 0);
        assert_eq!(AuditSeverity::Alert.as_syslog_severity(), 1);
        assert_eq!(AuditSeverity::Informational.as_syslog_severity(), 6);
        assert_eq!(AuditSeverity::Debug.as_syslog_severity(), 7);
    }

    #[test]
    fn test_audit_event_serde_roundtrip() {
        let event = AuditEvent::new(
            AuditEventKind::AuthLoginFailed,
            AuditSeverity::Warning,
            "test".to_string(),
        )
        .with_source(AuditSource {
            ip: Some("192.168.1.1".to_string()),
            user_agent: Some("curl/8.0".to_string()),
            subject: None,
            request_id: Some("req-123".to_string()),
        });

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: AuditEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, event.id);
        assert_eq!(deserialized.kind, AuditEventKind::AuthLoginFailed);
        assert_eq!(deserialized.source.ip, Some("192.168.1.1".to_string()));
    }
}
