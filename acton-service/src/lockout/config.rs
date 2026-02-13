//! Login lockout configuration
//!
//! Configures progressive delay and account lockout behavior for brute force protection.

use serde::{Deserialize, Serialize};

/// Login lockout configuration
///
/// Controls progressive delays and account lockout thresholds.
/// All durations are in seconds or milliseconds as noted.
///
/// # Example (config.toml)
///
/// ```toml
/// [lockout]
/// enabled = true
/// max_attempts = 5
/// window_secs = 900
/// lockout_duration_secs = 1800
/// progressive_delay_enabled = true
/// base_delay_ms = 1000
/// max_delay_ms = 30000
/// delay_multiplier = 2.0
/// warning_threshold = 3
/// key_prefix = "lockout"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct LockoutConfig {
    /// Whether lockout enforcement is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Maximum failed attempts before account is locked
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,

    /// Window in seconds during which failed attempts are counted
    #[serde(default = "default_window_secs")]
    pub window_secs: u64,

    /// Duration in seconds that an account remains locked
    #[serde(default = "default_lockout_duration_secs")]
    pub lockout_duration_secs: u64,

    /// Whether to apply progressive delays after each failure
    #[serde(default = "default_true")]
    pub progressive_delay_enabled: bool,

    /// Base delay in milliseconds for progressive delay (first failure)
    #[serde(default = "default_base_delay_ms")]
    pub base_delay_ms: u64,

    /// Maximum delay in milliseconds (cap for progressive delay)
    #[serde(default = "default_max_delay_ms")]
    pub max_delay_ms: u64,

    /// Multiplier for exponential backoff (delay = base * multiplier^(attempts-1))
    #[serde(default = "default_delay_multiplier")]
    pub delay_multiplier: f64,

    /// Number of attempts before a warning notification is sent (0 = disabled)
    #[serde(default = "default_warning_threshold")]
    pub warning_threshold: u32,

    /// Redis key prefix for lockout keys
    #[serde(default = "default_key_prefix")]
    pub key_prefix: String,
}

impl LockoutConfig {
    /// Validate the configuration, returning an error message if invalid
    pub fn validate(&self) -> Result<(), String> {
        if self.key_prefix.is_empty() {
            return Err("key_prefix must not be empty".to_string());
        }
        if self.key_prefix.contains(':') {
            return Err("key_prefix must not contain ':'".to_string());
        }
        if self.key_prefix.contains(char::is_whitespace) {
            return Err("key_prefix must not contain whitespace".to_string());
        }
        if self.max_attempts == 0 {
            return Err("max_attempts must be greater than 0".to_string());
        }
        if self.window_secs == 0 {
            return Err("window_secs must be greater than 0".to_string());
        }
        if self.lockout_duration_secs == 0 {
            return Err("lockout_duration_secs must be greater than 0".to_string());
        }
        if self.delay_multiplier < 1.0 {
            return Err("delay_multiplier must be >= 1.0".to_string());
        }
        Ok(())
    }
}

impl Default for LockoutConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_attempts: default_max_attempts(),
            window_secs: default_window_secs(),
            lockout_duration_secs: default_lockout_duration_secs(),
            progressive_delay_enabled: true,
            base_delay_ms: default_base_delay_ms(),
            max_delay_ms: default_max_delay_ms(),
            delay_multiplier: default_delay_multiplier(),
            warning_threshold: default_warning_threshold(),
            key_prefix: default_key_prefix(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_max_attempts() -> u32 {
    5
}

fn default_window_secs() -> u64 {
    900 // 15 minutes
}

fn default_lockout_duration_secs() -> u64 {
    1800 // 30 minutes
}

fn default_base_delay_ms() -> u64 {
    1000 // 1 second
}

fn default_max_delay_ms() -> u64 {
    30000 // 30 seconds
}

fn default_delay_multiplier() -> f64 {
    2.0
}

fn default_warning_threshold() -> u32 {
    3
}

fn default_key_prefix() -> String {
    "lockout".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = LockoutConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_attempts, 5);
        assert_eq!(config.window_secs, 900);
        assert_eq!(config.lockout_duration_secs, 1800);
        assert!(config.progressive_delay_enabled);
        assert_eq!(config.base_delay_ms, 1000);
        assert_eq!(config.max_delay_ms, 30000);
        assert!((config.delay_multiplier - 2.0).abs() < f64::EPSILON);
        assert_eq!(config.warning_threshold, 3);
        assert_eq!(config.key_prefix, "lockout");
    }

    #[test]
    fn test_validate_valid_config() {
        let config = LockoutConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_empty_key_prefix() {
        let mut config = LockoutConfig::default();
        config.key_prefix = "".to_string();
        assert_eq!(
            config.validate(),
            Err("key_prefix must not be empty".to_string())
        );
    }

    #[test]
    fn test_validate_key_prefix_with_colon() {
        let mut config = LockoutConfig::default();
        config.key_prefix = "my:prefix".to_string();
        assert_eq!(
            config.validate(),
            Err("key_prefix must not contain ':'".to_string())
        );
    }

    #[test]
    fn test_validate_key_prefix_with_whitespace() {
        let mut config = LockoutConfig::default();
        config.key_prefix = "my prefix".to_string();
        assert_eq!(
            config.validate(),
            Err("key_prefix must not contain whitespace".to_string())
        );
    }

    #[test]
    fn test_validate_zero_max_attempts() {
        let mut config = LockoutConfig::default();
        config.max_attempts = 0;
        assert_eq!(
            config.validate(),
            Err("max_attempts must be greater than 0".to_string())
        );
    }

    #[test]
    fn test_validate_zero_window_secs() {
        let mut config = LockoutConfig::default();
        config.window_secs = 0;
        assert_eq!(
            config.validate(),
            Err("window_secs must be greater than 0".to_string())
        );
    }

    #[test]
    fn test_validate_zero_lockout_duration() {
        let mut config = LockoutConfig::default();
        config.lockout_duration_secs = 0;
        assert_eq!(
            config.validate(),
            Err("lockout_duration_secs must be greater than 0".to_string())
        );
    }

    #[test]
    fn test_validate_low_multiplier() {
        let mut config = LockoutConfig::default();
        config.delay_multiplier = 0.5;
        assert_eq!(
            config.validate(),
            Err("delay_multiplier must be >= 1.0".to_string())
        );
    }

    #[test]
    fn test_serde_roundtrip() {
        let config = LockoutConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: LockoutConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.max_attempts, config.max_attempts);
        assert_eq!(deserialized.window_secs, config.window_secs);
        assert_eq!(deserialized.key_prefix, config.key_prefix);
    }
}
