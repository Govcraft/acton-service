//! Account-specific error types

use super::types::AccountStatus;

/// Errors specific to account management operations
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AccountError {
    /// Account not found by ID or identifier
    #[error("account not found: {0}")]
    NotFound(String),

    /// Account already exists with the given email
    #[error("account already exists with email: {0}")]
    AlreadyExists(String),

    /// Invalid status transition attempted
    #[error("invalid status transition from {from} to {to}")]
    InvalidTransition {
        /// Current status
        from: AccountStatus,
        /// Attempted target status
        to: AccountStatus,
    },

    /// Invalid credentials (wrong password)
    #[error("invalid credentials")]
    InvalidCredentials,

    /// Account is not in an active state
    #[error("account is {status}: {reason}")]
    AccountInactive {
        /// Current account status
        status: AccountStatus,
        /// Reason the account is inactive
        reason: String,
    },

    /// Validation error (bad input)
    #[error("validation error: {0}")]
    Validation(String),

    /// Storage backend error
    #[error("storage error: {0}")]
    Storage(String),

    /// Invalid account ID format
    #[error("invalid account ID: {0}")]
    InvalidId(String),
}
