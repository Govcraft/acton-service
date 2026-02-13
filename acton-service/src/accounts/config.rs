//! Account management configuration

use serde::{Deserialize, Serialize};

use super::types::AccountStatus;

/// Configuration for account lifecycle management
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AccountsConfig {
    /// Default status for newly created accounts
    #[serde(default = "default_status")]
    pub default_status: AccountStatus,

    /// Whether new accounts require email verification
    #[serde(default = "default_true")]
    pub require_email_verification: bool,

    /// Days of inactivity before auto-expiration (0 = disabled)
    #[serde(default)]
    pub inactivity_expiry_days: u64,

    /// Whether usernames must be unique
    #[serde(default = "default_false")]
    pub unique_usernames: bool,

    /// Whether to emit audit events for account lifecycle changes
    #[serde(default = "default_true")]
    pub audit_events: bool,
}

impl Default for AccountsConfig {
    fn default() -> Self {
        Self {
            default_status: default_status(),
            require_email_verification: true,
            inactivity_expiry_days: 0,
            unique_usernames: false,
            audit_events: true,
        }
    }
}

fn default_status() -> AccountStatus {
    AccountStatus::PendingVerification
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AccountsConfig::default();
        assert_eq!(config.default_status, AccountStatus::PendingVerification);
        assert!(config.require_email_verification);
        assert_eq!(config.inactivity_expiry_days, 0);
        assert!(!config.unique_usernames);
        assert!(config.audit_events);
    }

    #[test]
    fn test_serde_roundtrip() {
        let config = AccountsConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: AccountsConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.default_status, config.default_status);
        assert_eq!(
            deserialized.require_email_verification,
            config.require_email_verification
        );
    }

    #[test]
    fn test_deserialize_with_defaults() {
        let json = "{}";
        let config: AccountsConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.default_status, AccountStatus::PendingVerification);
        assert!(config.require_email_verification);
    }
}
