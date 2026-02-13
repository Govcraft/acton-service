//! Login lockout with progressive delay and account lockout
//!
//! Provides brute force protection for login endpoints by tracking failed
//! attempts per identity in Redis. Supports configurable progressive delays,
//! threshold-based account lockout, and notification hooks.
//!
//! # Feature Dependencies
//!
//! Requires: `login-lockout` (which enables `auth` + `cache`)
//!
//! # Architecture
//!
//! - **Service approach**: Construct [`LoginLockout`] once, pass via `State`
//! - **Middleware approach**: Use [`LockoutMiddleware`] for automatic enforcement
//! - **Notifications**: Register [`LockoutNotification`] handlers for events
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use acton_service::lockout::{LoginLockout, LockoutConfig};
//!
//! let lockout = LoginLockout::new(lockout_config, redis_pool);
//!
//! // In your login handler:
//! let status = lockout.check(&email).await?;
//! if status.locked { /* return 423 */ }
//!
//! match authenticate(&creds).await {
//!     Ok(tokens) => { lockout.record_success(&email).await?; Ok(tokens) }
//!     Err(_) => {
//!         let status = lockout.record_failure(&email).await?;
//!         if status.delay_ms > 0 {
//!             tokio::time::sleep(Duration::from_millis(status.delay_ms)).await;
//!         }
//!         Err(Error::Unauthorized("Invalid credentials".into()))
//!     }
//! }
//! ```

pub mod config;
pub mod middleware;
pub mod notification;
pub mod service;

pub use config::LockoutConfig;
pub use middleware::LockoutMiddleware;
pub use notification::{LockoutEvent, LockoutNotification, UnlockReason};
pub use service::{LockoutStatus, LoginLockout};

// Audit integration: when both `login-lockout` and `audit` are active,
// provide a built-in notification handler that emits audit events.
#[cfg(feature = "audit")]
mod audit_integration {
    use async_trait::async_trait;

    use crate::audit::{AuditEvent, AuditEventKind, AuditLogger, AuditSeverity};

    use super::notification::{LockoutEvent, LockoutNotification};

    /// Notification handler that emits lockout events to the audit log
    ///
    /// Register via [`LoginLockout::with_audit`](super::LoginLockout::with_audit).
    pub struct AuditLockoutNotification {
        audit_logger: AuditLogger,
    }

    impl AuditLockoutNotification {
        /// Create a new audit lockout notification handler
        pub fn new(audit_logger: AuditLogger) -> Self {
            Self { audit_logger }
        }
    }

    #[async_trait]
    impl LockoutNotification for AuditLockoutNotification {
        async fn on_event(&self, event: LockoutEvent) {
            match event {
                LockoutEvent::AccountLocked {
                    ref identity,
                    attempt_count,
                    lockout_duration_secs,
                } => {
                    let audit_event = AuditEvent::new(
                        AuditEventKind::AuthAccountLocked,
                        AuditSeverity::Warning,
                        self.audit_logger.service_name().to_string(),
                    )
                    .with_metadata(serde_json::json!({
                        "identity": identity,
                        "attempt_count": attempt_count,
                        "lockout_duration_secs": lockout_duration_secs,
                    }));
                    self.audit_logger.log(audit_event).await;
                }
                LockoutEvent::AccountUnlocked {
                    ref identity,
                    ref reason,
                } => {
                    let audit_event = AuditEvent::new(
                        AuditEventKind::AuthAccountUnlocked,
                        AuditSeverity::Notice,
                        self.audit_logger.service_name().to_string(),
                    )
                    .with_metadata(serde_json::json!({
                        "identity": identity,
                        "reason": reason.to_string(),
                    }));
                    self.audit_logger.log(audit_event).await;
                }
                // Other events are not audit-worthy
                _ => {}
            }
        }
    }
}

#[cfg(feature = "audit")]
pub use audit_integration::AuditLockoutNotification;
