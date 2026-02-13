//! Signing key metadata types for key rotation
//!
//! Defines the core types used to track signing key lifecycle:
//! [`SigningKeyMetadata`], [`KeyStatus`], and [`KeyFormat`].

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

// ---------------------------------------------------------------------------
// KeyFormat
// ---------------------------------------------------------------------------

/// The cryptographic format of a signing key
///
/// Determines which token generator and validator can use this key.
/// Stored as a string in the database via [`Display`] and [`FromStr`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KeyFormat {
    /// PASETO v4.local (symmetric encryption)
    PasetoV4Local,
    /// PASETO v4.public (Ed25519 signing)
    PasetoV4Public,
    /// JWT with RS256 (RSA PKCS#1 v1.5 + SHA-256)
    JwtRs256,
    /// JWT with ES256 (ECDSA P-256 + SHA-256)
    JwtEs256,
}

impl fmt::Display for KeyFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PasetoV4Local => write!(f, "paseto_v4_local"),
            Self::PasetoV4Public => write!(f, "paseto_v4_public"),
            Self::JwtRs256 => write!(f, "jwt_rs256"),
            Self::JwtEs256 => write!(f, "jwt_es256"),
        }
    }
}

/// Error returned when parsing a [`KeyFormat`] from a string fails
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseKeyFormatError {
    /// The invalid input that could not be parsed
    pub input: String,
}

impl fmt::Display for ParseKeyFormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid key format '{}': expected one of \
             paseto_v4_local, paseto_v4_public, jwt_rs256, jwt_es256",
            self.input
        )
    }
}

impl std::error::Error for ParseKeyFormatError {}

impl FromStr for KeyFormat {
    type Err = ParseKeyFormatError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "paseto_v4_local" => Ok(Self::PasetoV4Local),
            "paseto_v4_public" => Ok(Self::PasetoV4Public),
            "jwt_rs256" => Ok(Self::JwtRs256),
            "jwt_es256" => Ok(Self::JwtEs256),
            other => Err(ParseKeyFormatError {
                input: other.to_string(),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// KeyStatus
// ---------------------------------------------------------------------------

/// The lifecycle status of a signing key
///
/// Keys progress through: `Active` -> `Draining` -> `Retired`.
/// Stored as a string in the database via [`Display`] and [`FromStr`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KeyStatus {
    /// Key is currently used for signing new tokens AND validating existing tokens.
    /// Only one key per service should be Active at any time.
    Active,
    /// Key no longer signs new tokens but still validates existing tokens
    /// during the drain window.
    Draining,
    /// Key's drain window has expired. Metadata retained for audit trail only.
    Retired,
}

impl fmt::Display for KeyStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Draining => write!(f, "draining"),
            Self::Retired => write!(f, "retired"),
        }
    }
}

/// Error returned when parsing a [`KeyStatus`] from a string fails
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseKeyStatusError {
    /// The invalid input that could not be parsed
    pub input: String,
}

impl fmt::Display for ParseKeyStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid key status '{}': expected one of active, draining, retired",
            self.input
        )
    }
}

impl std::error::Error for ParseKeyStatusError {}

impl FromStr for KeyStatus {
    type Err = ParseKeyStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(Self::Active),
            "draining" => Ok(Self::Draining),
            "retired" => Ok(Self::Retired),
            other => Err(ParseKeyStatusError {
                input: other.to_string(),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// SigningKeyMetadata
// ---------------------------------------------------------------------------

/// Metadata for a signing key managed by the key rotation system
///
/// Contains the key material, lifecycle status, and timestamps tracking
/// each state transition. The `key_hash` field provides BLAKE3 integrity
/// verification of the key material.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningKeyMetadata {
    /// Unique key identifier (UUID-based, time-sortable)
    pub kid: String,

    /// Cryptographic format of this key
    pub format: KeyFormat,

    /// Base64-encoded key material
    pub key_material: String,

    /// Current lifecycle status
    pub status: KeyStatus,

    /// When this key was first generated
    pub created_at: DateTime<Utc>,

    /// When this key was promoted to Active status
    pub activated_at: Option<DateTime<Utc>>,

    /// When this key transitioned from Active to Draining
    pub draining_since: Option<DateTime<Utc>>,

    /// When this key transitioned from Draining to Retired
    pub retired_at: Option<DateTime<Utc>>,

    /// When the drain window expires (triggers transition to Retired)
    pub drain_expires_at: Option<DateTime<Utc>>,

    /// The service that owns this key
    pub service_name: String,

    /// BLAKE3 hash of the key material for integrity verification
    pub key_hash: String,
}

impl PartialEq for SigningKeyMetadata {
    fn eq(&self, other: &Self) -> bool {
        self.kid == other.kid
    }
}

impl Eq for SigningKeyMetadata {}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // KeyFormat tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_key_format_display() {
        assert_eq!(KeyFormat::PasetoV4Local.to_string(), "paseto_v4_local");
        assert_eq!(KeyFormat::PasetoV4Public.to_string(), "paseto_v4_public");
        assert_eq!(KeyFormat::JwtRs256.to_string(), "jwt_rs256");
        assert_eq!(KeyFormat::JwtEs256.to_string(), "jwt_es256");
    }

    #[test]
    fn test_key_format_from_str_valid() {
        assert_eq!(
            KeyFormat::from_str("paseto_v4_local").unwrap(),
            KeyFormat::PasetoV4Local
        );
        assert_eq!(
            KeyFormat::from_str("paseto_v4_public").unwrap(),
            KeyFormat::PasetoV4Public
        );
        assert_eq!(
            KeyFormat::from_str("jwt_rs256").unwrap(),
            KeyFormat::JwtRs256
        );
        assert_eq!(
            KeyFormat::from_str("jwt_es256").unwrap(),
            KeyFormat::JwtEs256
        );
    }

    #[test]
    fn test_key_format_from_str_invalid() {
        let err = KeyFormat::from_str("unknown").unwrap_err();
        assert_eq!(err.input, "unknown");
        assert!(err.to_string().contains("invalid key format"));
        assert!(err.to_string().contains("unknown"));
    }

    #[test]
    fn test_key_format_roundtrip() {
        let formats = [
            KeyFormat::PasetoV4Local,
            KeyFormat::PasetoV4Public,
            KeyFormat::JwtRs256,
            KeyFormat::JwtEs256,
        ];
        for format in formats {
            let s = format.to_string();
            let parsed = KeyFormat::from_str(&s).unwrap();
            assert_eq!(format, parsed);
        }
    }

    #[test]
    fn test_key_format_serde_roundtrip() {
        let format = KeyFormat::PasetoV4Local;
        let json = serde_json::to_string(&format).expect("serialize");
        let deserialized: KeyFormat = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(format, deserialized);
    }

    // -----------------------------------------------------------------------
    // KeyStatus tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_key_status_display() {
        assert_eq!(KeyStatus::Active.to_string(), "active");
        assert_eq!(KeyStatus::Draining.to_string(), "draining");
        assert_eq!(KeyStatus::Retired.to_string(), "retired");
    }

    #[test]
    fn test_key_status_from_str_valid() {
        assert_eq!(KeyStatus::from_str("active").unwrap(), KeyStatus::Active);
        assert_eq!(
            KeyStatus::from_str("draining").unwrap(),
            KeyStatus::Draining
        );
        assert_eq!(KeyStatus::from_str("retired").unwrap(), KeyStatus::Retired);
    }

    #[test]
    fn test_key_status_from_str_invalid() {
        let err = KeyStatus::from_str("pending").unwrap_err();
        assert_eq!(err.input, "pending");
        assert!(err.to_string().contains("invalid key status"));
    }

    #[test]
    fn test_key_status_roundtrip() {
        let statuses = [KeyStatus::Active, KeyStatus::Draining, KeyStatus::Retired];
        for status in statuses {
            let s = status.to_string();
            let parsed = KeyStatus::from_str(&s).unwrap();
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn test_key_status_serde_roundtrip() {
        let status = KeyStatus::Draining;
        let json = serde_json::to_string(&status).expect("serialize");
        let deserialized: KeyStatus = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(status, deserialized);
    }

    // -----------------------------------------------------------------------
    // SigningKeyMetadata tests
    // -----------------------------------------------------------------------

    fn sample_key() -> SigningKeyMetadata {
        SigningKeyMetadata {
            kid: "sigkey_test123".to_string(),
            format: KeyFormat::PasetoV4Local,
            key_material: "dGVzdGtleW1hdGVyaWFs".to_string(),
            status: KeyStatus::Active,
            created_at: Utc::now(),
            activated_at: Some(Utc::now()),
            draining_since: None,
            retired_at: None,
            drain_expires_at: None,
            service_name: "test-service".to_string(),
            key_hash: "abc123hash".to_string(),
        }
    }

    #[test]
    fn test_signing_key_metadata_equality_by_kid() {
        let key1 = sample_key();
        let mut key2 = sample_key();
        key2.status = KeyStatus::Draining; // different status, same kid
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_signing_key_metadata_inequality_by_kid() {
        let key1 = sample_key();
        let mut key2 = sample_key();
        key2.kid = "sigkey_other456".to_string();
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_signing_key_metadata_serde_roundtrip() {
        let key = sample_key();
        let json = serde_json::to_string(&key).expect("serialize");
        let deserialized: SigningKeyMetadata =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(key.kid, deserialized.kid);
        assert_eq!(key.format, deserialized.format);
        assert_eq!(key.status, deserialized.status);
        assert_eq!(key.key_material, deserialized.key_material);
        assert_eq!(key.service_name, deserialized.service_name);
        assert_eq!(key.key_hash, deserialized.key_hash);
    }

    #[test]
    fn test_parse_key_format_error_is_std_error() {
        let err = ParseKeyFormatError {
            input: "bad".to_string(),
        };
        let _: &dyn std::error::Error = &err;
    }

    #[test]
    fn test_parse_key_status_error_is_std_error() {
        let err = ParseKeyStatusError {
            input: "bad".to_string(),
        };
        let _: &dyn std::error::Error = &err;
    }
}
