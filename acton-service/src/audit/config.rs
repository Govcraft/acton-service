//! Audit logging configuration
//!
//! Loaded from `[audit]` section of config.toml or environment variables.

use serde::{Deserialize, Serialize};

/// Audit logging configuration
///
/// Controls which events are captured, where they are sent, and
/// which routes are audited.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditConfig {
    /// Enable audit logging (default: true)
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Audit all HTTP requests (default: false)
    ///
    /// When false, only requests matching `audited_routes` patterns are audited.
    /// Auth events are always audited when `audit_auth_events` is true.
    #[serde(default)]
    pub audit_all_requests: bool,

    /// Automatically emit audit events for auth operations (default: true)
    #[serde(default = "default_true")]
    pub audit_auth_events: bool,

    /// Syslog export configuration
    #[serde(default)]
    pub syslog: SyslogConfig,

    /// Enable OTLP log export (default: false, requires observability feature)
    #[serde(default)]
    pub otlp_logs_enabled: bool,

    /// Glob patterns for routes that should be audited
    ///
    /// Examples: `["/api/v1/admin/*", "/api/v1/users/*/delete"]`
    #[serde(default)]
    pub audited_routes: Vec<String>,

    /// Routes to exclude from auditing (default: ["/health", "/ready", "/metrics"])
    #[serde(default = "default_excluded_routes")]
    pub excluded_routes: Vec<String>,

    /// Days to retain audit events (None = infinite)
    #[serde(default)]
    pub retention_days: Option<u32>,

    /// Directory path for JSONL archive before purge (None = skip archival)
    #[serde(default)]
    pub archive_path: Option<String>,

    /// Hours between cleanup runs (default: 24)
    #[serde(default = "default_cleanup_interval")]
    pub cleanup_interval_hours: u32,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            audit_all_requests: false,
            audit_auth_events: true,
            syslog: SyslogConfig::default(),
            otlp_logs_enabled: false,
            audited_routes: Vec::new(),
            excluded_routes: default_excluded_routes(),
            retention_days: None,
            archive_path: None,
            cleanup_interval_hours: default_cleanup_interval(),
        }
    }
}

/// Syslog export configuration (RFC 5424)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyslogConfig {
    /// Transport protocol: "udp", "tcp", or "none"
    #[serde(default = "default_syslog_transport")]
    pub transport: String,

    /// Syslog server address
    #[serde(default = "default_syslog_address")]
    pub address: String,

    /// Syslog facility code (default: 13 = audit)
    #[serde(default = "default_syslog_facility")]
    pub facility: u8,

    /// Application name in syslog messages
    #[serde(default)]
    pub app_name: Option<String>,
}

impl Default for SyslogConfig {
    fn default() -> Self {
        Self {
            transport: default_syslog_transport(),
            address: default_syslog_address(),
            facility: default_syslog_facility(),
            app_name: None,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_excluded_routes() -> Vec<String> {
    vec![
        "/health".to_string(),
        "/ready".to_string(),
        "/metrics".to_string(),
    ]
}

fn default_syslog_transport() -> String {
    "udp".to_string()
}

fn default_syslog_address() -> String {
    "127.0.0.1:514".to_string()
}

fn default_syslog_facility() -> u8 {
    13 // log_audit
}

fn default_cleanup_interval() -> u32 {
    24
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_config_defaults() {
        let config = AuditConfig::default();
        assert!(config.enabled);
        assert!(!config.audit_all_requests);
        assert!(config.audit_auth_events);
        assert!(!config.otlp_logs_enabled);
        assert!(config.audited_routes.is_empty());
        assert_eq!(
            config.excluded_routes,
            vec!["/health", "/ready", "/metrics"]
        );
        assert!(config.retention_days.is_none());
        assert!(config.archive_path.is_none());
        assert_eq!(config.cleanup_interval_hours, 24);
    }

    #[test]
    fn test_syslog_config_defaults() {
        let config = SyslogConfig::default();
        assert_eq!(config.transport, "udp");
        assert_eq!(config.address, "127.0.0.1:514");
        assert_eq!(config.facility, 13);
        assert!(config.app_name.is_none());
    }

    #[test]
    fn test_audit_config_serde_roundtrip() {
        let config = AuditConfig {
            enabled: true,
            audit_all_requests: true,
            audit_auth_events: false,
            syslog: SyslogConfig {
                transport: "tcp".to_string(),
                address: "syslog.example.com:514".to_string(),
                facility: 10,
                app_name: Some("my-service".to_string()),
            },
            otlp_logs_enabled: true,
            audited_routes: vec!["/api/v1/admin/*".to_string()],
            excluded_routes: vec!["/health".to_string()],
            retention_days: Some(90),
            archive_path: Some("/var/audit/archive".to_string()),
            cleanup_interval_hours: 12,
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: AuditConfig = serde_json::from_str(&json).unwrap();

        assert!(deserialized.audit_all_requests);
        assert!(!deserialized.audit_auth_events);
        assert_eq!(deserialized.syslog.transport, "tcp");
        assert_eq!(deserialized.syslog.facility, 10);
        assert!(deserialized.otlp_logs_enabled);
        assert_eq!(deserialized.audited_routes, vec!["/api/v1/admin/*"]);
        assert_eq!(deserialized.retention_days, Some(90));
        assert_eq!(
            deserialized.archive_path,
            Some("/var/audit/archive".to_string())
        );
        assert_eq!(deserialized.cleanup_interval_hours, 12);
    }

    #[test]
    fn test_retention_fields_default_from_json() {
        // Fields should default when missing from JSON
        let json = r#"{"enabled": true}"#;
        let config: AuditConfig = serde_json::from_str(json).unwrap();
        assert!(config.retention_days.is_none());
        assert!(config.archive_path.is_none());
        assert_eq!(config.cleanup_interval_hours, 24);
    }
}
