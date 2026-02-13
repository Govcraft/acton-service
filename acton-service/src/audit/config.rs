//! Audit logging configuration
//!
//! Loaded from `[audit]` section of config.toml or environment variables.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

    /// Alert hook configuration for storage failure notifications
    #[serde(default)]
    pub alerts: Option<AlertConfig>,
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
            alerts: None,
        }
    }
}

/// Alert hook configuration for audit storage failures
///
/// When enabled, dispatches notifications when persistent storage is
/// unreachable beyond a configurable threshold, and when it recovers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertConfig {
    /// Enable storage failure alerts (default: false)
    #[serde(default)]
    pub enabled: bool,

    /// Seconds of continuous failure before triggering an alert (default: 30)
    #[serde(default = "default_threshold_secs")]
    pub threshold_secs: u64,

    /// Minimum seconds between repeated alerts (default: 300)
    #[serde(default = "default_cooldown_secs")]
    pub cooldown_secs: u64,

    /// Send a recovery notification when storage comes back online (default: true)
    #[serde(default = "default_true")]
    pub notify_recovery: bool,

    /// Webhook endpoints to notify
    #[serde(default)]
    pub webhooks: Vec<WebhookAlertConfig>,
}

impl Default for AlertConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold_secs: default_threshold_secs(),
            cooldown_secs: default_cooldown_secs(),
            notify_recovery: true,
            webhooks: Vec::new(),
        }
    }
}

/// Configuration for a single webhook alert endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookAlertConfig {
    /// Destination URL for POST requests
    pub url: String,

    /// HTTP request timeout in seconds (default: 10)
    #[serde(default = "default_webhook_timeout_secs")]
    pub timeout_secs: u64,

    /// Additional HTTP headers (e.g., `{"Authorization": "Bearer token"}`)
    #[serde(default)]
    pub headers: HashMap<String, String>,
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

fn default_threshold_secs() -> u64 {
    30
}

fn default_cooldown_secs() -> u64 {
    300
}

fn default_webhook_timeout_secs() -> u64 {
    10
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
        assert!(config.alerts.is_none());
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
            alerts: None,
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: AuditConfig = serde_json::from_str(&json).unwrap();

        assert!(deserialized.audit_all_requests);
        assert!(!deserialized.audit_auth_events);
        assert_eq!(deserialized.syslog.transport, "tcp");
        assert_eq!(deserialized.syslog.facility, 10);
        assert!(deserialized.otlp_logs_enabled);
        assert_eq!(deserialized.audited_routes, vec!["/api/v1/admin/*"]);
    }

    #[test]
    fn test_alert_config_defaults() {
        let config = AlertConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.threshold_secs, 30);
        assert_eq!(config.cooldown_secs, 300);
        assert!(config.notify_recovery);
        assert!(config.webhooks.is_empty());
    }

    #[test]
    fn test_alert_config_serde_roundtrip() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer test-token".to_string());

        let config = AlertConfig {
            enabled: true,
            threshold_secs: 15,
            cooldown_secs: 120,
            notify_recovery: false,
            webhooks: vec![WebhookAlertConfig {
                url: "https://hooks.slack.com/test".to_string(),
                timeout_secs: 5,
                headers: headers.clone(),
            }],
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: AlertConfig = serde_json::from_str(&json).unwrap();

        assert!(deserialized.enabled);
        assert_eq!(deserialized.threshold_secs, 15);
        assert_eq!(deserialized.cooldown_secs, 120);
        assert!(!deserialized.notify_recovery);
        assert_eq!(deserialized.webhooks.len(), 1);
        assert_eq!(deserialized.webhooks[0].url, "https://hooks.slack.com/test");
        assert_eq!(deserialized.webhooks[0].timeout_secs, 5);
        assert_eq!(
            deserialized.webhooks[0]
                .headers
                .get("Authorization")
                .unwrap(),
            "Bearer test-token"
        );
    }

    #[test]
    fn test_audit_config_with_alerts_json() {
        let json_str = r#"{
            "enabled": true,
            "audit_all_requests": false,
            "audit_auth_events": true,
            "otlp_logs_enabled": false,
            "syslog": {
                "transport": "udp",
                "address": "127.0.0.1:514",
                "facility": 13
            },
            "alerts": {
                "enabled": true,
                "threshold_secs": 30,
                "cooldown_secs": 300,
                "notify_recovery": true,
                "webhooks": [
                    {
                        "url": "https://hooks.slack.com/services/T00/B00/xxx",
                        "timeout_secs": 10
                    }
                ]
            }
        }"#;

        let config: AuditConfig = serde_json::from_str(json_str).unwrap();
        assert!(config.enabled);
        let alerts = config.alerts.unwrap();
        assert!(alerts.enabled);
        assert_eq!(alerts.threshold_secs, 30);
        assert_eq!(alerts.cooldown_secs, 300);
        assert!(alerts.notify_recovery);
        assert_eq!(alerts.webhooks.len(), 1);
        assert_eq!(
            alerts.webhooks[0].url,
            "https://hooks.slack.com/services/T00/B00/xxx"
        );
    }
}
