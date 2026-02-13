//! Lockout notification hooks
//!
//! Provides a trait for receiving lockout lifecycle events (failed attempts,
//! threshold warnings, account locks/unlocks). Notifications are dispatched
//! via `tokio::spawn` so they never block login responses.

use async_trait::async_trait;

/// Events emitted during the lockout lifecycle
///
/// Dispatched to [`LockoutNotification`] handlers via fire-and-forget
/// `tokio::spawn`, so handlers should be lightweight and non-blocking.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum LockoutEvent {
    /// A login attempt failed
    FailedAttempt {
        /// The identity (email, username, etc.) that failed
        identity: String,
        /// Current number of failed attempts in the window
        attempt_count: u32,
        /// Maximum attempts before lockout
        max_attempts: u32,
    },
    /// The warning threshold has been reached
    ApproachingThreshold {
        /// The identity approaching lockout
        identity: String,
        /// Current number of failed attempts
        attempt_count: u32,
        /// Remaining attempts before lockout
        remaining_attempts: u32,
    },
    /// Account has been locked due to too many failures
    AccountLocked {
        /// The identity that was locked
        identity: String,
        /// Number of failed attempts that triggered the lock
        attempt_count: u32,
        /// How long the account is locked (seconds)
        lockout_duration_secs: u64,
    },
    /// Account has been unlocked
    AccountUnlocked {
        /// The identity that was unlocked
        identity: String,
        /// Why the account was unlocked
        reason: UnlockReason,
    },
}

/// Reason an account was unlocked
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnlockReason {
    /// Lockout duration expired naturally
    Expired,
    /// A successful login cleared the lockout
    SuccessfulLogin,
    /// An administrator manually unlocked the account
    AdminAction,
}

impl std::fmt::Display for UnlockReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Expired => write!(f, "expired"),
            Self::SuccessfulLogin => write!(f, "successful_login"),
            Self::AdminAction => write!(f, "admin_action"),
        }
    }
}

/// Trait for receiving lockout lifecycle notifications
///
/// Implement this trait to react to lockout events (e.g., send emails,
/// emit metrics, write audit logs). Handlers are invoked asynchronously
/// and must not panic.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::lockout::{LockoutNotification, LockoutEvent};
///
/// struct EmailNotifier { /* ... */ }
///
/// #[async_trait]
/// impl LockoutNotification for EmailNotifier {
///     async fn on_event(&self, event: LockoutEvent) {
///         if let LockoutEvent::AccountLocked { identity, .. } = event {
///             // send_lockout_email(&identity).await;
///         }
///     }
/// }
/// ```
#[async_trait]
pub trait LockoutNotification: Send + Sync + 'static {
    /// Called when a lockout lifecycle event occurs
    ///
    /// This method is invoked inside `tokio::spawn`, so it will not
    /// block the login response. Implementations should handle their
    /// own errors internally (log and continue).
    async fn on_event(&self, event: LockoutEvent);
}
