//! Account lifecycle notification hooks
//!
//! Provides a trait for receiving account lifecycle events (creation,
//! status changes, email verification, etc.). Notifications are dispatched
//! via `tokio::spawn` so they never block account operations.

use async_trait::async_trait;

/// Events emitted during the account lifecycle
///
/// Dispatched to [`AccountNotification`] handlers via fire-and-forget
/// `tokio::spawn`, so handlers should be lightweight and non-blocking.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum AccountEvent {
    /// A new account was created
    Created {
        /// The new account's ID
        account_id: String,
        /// The account's email
        email: String,
    },
    /// Account was activated (email verified or admin action)
    Activated {
        /// The account ID
        account_id: String,
    },
    /// Account was administratively disabled
    Disabled {
        /// The account ID
        account_id: String,
        /// Reason for disabling
        reason: String,
    },
    /// Account was locked due to security events
    Locked {
        /// The account ID
        account_id: String,
        /// Reason for locking
        reason: String,
    },
    /// Account was unlocked
    Unlocked {
        /// The account ID
        account_id: String,
    },
    /// Account was suspended
    Suspended {
        /// The account ID
        account_id: String,
        /// Reason for suspension
        reason: String,
    },
    /// Account expired
    Expired {
        /// The account ID
        account_id: String,
    },
    /// Account was deleted
    Deleted {
        /// The account ID
        account_id: String,
    },
    /// Email was verified
    EmailVerified {
        /// The account ID
        account_id: String,
    },
    /// Password was changed
    PasswordChanged {
        /// The account ID
        account_id: String,
    },
    /// Roles were updated
    RolesUpdated {
        /// The account ID
        account_id: String,
        /// The new set of roles
        roles: Vec<String>,
    },
    /// Profile (email, username, metadata) was updated
    ProfileUpdated {
        /// The account ID
        account_id: String,
    },
}

/// Trait for receiving account lifecycle notifications
///
/// Implement this trait to react to account events (e.g., send emails,
/// emit metrics, write audit logs). Handlers are invoked asynchronously
/// and must not panic.
#[async_trait]
pub trait AccountNotification: Send + Sync + 'static {
    /// Called when an account lifecycle event occurs
    ///
    /// This method is invoked inside `tokio::spawn`, so it will not
    /// block the account operation. Implementations should handle their
    /// own errors internally (log and continue).
    async fn on_event(&self, event: AccountEvent);
}
