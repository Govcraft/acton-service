//! Key rotation configuration
//!
//! Loaded from `[auth.key_rotation]` section of config.toml or environment variables.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Key rotation configuration for automated signing key lifecycle management
///
/// When enabled, signing keys are rotated on a configurable schedule with
/// zero-downtime draining windows to ensure no valid tokens are rejected
/// during rotation.
///
/// # Example (config.toml)
///
/// ```toml
/// [auth.key_rotation]
/// enabled = true
/// rotation_period_secs = 86400
/// drain_grace_period_secs = 300
/// check_interval_secs = 60
/// retention_days = 90
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRotationConfig {
    /// Enable automated key rotation (default: false)
    ///
    /// When false, the service uses static keys loaded from files.
    /// When true, keys are managed in the database with periodic rotation.
    #[serde(default)]
    pub enabled: bool,

    /// Seconds between key rotations (default: 86400 = 24 hours)
    ///
    /// The active signing key will be rotated after this many seconds
    /// have elapsed since its activation.
    #[serde(default = "default_rotation_period")]
    pub rotation_period_secs: u64,

    /// Grace period in seconds added to the drain window (default: 300 = 5 minutes)
    ///
    /// After rotation, the old key enters a draining state and continues
    /// to validate tokens for `max(access_token_ttl, refresh_token_ttl) + drain_grace_period_secs`.
    #[serde(default = "default_drain_grace_period")]
    pub drain_grace_period_secs: u64,

    /// Seconds between key status check ticks (default: 60)
    ///
    /// Controls how frequently the agent checks whether a rotation is
    /// needed and whether draining keys have expired.
    #[serde(default = "default_check_interval")]
    pub check_interval_secs: u64,

    /// Days to retain retired key metadata for audit trail (default: 90)
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,

    /// Path to an initial key file for bootstrapping
    ///
    /// When set and no active key exists in the database, this key file
    /// is read and stored as the first active signing key.
    #[serde(default)]
    pub bootstrap_key_path: Option<PathBuf>,
}

impl Default for KeyRotationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            rotation_period_secs: default_rotation_period(),
            drain_grace_period_secs: default_drain_grace_period(),
            check_interval_secs: default_check_interval(),
            retention_days: default_retention_days(),
            bootstrap_key_path: None,
        }
    }
}

fn default_rotation_period() -> u64 {
    86400 // 24 hours
}

fn default_drain_grace_period() -> u64 {
    300 // 5 minutes
}

fn default_check_interval() -> u64 {
    60 // 1 minute
}

fn default_retention_days() -> u32 {
    90
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_values() {
        let config = KeyRotationConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.rotation_period_secs, 86400);
        assert_eq!(config.drain_grace_period_secs, 300);
        assert_eq!(config.check_interval_secs, 60);
        assert_eq!(config.retention_days, 90);
        assert!(config.bootstrap_key_path.is_none());
    }

    #[test]
    fn test_serde_roundtrip() {
        let config = KeyRotationConfig {
            enabled: true,
            rotation_period_secs: 3600,
            drain_grace_period_secs: 120,
            check_interval_secs: 30,
            retention_days: 180,
            bootstrap_key_path: Some(PathBuf::from("/etc/keys/initial.key")),
        };

        let json = serde_json::to_string(&config).expect("serialize");
        let deserialized: KeyRotationConfig = serde_json::from_str(&json).expect("deserialize");

        assert!(deserialized.enabled);
        assert_eq!(deserialized.rotation_period_secs, 3600);
        assert_eq!(deserialized.drain_grace_period_secs, 120);
        assert_eq!(deserialized.check_interval_secs, 30);
        assert_eq!(deserialized.retention_days, 180);
        assert_eq!(
            deserialized.bootstrap_key_path,
            Some(PathBuf::from("/etc/keys/initial.key"))
        );
    }

    #[test]
    fn test_serde_defaults_from_partial_json() {
        let json = r#"{"enabled": true}"#;
        let config: KeyRotationConfig = serde_json::from_str(json).expect("deserialize");
        assert!(config.enabled);
        assert_eq!(config.rotation_period_secs, 86400);
        assert_eq!(config.drain_grace_period_secs, 300);
        assert_eq!(config.check_interval_secs, 60);
        assert_eq!(config.retention_days, 90);
        assert!(config.bootstrap_key_path.is_none());
    }

    #[test]
    fn test_serde_empty_json_uses_defaults() {
        let json = "{}";
        let config: KeyRotationConfig = serde_json::from_str(json).expect("deserialize");
        assert!(!config.enabled);
        assert_eq!(config.rotation_period_secs, 86400);
    }
}
