//! Account types for NIST AC-2 lifecycle management
//!
//! Core entity types including `AccountId` (TypeID with `acct` prefix),
//! `Account` entity, `AccountStatus` lifecycle states, and DTOs.

use chrono::{DateTime, Utc};
use mti::prelude::*;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

// ============================================================================
// AccountId (TypeID newtype)
// ============================================================================

/// A type-safe account identifier using UUIDv7.
///
/// Uses the TypeID format: `acct_<base32-encoded-uuidv7>`
///
/// UUIDv7 provides time-sortability, making account IDs naturally ordered
/// by creation time.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccountId(MagicTypeId);

impl AccountId {
    /// The prefix used for account IDs
    pub const PREFIX: &'static str = "acct";

    /// Creates a new account ID with a UUIDv7 (time-sortable).
    #[must_use]
    pub fn new() -> Self {
        Self(Self::PREFIX.create_type_id::<V7>())
    }

    /// Returns the account ID as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Returns the prefix portion of the ID.
    #[must_use]
    pub fn prefix(&self) -> &str {
        self.0.prefix().as_str()
    }
}

impl Default for AccountId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for AccountId {
    type Err = AccountIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mti = MagicTypeId::from_str(s).map_err(AccountIdError::Parse)?;

        if mti.prefix().as_str() != Self::PREFIX {
            return Err(AccountIdError::InvalidPrefix {
                expected: Self::PREFIX.to_string(),
                actual: mti.prefix().as_str().to_string(),
            });
        }

        Ok(Self(mti))
    }
}

impl AsRef<str> for AccountId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<AccountId> for String {
    fn from(id: AccountId) -> Self {
        id.0.to_string()
    }
}

impl Serialize for AccountId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AccountId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        AccountId::from_str(&s).map_err(serde::de::Error::custom)
    }
}

/// Error type for account ID parsing.
#[derive(Debug, thiserror::Error)]
pub enum AccountIdError {
    /// The ID could not be parsed as a valid TypeID.
    #[error("failed to parse account ID: {0}")]
    Parse(#[from] MagicTypeIdError),

    /// The prefix was not the expected value.
    #[error("invalid prefix: expected '{expected}', got '{actual}'")]
    InvalidPrefix {
        /// The expected prefix.
        expected: String,
        /// The actual prefix found.
        actual: String,
    },
}

// ============================================================================
// AccountStatus (NIST AC-2 lifecycle states)
// ============================================================================

/// NIST AC-2 account lifecycle states
///
/// Represents the complete lifecycle of an account from provisioning
/// through deprovisioning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    /// Created, awaiting email verification
    PendingVerification,
    /// Normal operational state
    Active,
    /// Administratively disabled (AC-2(3))
    Disabled,
    /// Locked due to security events (AC-7 integration)
    Locked,
    /// Past expiration date (AC-2(3))
    Expired,
    /// Temporary suspension (policy violation, etc.)
    Suspended,
}

impl AccountStatus {
    /// Returns whether a transition from self to target is valid
    pub fn can_transition_to(&self, target: AccountStatus) -> bool {
        matches!(
            (self, target),
            // From PendingVerification
            (AccountStatus::PendingVerification, AccountStatus::Active)
                | (AccountStatus::PendingVerification, AccountStatus::Disabled)
                | (AccountStatus::PendingVerification, AccountStatus::Expired)
                // From Active
                | (AccountStatus::Active, AccountStatus::Disabled)
                | (AccountStatus::Active, AccountStatus::Locked)
                | (AccountStatus::Active, AccountStatus::Expired)
                | (AccountStatus::Active, AccountStatus::Suspended)
                // From Disabled
                | (AccountStatus::Disabled, AccountStatus::Active)
                // From Locked
                | (AccountStatus::Locked, AccountStatus::Active)
                | (AccountStatus::Locked, AccountStatus::Disabled)
                // From Expired
                | (AccountStatus::Expired, AccountStatus::Active)
                | (AccountStatus::Expired, AccountStatus::Disabled)
                // From Suspended
                | (AccountStatus::Suspended, AccountStatus::Active)
                | (AccountStatus::Suspended, AccountStatus::Disabled)
        )
    }
}

impl fmt::Display for AccountStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PendingVerification => write!(f, "pending_verification"),
            Self::Active => write!(f, "active"),
            Self::Disabled => write!(f, "disabled"),
            Self::Locked => write!(f, "locked"),
            Self::Expired => write!(f, "expired"),
            Self::Suspended => write!(f, "suspended"),
        }
    }
}

impl FromStr for AccountStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending_verification" => Ok(Self::PendingVerification),
            "active" => Ok(Self::Active),
            "disabled" => Ok(Self::Disabled),
            "locked" => Ok(Self::Locked),
            "expired" => Ok(Self::Expired),
            "suspended" => Ok(Self::Suspended),
            other => Err(format!("unknown account status: {}", other)),
        }
    }
}

// ============================================================================
// Account Entity
// ============================================================================

/// Account entity with full NIST AC-2 lifecycle tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Account {
    /// Unique account identifier
    pub id: AccountId,
    /// Email address (lowercase-normalized)
    pub email: String,
    /// Optional username
    pub username: Option<String>,
    /// Argon2id password hash (None for OAuth-only accounts)
    pub password_hash: Option<String>,
    /// Current lifecycle status
    pub status: AccountStatus,
    /// Assigned roles
    pub roles: Vec<String>,
    /// Whether email has been verified
    pub email_verified: bool,
    /// When email was verified
    pub email_verified_at: Option<DateTime<Utc>>,
    /// Last successful login
    pub last_login_at: Option<DateTime<Utc>>,
    /// When account was locked
    pub locked_at: Option<DateTime<Utc>>,
    /// Reason for lock
    pub locked_reason: Option<String>,
    /// When account was disabled
    pub disabled_at: Option<DateTime<Utc>>,
    /// Reason for disabling
    pub disabled_reason: Option<String>,
    /// Account expiration date
    pub expires_at: Option<DateTime<Utc>>,
    /// When password was last changed
    pub password_changed_at: Option<DateTime<Utc>>,
    /// Persistent failed login counter (survives Redis flushes)
    pub failed_login_count: u32,
    /// Additional structured metadata
    pub metadata: Option<serde_json::Value>,
    /// When the account was created
    pub created_at: DateTime<Utc>,
    /// When the account was last updated
    pub updated_at: DateTime<Utc>,
}

// ============================================================================
// DTOs
// ============================================================================

/// Request to create a new account
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CreateAccount {
    /// Email address
    pub email: String,
    /// Optional username
    pub username: Option<String>,
    /// Plaintext password (will be hashed by AccountService)
    pub password: Option<String>,
    /// Roles to assign
    #[serde(default)]
    pub roles: Vec<String>,
    /// Account expiration date
    pub expires_at: Option<DateTime<Utc>>,
    /// Additional metadata
    pub metadata: Option<serde_json::Value>,
    /// Override config default for email verification requirement
    pub require_email_verification: Option<bool>,
}

/// Request to update an existing account
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[non_exhaustive]
pub struct UpdateAccount {
    /// New email address
    pub email: Option<String>,
    /// New username
    pub username: Option<String>,
    /// New roles (replaces existing)
    pub roles: Option<Vec<String>>,
    /// New expiration (Some(None) clears expiration)
    pub expires_at: Option<Option<DateTime<Utc>>>,
    /// New metadata
    pub metadata: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_id_new() {
        let id = AccountId::new();
        assert!(id.as_str().starts_with("acct_"));
        assert_eq!(id.prefix(), "acct");
    }

    #[test]
    fn test_account_id_display_fromstr_roundtrip() {
        let id = AccountId::new();
        let s = id.to_string();
        let parsed = AccountId::from_str(&s).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn test_account_id_invalid_prefix() {
        let result = AccountId::from_str("req_01h455vb4pex5vsknk084sn02q");
        assert!(result.is_err());
        match result.unwrap_err() {
            AccountIdError::InvalidPrefix { expected, actual } => {
                assert_eq!(expected, "acct");
                assert_eq!(actual, "req");
            }
            _ => panic!("Expected InvalidPrefix error"),
        }
    }

    #[test]
    fn test_account_id_invalid_format() {
        let result = AccountId::from_str("acct_invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_account_id_serde_roundtrip() {
        let id = AccountId::new();
        let json = serde_json::to_string(&id).unwrap();
        let deserialized: AccountId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, deserialized);
    }

    #[test]
    fn test_account_id_ordering() {
        let id1 = AccountId::new();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let id2 = AccountId::new();
        assert!(id1 < id2);
    }

    #[test]
    fn test_account_status_display_fromstr_roundtrip() {
        let statuses = [
            AccountStatus::PendingVerification,
            AccountStatus::Active,
            AccountStatus::Disabled,
            AccountStatus::Locked,
            AccountStatus::Expired,
            AccountStatus::Suspended,
        ];
        for status in statuses {
            let s = status.to_string();
            let parsed = AccountStatus::from_str(&s).unwrap();
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn test_status_valid_transitions() {
        use AccountStatus::*;
        let valid = [
            (PendingVerification, Active),
            (PendingVerification, Disabled),
            (PendingVerification, Expired),
            (Active, Disabled),
            (Active, Locked),
            (Active, Expired),
            (Active, Suspended),
            (Disabled, Active),
            (Locked, Active),
            (Locked, Disabled),
            (Expired, Active),
            (Expired, Disabled),
            (Suspended, Active),
            (Suspended, Disabled),
        ];
        for (from, to) in valid {
            assert!(
                from.can_transition_to(to),
                "{} -> {} should be valid",
                from,
                to
            );
        }
    }

    #[test]
    fn test_status_invalid_transitions() {
        use AccountStatus::*;
        let invalid = [
            (PendingVerification, Locked),
            (PendingVerification, Suspended),
            (Active, PendingVerification),
            (Active, Active),
            (Disabled, Disabled),
            (Disabled, Locked),
            (Disabled, Expired),
            (Disabled, Suspended),
            (Locked, Locked),
            (Locked, Expired),
            (Locked, Suspended),
            (Expired, Expired),
            (Expired, Locked),
            (Expired, Suspended),
            (Suspended, Suspended),
            (Suspended, Locked),
            (Suspended, Expired),
        ];
        for (from, to) in invalid {
            assert!(
                !from.can_transition_to(to),
                "{} -> {} should be invalid",
                from,
                to
            );
        }
    }

    #[test]
    fn test_account_status_serde() {
        let status = AccountStatus::PendingVerification;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"pending_verification\"");
        let deserialized: AccountStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, deserialized);
    }
}
