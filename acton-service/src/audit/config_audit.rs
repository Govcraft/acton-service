//! Configuration change auditing (NIST CM-3)
//!
//! Provides config fingerprinting, sensitive value redaction, and drift detection
//! to satisfy NIST SP 800-53 CM-3 (Configuration Change Control).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::event::{AuditEvent, AuditEventKind, AuditSeverity};
use super::logger::AuditLogger;

/// Sentinel value used to replace sensitive configuration values.
const REDACTED: &str = "[REDACTED]";

/// Field name patterns that indicate sensitive values.
///
/// Matching is case-insensitive by exact name or `_suffix`.
/// Intentionally aggressive — false positives (over-redacting) are safe,
/// while false negatives leak secrets into the audit trail.
const SENSITIVE_PATTERNS: &[&str] = &[
    "password",
    "secret",
    "key",
    "token",
    "auth_token",
    "encryption_key",
    "url",
    "key_path",
];

/// Check if a field name matches any known sensitive pattern.
///
/// Uses exact match or `_suffix` match (case-insensitive).
fn is_sensitive_field(field_name: &str) -> bool {
    let lower = field_name.to_lowercase();
    SENSITIVE_PATTERNS
        .iter()
        .any(|pattern| lower == *pattern || lower.ends_with(&format!("_{}", pattern)))
}

/// Redact sensitive fields from a serialized config snapshot.
///
/// Recursively walks the JSON value and replaces any field whose name
/// matches a known sensitive pattern with `[REDACTED]`.
pub fn redact_config(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut redacted = serde_json::Map::new();
            for (key, val) in map {
                if is_sensitive_field(key) {
                    match val {
                        Value::Object(_) => {
                            redacted.insert(key.clone(), redact_config(val));
                        }
                        _ => {
                            redacted.insert(key.clone(), Value::String(REDACTED.to_string()));
                        }
                    }
                } else {
                    redacted.insert(key.clone(), redact_config(val));
                }
            }
            Value::Object(redacted)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(redact_config).collect()),
        other => other.clone(),
    }
}

/// Compute a BLAKE3 fingerprint of a redacted config snapshot.
///
/// Deterministic: the same config content always produces the same hash.
pub fn compute_config_fingerprint(redacted_config: &Value) -> String {
    let canonical = serde_json::to_string(redacted_config).unwrap_or_else(|_| "{}".to_string());
    blake3::hash(canonical.as_bytes()).to_hex().to_string()
}

/// Collect the list of Cargo features enabled at compile time.
#[allow(clippy::vec_init_then_push)]
pub fn enabled_features() -> Vec<&'static str> {
    let mut features = Vec::new();

    #[cfg(feature = "http")]
    features.push("http");
    #[cfg(feature = "grpc")]
    features.push("grpc");
    #[cfg(feature = "database")]
    features.push("database");
    #[cfg(feature = "turso")]
    features.push("turso");
    #[cfg(feature = "surrealdb")]
    features.push("surrealdb");
    #[cfg(feature = "cache")]
    features.push("cache");
    #[cfg(feature = "events")]
    features.push("events");
    #[cfg(feature = "observability")]
    features.push("observability");
    #[cfg(feature = "resilience")]
    features.push("resilience");
    #[cfg(feature = "otel-metrics")]
    features.push("otel-metrics");
    #[cfg(feature = "governor")]
    features.push("governor");
    #[cfg(feature = "websocket")]
    features.push("websocket");
    #[cfg(feature = "openapi")]
    features.push("openapi");
    #[cfg(feature = "cedar-authz")]
    features.push("cedar-authz");
    #[cfg(feature = "jwt")]
    features.push("jwt");
    #[cfg(feature = "auth")]
    features.push("auth");
    #[cfg(feature = "oauth")]
    features.push("oauth");
    #[cfg(feature = "session")]
    features.push("session");
    #[cfg(feature = "session-memory")]
    features.push("session-memory");
    #[cfg(feature = "session-redis")]
    features.push("session-redis");
    #[cfg(feature = "htmx")]
    features.push("htmx");
    #[cfg(feature = "askama")]
    features.push("askama");
    #[cfg(feature = "sse")]
    features.push("sse");
    #[cfg(feature = "pagination")]
    features.push("pagination");
    #[cfg(feature = "repository")]
    features.push("repository");
    #[cfg(feature = "handlers")]
    features.push("handlers");
    #[cfg(feature = "tls")]
    features.push("tls");
    features.push("audit"); // always true when this code compiles
    #[cfg(feature = "login-lockout")]
    features.push("login-lockout");
    #[cfg(feature = "accounts")]
    features.push("accounts");
    #[cfg(feature = "account-handlers")]
    features.push("account-handlers");

    features
}

/// Build a `ConfigLoaded` audit event with full metadata.
pub fn build_config_loaded_event(
    service_name: &str,
    config_hash: &str,
    redacted_config: &Value,
    environment: &str,
) -> AuditEvent {
    let metadata = serde_json::json!({
        "config_hash": config_hash,
        "redacted_config": redacted_config,
        "enabled_features": enabled_features(),
        "environment": environment,
    });

    AuditEvent::new(
        AuditEventKind::ConfigLoaded,
        AuditSeverity::Informational,
        service_name.to_string(),
    )
    .with_metadata(metadata)
}

/// Result of comparing active config against on-disk sources.
#[derive(Debug, Serialize, Deserialize)]
pub struct DriftCheckResult {
    /// Whether the on-disk config differs from the active config
    pub drift_detected: bool,
    /// BLAKE3 hash of the active (running) configuration
    pub active_hash: String,
    /// BLAKE3 hash of the on-disk configuration
    pub disk_hash: String,
    /// Top-level config sections that changed (if drift detected)
    pub changed_sections: Vec<String>,
}

/// Compare two config values and return which top-level sections differ.
pub fn find_changed_sections(active: &Value, disk: &Value) -> Vec<String> {
    let mut changed = Vec::new();

    if let (Value::Object(active_map), Value::Object(disk_map)) = (active, disk) {
        for (key, active_val) in active_map {
            match disk_map.get(key) {
                Some(disk_val) if active_val != disk_val => {
                    changed.push(key.clone());
                }
                None => {
                    changed.push(format!("{} (removed from disk)", key));
                }
                _ => {}
            }
        }
        for key in disk_map.keys() {
            if !active_map.contains_key(key) {
                changed.push(format!("{} (new on disk)", key));
            }
        }
    }

    changed
}

/// Emit a `ConfigDriftDetected` audit event.
pub async fn emit_drift_event(
    logger: &AuditLogger,
    active_hash: &str,
    disk_hash: &str,
    changed_sections: &[String],
) {
    let metadata = serde_json::json!({
        "active_hash": active_hash,
        "disk_hash": disk_hash,
        "changed_sections": changed_sections,
    });

    let event = AuditEvent::new(
        AuditEventKind::ConfigDriftDetected,
        AuditSeverity::Warning,
        logger.service_name().to_string(),
    )
    .with_metadata(metadata);

    logger.log(event).await;
}

/// `GET /admin/config/drift` endpoint handler.
///
/// Reloads config from disk, computes its fingerprint, and compares
/// against the active config fingerprint stored in AppState.
/// If drift is detected, emits a `ConfigDriftDetected` audit event.
pub async fn drift_check_handler<T>(
    axum::extract::State(state): axum::extract::State<crate::state::AppState<T>>,
) -> axum::response::Response
where
    T: Serialize + serde::de::DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::Json;

    let active_hash = match state.config_fingerprint() {
        Some(hash) => hash.to_string(),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "Config fingerprint not available"
                })),
            )
                .into_response();
        }
    };

    // Reload config from disk using the same Figment sources
    let disk_config =
        match crate::config::Config::<T>::load_for_service(&state.config().service.name) {
            Ok(c) => c,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": format!("Failed to reload config from disk: {}", e)
                    })),
                )
                    .into_response();
            }
        };

    // Serialize, redact, and fingerprint the disk config
    let disk_serialized = match serde_json::to_value(&disk_config) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("Failed to serialize disk config: {}", e)
                })),
            )
                .into_response();
        }
    };
    let disk_redacted = redact_config(&disk_serialized);
    let disk_hash = compute_config_fingerprint(&disk_redacted);

    let drift_detected = active_hash != disk_hash;

    let changed_sections = if drift_detected {
        let active_serialized = serde_json::to_value(state.config()).unwrap_or_default();
        let active_redacted = redact_config(&active_serialized);
        find_changed_sections(&active_redacted, &disk_redacted)
    } else {
        Vec::new()
    };

    // Emit audit event if drift detected
    if drift_detected {
        if let Some(logger) = state.audit_logger() {
            if logger.config().audit_config_events {
                emit_drift_event(logger, &active_hash, &disk_hash, &changed_sections).await;
            }
        }
    }

    let result = DriftCheckResult {
        drift_detected,
        active_hash,
        disk_hash,
        changed_sections,
    };

    (StatusCode::OK, Json(result)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_database_url() {
        let config = serde_json::json!({
            "database": {
                "url": "postgres://user:pass@localhost/db",
                "max_connections": 50
            },
            "service": {
                "name": "test",
                "port": 8080
            }
        });
        let redacted = redact_config(&config);
        assert_eq!(redacted["database"]["url"], "[REDACTED]");
        assert_eq!(redacted["database"]["max_connections"], 50);
        assert_eq!(redacted["service"]["name"], "test");
        assert_eq!(redacted["service"]["port"], 8080);
    }

    #[test]
    fn test_redact_nested_sensitive_fields() {
        let config = serde_json::json!({
            "surrealdb": {
                "password": "secret123",
                "username": "root",
                "namespace": "default"
            },
            "turso": {
                "auth_token": "my-token",
                "encryption_key": "my-key"
            }
        });
        let redacted = redact_config(&config);
        assert_eq!(redacted["surrealdb"]["password"], "[REDACTED]");
        assert_eq!(redacted["surrealdb"]["username"], "root");
        assert_eq!(redacted["surrealdb"]["namespace"], "default");
        assert_eq!(redacted["turso"]["auth_token"], "[REDACTED]");
        assert_eq!(redacted["turso"]["encryption_key"], "[REDACTED]");
    }

    #[test]
    fn test_redact_preserves_non_sensitive() {
        let config = serde_json::json!({
            "service": {
                "name": "my-app",
                "port": 3000,
                "log_level": "info",
                "environment": "production"
            },
            "rate_limit": {
                "per_user_rpm": 60,
                "window_secs": 60
            }
        });
        let redacted = redact_config(&config);
        assert_eq!(redacted, config);
    }

    #[test]
    fn test_redact_arrays() {
        let config = serde_json::json!({
            "items": [
                {"name": "public", "secret": "hidden"},
                {"name": "also-public"}
            ]
        });
        let redacted = redact_config(&config);
        assert_eq!(redacted["items"][0]["name"], "public");
        assert_eq!(redacted["items"][0]["secret"], "[REDACTED]");
        assert_eq!(redacted["items"][1]["name"], "also-public");
    }

    #[test]
    fn test_redact_nested_object_with_sensitive_key() {
        let config = serde_json::json!({
            "token": {
                "format": "paseto",
                "purpose": "local"
            }
        });
        let redacted = redact_config(&config);
        // "token" is a sensitive key, but it's an object so it gets recursed into
        assert_eq!(redacted["token"]["format"], "paseto");
        assert_eq!(redacted["token"]["purpose"], "local");
    }

    #[test]
    fn test_fingerprint_deterministic() {
        let config = serde_json::json!({"service": {"name": "test", "port": 8080}});
        let h1 = compute_config_fingerprint(&config);
        let h2 = compute_config_fingerprint(&config);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_fingerprint_changes_on_modification() {
        let c1 = serde_json::json!({"service": {"port": 8080}});
        let c2 = serde_json::json!({"service": {"port": 9090}});
        assert_ne!(
            compute_config_fingerprint(&c1),
            compute_config_fingerprint(&c2)
        );
    }

    #[test]
    fn test_find_changed_sections_modification() {
        let active = serde_json::json!({
            "service": {"port": 8080},
            "database": {"max_connections": 50}
        });
        let disk = serde_json::json!({
            "service": {"port": 9090},
            "database": {"max_connections": 50}
        });
        let changed = find_changed_sections(&active, &disk);
        assert_eq!(changed, vec!["service"]);
    }

    #[test]
    fn test_find_changed_sections_addition() {
        let active = serde_json::json!({"service": {"port": 8080}});
        let disk = serde_json::json!({
            "service": {"port": 8080},
            "redis": {"url": "redis://localhost"}
        });
        let changed = find_changed_sections(&active, &disk);
        assert_eq!(changed, vec!["redis (new on disk)"]);
    }

    #[test]
    fn test_find_changed_sections_removal() {
        let active = serde_json::json!({
            "service": {"port": 8080},
            "redis": {"url": "redis://localhost"}
        });
        let disk = serde_json::json!({"service": {"port": 8080}});
        let changed = find_changed_sections(&active, &disk);
        assert_eq!(changed, vec!["redis (removed from disk)"]);
    }

    #[test]
    fn test_find_changed_sections_no_drift() {
        let config = serde_json::json!({"service": {"port": 8080}});
        let changed = find_changed_sections(&config, &config);
        assert!(changed.is_empty());
    }

    #[test]
    fn test_is_sensitive_field_positive() {
        assert!(is_sensitive_field("password"));
        assert!(is_sensitive_field("secret"));
        assert!(is_sensitive_field("token"));
        assert!(is_sensitive_field("url"));
        assert!(is_sensitive_field("key"));
        assert!(is_sensitive_field("key_path"));
        assert!(is_sensitive_field("auth_token"));
        assert!(is_sensitive_field("encryption_key"));
        assert!(is_sensitive_field("api_key"));
        assert!(is_sensitive_field("database_url"));
        assert!(is_sensitive_field("db_password"));
    }

    #[test]
    fn test_is_sensitive_field_negative() {
        assert!(!is_sensitive_field("port"));
        assert!(!is_sensitive_field("name"));
        assert!(!is_sensitive_field("max_connections"));
        assert!(!is_sensitive_field("enabled"));
        assert!(!is_sensitive_field("environment"));
        assert!(!is_sensitive_field("log_level"));
        assert!(!is_sensitive_field("timeout_secs"));
    }

    #[test]
    fn test_is_sensitive_field_case_insensitive() {
        assert!(is_sensitive_field("PASSWORD"));
        assert!(is_sensitive_field("Secret"));
        assert!(is_sensitive_field("AUTH_TOKEN"));
    }

    #[test]
    fn test_build_config_loaded_event() {
        let redacted = serde_json::json!({"service": {"name": "test"}});
        let hash = compute_config_fingerprint(&redacted);
        let event = build_config_loaded_event("test-service", &hash, &redacted, "production");

        assert_eq!(event.kind, AuditEventKind::ConfigLoaded);
        assert_eq!(event.service_name, "test-service");

        let metadata = event.metadata.unwrap();
        assert_eq!(metadata["config_hash"], hash);
        assert_eq!(metadata["environment"], "production");
        assert!(metadata["enabled_features"].is_array());
        assert!(metadata["redacted_config"].is_object());
    }

    #[test]
    fn test_enabled_features_includes_audit() {
        let features = enabled_features();
        assert!(features.contains(&"audit"));
    }
}
