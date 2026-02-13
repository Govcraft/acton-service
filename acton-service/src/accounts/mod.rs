//! Account lifecycle management (NIST SP 800-53 AC-2)
//!
//! Provides account CRUD, lifecycle state management, email verification,
//! password management, and notification hooks for provisioning/deprovisioning.
//!
//! # Feature Dependencies
//!
//! Requires: `accounts` (which enables `auth`)
//!
//! # Architecture
//!
//! - **AccountService**: Central orchestrator for all account operations
//! - **AccountStorage**: Pluggable persistence (PostgreSQL, Turso, SurrealDB)
//! - **AccountNotification**: Event hooks for lifecycle changes
//! - **Audit integration**: Bridges account events to the audit log (when `audit` active)

pub mod config;
pub mod error;
#[cfg(feature = "account-handlers")]
pub mod handlers;
pub mod notification;
pub mod storage;
pub mod types;

pub use config::AccountsConfig;
pub use error::AccountError;
pub use notification::{AccountEvent, AccountNotification};
pub use storage::AccountStorage;
pub use types::{Account, AccountId, AccountIdError, AccountStatus, CreateAccount, UpdateAccount};

use chrono::Utc;
use std::sync::Arc;

use crate::auth::PasswordHasher;
use crate::error::Error;

/// Central service for account lifecycle management
///
/// Orchestrates storage, password hashing, validation, and notification
/// dispatch for all account operations.
#[derive(Clone)]
pub struct AccountService {
    storage: Arc<dyn AccountStorage>,
    hasher: PasswordHasher,
    config: AccountsConfig,
    notifications: Vec<Arc<dyn AccountNotification>>,
}

impl AccountService {
    /// Create a new account service
    pub fn new(
        storage: Arc<dyn AccountStorage>,
        hasher: PasswordHasher,
        config: AccountsConfig,
    ) -> Self {
        Self {
            storage,
            hasher,
            config,
            notifications: Vec::new(),
        }
    }

    /// Register a notification handler
    pub fn with_notification(mut self, handler: Arc<dyn AccountNotification>) -> Self {
        self.notifications.push(handler);
        self
    }

    /// Register the audit notification bridge (when `audit` feature is active)
    #[cfg(feature = "audit")]
    pub fn with_audit(self, audit_logger: crate::audit::AuditLogger) -> Self {
        let handler = Arc::new(AuditAccountNotification::new(audit_logger));
        self.with_notification(handler)
    }

    /// Create a new account
    ///
    /// - Validates email format and normalizes to lowercase
    /// - Checks for duplicate email
    /// - Hashes password (if provided)
    /// - Sets initial status per config
    pub async fn create_account(&self, data: CreateAccount) -> Result<Account, AccountError> {
        // Validate email format
        let email = data.email.trim().to_lowercase();
        if !is_valid_email(&email) {
            return Err(AccountError::Validation(format!(
                "invalid email format: {}",
                email
            )));
        }

        // Check for duplicate email
        if self
            .storage
            .get_by_email(&email)
            .await
            .map_err(|e| AccountError::Storage(e.to_string()))?
            .is_some()
        {
            return Err(AccountError::AlreadyExists(email));
        }

        // Check for duplicate username if configured
        if self.config.unique_usernames {
            if let Some(ref username) = data.username {
                if self
                    .storage
                    .get_by_username(username)
                    .await
                    .map_err(|e| AccountError::Storage(e.to_string()))?
                    .is_some()
                {
                    return Err(AccountError::AlreadyExists(format!(
                        "username: {}",
                        username
                    )));
                }
            }
        }

        // Hash password
        let password_hash = if let Some(ref password) = data.password {
            Some(
                self.hasher
                    .hash(password)
                    .map_err(|e| AccountError::Validation(e.to_string()))?,
            )
        } else {
            None
        };

        // Determine initial status
        let require_verification = data
            .require_email_verification
            .unwrap_or(self.config.require_email_verification);

        let status = if require_verification {
            AccountStatus::PendingVerification
        } else {
            AccountStatus::Active
        };

        let now = Utc::now();
        let account = Account {
            id: AccountId::new(),
            email: email.clone(),
            username: data.username,
            password_hash,
            status,
            roles: data.roles,
            email_verified: !require_verification,
            email_verified_at: if !require_verification {
                Some(now)
            } else {
                None
            },
            last_login_at: None,
            locked_at: None,
            locked_reason: None,
            disabled_at: None,
            disabled_reason: None,
            expires_at: data.expires_at,
            password_changed_at: if data.password.is_some() {
                Some(now)
            } else {
                None
            },
            failed_login_count: 0,
            metadata: data.metadata,
            created_at: now,
            updated_at: now,
        };

        self.storage
            .create(&account)
            .await
            .map_err(|e| AccountError::Storage(e.to_string()))?;

        self.notify(AccountEvent::Created {
            account_id: account.id.to_string(),
            email: account.email.clone(),
        });

        Ok(account)
    }

    /// Get an account by ID
    ///
    /// Checks expiration and auto-transitions to Expired if past `expires_at`.
    pub async fn get_account(&self, id: &str) -> Result<Option<Account>, AccountError> {
        let account = self
            .storage
            .get_by_id(id)
            .await
            .map_err(|e| AccountError::Storage(e.to_string()))?;

        if let Some(mut acct) = account {
            // Auto-expire if past expiration
            if acct.status != AccountStatus::Expired {
                if let Some(expires_at) = acct.expires_at {
                    if expires_at < Utc::now() {
                        let _ = self
                            .storage
                            .update_status(id, AccountStatus::Expired, None)
                            .await;
                        acct.status = AccountStatus::Expired;

                        self.notify(AccountEvent::Expired {
                            account_id: id.to_string(),
                        });
                    }
                }
            }
            Ok(Some(acct))
        } else {
            Ok(None)
        }
    }

    /// Get an account by email (for login flow)
    pub async fn get_account_by_email(
        &self,
        email: &str,
    ) -> Result<Option<Account>, AccountError> {
        self.storage
            .get_by_email(email)
            .await
            .map_err(|e| AccountError::Storage(e.to_string()))
    }

    /// Update account profile fields
    pub async fn update_account(
        &self,
        id: &str,
        data: UpdateAccount,
    ) -> Result<Account, AccountError> {
        let mut account = self
            .storage
            .get_by_id(id)
            .await
            .map_err(|e| AccountError::Storage(e.to_string()))?
            .ok_or_else(|| AccountError::NotFound(id.to_string()))?;

        if let Some(email) = data.email {
            let email = email.trim().to_lowercase();
            if !is_valid_email(&email) {
                return Err(AccountError::Validation(format!(
                    "invalid email format: {}",
                    email
                )));
            }
            // Check for duplicate
            if let Some(existing) = self
                .storage
                .get_by_email(&email)
                .await
                .map_err(|e| AccountError::Storage(e.to_string()))?
            {
                if existing.id != account.id {
                    return Err(AccountError::AlreadyExists(email));
                }
            }
            account.email = email;
        }

        if let Some(username) = data.username {
            account.username = Some(username);
        }

        if let Some(roles) = data.roles {
            let old_roles = account.roles.clone();
            account.roles = roles.clone();
            if old_roles != roles {
                self.notify(AccountEvent::RolesUpdated {
                    account_id: id.to_string(),
                    roles,
                });
            }
        }

        if let Some(expires_at) = data.expires_at {
            account.expires_at = expires_at;
        }

        if let Some(metadata) = data.metadata {
            account.metadata = Some(metadata);
        }

        account.updated_at = Utc::now();

        self.storage
            .update(&account)
            .await
            .map_err(|e| AccountError::Storage(e.to_string()))?;

        self.notify(AccountEvent::ProfileUpdated {
            account_id: id.to_string(),
        });

        Ok(account)
    }

    /// Disable an account (admin action)
    pub async fn disable_account(&self, id: &str, reason: &str) -> Result<(), AccountError> {
        let account = self.require_account(id).await?;
        self.validate_transition(&account, AccountStatus::Disabled)?;

        self.storage
            .update_status(id, AccountStatus::Disabled, Some(reason))
            .await
            .map_err(|e| AccountError::Storage(e.to_string()))?;

        self.notify(AccountEvent::Disabled {
            account_id: id.to_string(),
            reason: reason.to_string(),
        });

        Ok(())
    }

    /// Enable (re-activate) an account
    pub async fn enable_account(&self, id: &str) -> Result<(), AccountError> {
        let account = self.require_account(id).await?;
        self.validate_transition(&account, AccountStatus::Active)?;

        self.storage
            .update_status(id, AccountStatus::Active, None)
            .await
            .map_err(|e| AccountError::Storage(e.to_string()))?;

        self.notify(AccountEvent::Activated {
            account_id: id.to_string(),
        });

        Ok(())
    }

    /// Lock an account (security event)
    pub async fn lock_account(&self, id: &str, reason: &str) -> Result<(), AccountError> {
        let account = self.require_account(id).await?;
        self.validate_transition(&account, AccountStatus::Locked)?;

        self.storage
            .update_status(id, AccountStatus::Locked, Some(reason))
            .await
            .map_err(|e| AccountError::Storage(e.to_string()))?;

        self.notify(AccountEvent::Locked {
            account_id: id.to_string(),
            reason: reason.to_string(),
        });

        Ok(())
    }

    /// Unlock an account
    pub async fn unlock_account(&self, id: &str) -> Result<(), AccountError> {
        let account = self.require_account(id).await?;
        self.validate_transition(&account, AccountStatus::Active)?;

        self.storage
            .update_status(id, AccountStatus::Active, None)
            .await
            .map_err(|e| AccountError::Storage(e.to_string()))?;

        self.notify(AccountEvent::Unlocked {
            account_id: id.to_string(),
        });

        Ok(())
    }

    /// Suspend an account
    pub async fn suspend_account(&self, id: &str, reason: &str) -> Result<(), AccountError> {
        let account = self.require_account(id).await?;
        self.validate_transition(&account, AccountStatus::Suspended)?;

        self.storage
            .update_status(id, AccountStatus::Suspended, Some(reason))
            .await
            .map_err(|e| AccountError::Storage(e.to_string()))?;

        self.notify(AccountEvent::Suspended {
            account_id: id.to_string(),
            reason: reason.to_string(),
        });

        Ok(())
    }

    /// Verify an account's email address
    pub async fn verify_email(&self, id: &str) -> Result<(), AccountError> {
        let mut account = self.require_account(id).await?;

        if account.email_verified {
            return Ok(()); // Already verified, idempotent
        }

        account.email_verified = true;
        account.email_verified_at = Some(Utc::now());
        account.updated_at = Utc::now();

        // Transition from PendingVerification to Active
        if account.status == AccountStatus::PendingVerification {
            account.status = AccountStatus::Active;
        }

        self.storage
            .update(&account)
            .await
            .map_err(|e| AccountError::Storage(e.to_string()))?;

        self.notify(AccountEvent::EmailVerified {
            account_id: id.to_string(),
        });

        if account.status == AccountStatus::Active {
            self.notify(AccountEvent::Activated {
                account_id: id.to_string(),
            });
        }

        Ok(())
    }

    /// Change an account's password
    pub async fn change_password(&self, id: &str, new_password: &str) -> Result<(), AccountError> {
        let mut account = self.require_account(id).await?;

        let hash = self
            .hasher
            .hash(new_password)
            .map_err(|e| AccountError::Validation(e.to_string()))?;

        account.password_hash = Some(hash);
        account.password_changed_at = Some(Utc::now());
        account.updated_at = Utc::now();

        self.storage
            .update(&account)
            .await
            .map_err(|e| AccountError::Storage(e.to_string()))?;

        self.notify(AccountEvent::PasswordChanged {
            account_id: id.to_string(),
        });

        Ok(())
    }

    /// Hard delete an account (GDPR)
    pub async fn delete_account(&self, id: &str) -> Result<(), AccountError> {
        let deleted = self
            .storage
            .delete(id)
            .await
            .map_err(|e| AccountError::Storage(e.to_string()))?;

        if !deleted {
            return Err(AccountError::NotFound(id.to_string()));
        }

        self.notify(AccountEvent::Deleted {
            account_id: id.to_string(),
        });

        Ok(())
    }

    /// List accounts with optional status filter and pagination
    pub async fn list_accounts(
        &self,
        status: Option<AccountStatus>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Account>, AccountError> {
        self.storage
            .list(status, limit, offset)
            .await
            .map_err(|e| AccountError::Storage(e.to_string()))
    }

    /// Count accounts with optional status filter
    pub async fn count_accounts(
        &self,
        status: Option<AccountStatus>,
    ) -> Result<u64, AccountError> {
        self.storage
            .count(status)
            .await
            .map_err(|e| AccountError::Storage(e.to_string()))
    }

    /// Authenticate with email and password
    ///
    /// Checks:
    /// 1. Account exists
    /// 2. Account is Active
    /// 3. Password matches
    /// 4. Updates `last_login_at`
    pub async fn authenticate(
        &self,
        email: &str,
        password: &str,
    ) -> Result<Account, AccountError> {
        let email = email.trim().to_lowercase();

        let account = self
            .storage
            .get_by_email(&email)
            .await
            .map_err(|e| AccountError::Storage(e.to_string()))?
            .ok_or_else(|| AccountError::NotFound(email.clone()))?;

        // Check status
        if account.status != AccountStatus::Active {
            let reason = match account.status {
                AccountStatus::PendingVerification => "email not verified".to_string(),
                AccountStatus::Disabled => account
                    .disabled_reason
                    .clone()
                    .unwrap_or_else(|| "administratively disabled".to_string()),
                AccountStatus::Locked => account
                    .locked_reason
                    .clone()
                    .unwrap_or_else(|| "account locked".to_string()),
                AccountStatus::Expired => "account expired".to_string(),
                AccountStatus::Suspended => "account suspended".to_string(),
                _ => "account not active".to_string(),
            };
            return Err(AccountError::AccountInactive {
                status: account.status,
                reason,
            });
        }

        // Check password
        let password_hash = account
            .password_hash
            .as_ref()
            .ok_or(AccountError::InvalidCredentials)?;

        let valid = self
            .hasher
            .verify(password, password_hash)
            .map_err(|e| AccountError::Storage(e.to_string()))?;

        if !valid {
            return Err(AccountError::InvalidCredentials);
        }

        // Record login
        let _ = self.storage.record_login(account.id.as_str()).await;

        Ok(account)
    }

    // ========================================================================
    // Internal helpers
    // ========================================================================

    async fn require_account(&self, id: &str) -> Result<Account, AccountError> {
        self.storage
            .get_by_id(id)
            .await
            .map_err(|e| AccountError::Storage(e.to_string()))?
            .ok_or_else(|| AccountError::NotFound(id.to_string()))
    }

    fn validate_transition(
        &self,
        account: &Account,
        target: AccountStatus,
    ) -> Result<(), AccountError> {
        if !account.status.can_transition_to(target) {
            return Err(AccountError::InvalidTransition {
                from: account.status,
                to: target,
            });
        }
        Ok(())
    }

    fn notify(&self, event: AccountEvent) {
        for handler in &self.notifications {
            let handler = handler.clone();
            let event = event.clone();
            tokio::spawn(async move {
                handler.on_event(event).await;
            });
        }
    }
}

/// Convert AccountError to the framework Error type
impl From<AccountError> for Error {
    fn from(err: AccountError) -> Self {
        match &err {
            AccountError::NotFound(_) => Error::NotFound(err.to_string()),
            AccountError::AlreadyExists(_) => Error::Conflict(err.to_string()),
            AccountError::InvalidTransition { .. } => Error::ValidationError(err.to_string()),
            AccountError::InvalidCredentials => Error::Unauthorized(err.to_string()),
            AccountError::AccountInactive { .. } => Error::Forbidden(err.to_string()),
            AccountError::Validation(_) => Error::BadRequest(err.to_string()),
            AccountError::Storage(_) => Error::Internal(err.to_string()),
            AccountError::InvalidId(_) => Error::BadRequest(err.to_string()),
        }
    }
}

/// Basic email format validation (lowercase, contains @, has domain)
fn is_valid_email(email: &str) -> bool {
    let parts: Vec<&str> = email.split('@').collect();
    if parts.len() != 2 {
        return false;
    }
    let local = parts[0];
    let domain = parts[1];
    !local.is_empty() && !domain.is_empty() && domain.contains('.')
}

// ============================================================================
// Audit integration bridge
// ============================================================================

#[cfg(feature = "audit")]
mod audit_integration {
    use async_trait::async_trait;

    use crate::audit::{AuditEvent, AuditEventKind, AuditLogger, AuditSeverity};

    use super::notification::{AccountEvent, AccountNotification};

    /// Notification handler that bridges account events to the audit log
    pub struct AuditAccountNotification {
        audit_logger: AuditLogger,
    }

    impl AuditAccountNotification {
        /// Create a new audit account notification handler
        pub fn new(audit_logger: AuditLogger) -> Self {
            Self { audit_logger }
        }
    }

    #[async_trait]
    impl AccountNotification for AuditAccountNotification {
        async fn on_event(&self, event: AccountEvent) {
            let (kind, severity, metadata) = match event {
                AccountEvent::Created {
                    ref account_id,
                    ref email,
                } => (
                    AuditEventKind::AccountCreated,
                    AuditSeverity::Informational,
                    serde_json::json!({
                        "account_id": account_id,
                        "email": email,
                    }),
                ),
                AccountEvent::Activated { ref account_id } => (
                    AuditEventKind::AccountEnabled,
                    AuditSeverity::Informational,
                    serde_json::json!({ "account_id": account_id }),
                ),
                AccountEvent::Disabled {
                    ref account_id,
                    ref reason,
                } => (
                    AuditEventKind::AccountDisabled,
                    AuditSeverity::Warning,
                    serde_json::json!({
                        "account_id": account_id,
                        "reason": reason,
                    }),
                ),
                AccountEvent::Locked {
                    ref account_id,
                    ref reason,
                } => (
                    AuditEventKind::AccountLocked,
                    AuditSeverity::Warning,
                    serde_json::json!({
                        "account_id": account_id,
                        "reason": reason,
                    }),
                ),
                AccountEvent::Unlocked { ref account_id } => (
                    AuditEventKind::AccountUnlocked,
                    AuditSeverity::Notice,
                    serde_json::json!({ "account_id": account_id }),
                ),
                AccountEvent::Suspended {
                    ref account_id,
                    ref reason,
                } => (
                    AuditEventKind::AccountDisabled,
                    AuditSeverity::Warning,
                    serde_json::json!({
                        "account_id": account_id,
                        "reason": reason,
                        "action": "suspended",
                    }),
                ),
                AccountEvent::Expired { ref account_id } => (
                    AuditEventKind::AccountExpired,
                    AuditSeverity::Notice,
                    serde_json::json!({ "account_id": account_id }),
                ),
                AccountEvent::Deleted { ref account_id } => (
                    AuditEventKind::AccountDeleted,
                    AuditSeverity::Warning,
                    serde_json::json!({ "account_id": account_id }),
                ),
                AccountEvent::EmailVerified { ref account_id } => (
                    AuditEventKind::AccountUpdated,
                    AuditSeverity::Informational,
                    serde_json::json!({
                        "account_id": account_id,
                        "action": "email_verified",
                    }),
                ),
                AccountEvent::PasswordChanged { ref account_id } => (
                    AuditEventKind::AccountUpdated,
                    AuditSeverity::Informational,
                    serde_json::json!({
                        "account_id": account_id,
                        "action": "password_changed",
                    }),
                ),
                AccountEvent::RolesUpdated {
                    ref account_id,
                    ref roles,
                } => (
                    AuditEventKind::AccountUpdated,
                    AuditSeverity::Notice,
                    serde_json::json!({
                        "account_id": account_id,
                        "action": "roles_updated",
                        "roles": roles,
                    }),
                ),
                AccountEvent::ProfileUpdated { ref account_id } => (
                    AuditEventKind::AccountUpdated,
                    AuditSeverity::Informational,
                    serde_json::json!({
                        "account_id": account_id,
                        "action": "profile_updated",
                    }),
                ),
            };

            let audit_event = AuditEvent::new(
                kind,
                severity,
                self.audit_logger.service_name().to_string(),
            )
            .with_metadata(metadata);

            self.audit_logger.log(audit_event).await;
        }
    }
}

#[cfg(feature = "audit")]
pub use audit_integration::AuditAccountNotification;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_email_validation() {
        assert!(is_valid_email("user@example.com"));
        assert!(is_valid_email("user+tag@example.co.uk"));
        assert!(!is_valid_email("user@"));
        assert!(!is_valid_email("@example.com"));
        assert!(!is_valid_email("userexample.com"));
        assert!(!is_valid_email("user@example"));
        assert!(!is_valid_email(""));
    }

    #[test]
    fn test_account_error_to_framework_error() {
        let err: Error = AccountError::NotFound("test".into()).into();
        assert!(matches!(err, Error::NotFound(_)));

        let err: Error = AccountError::AlreadyExists("test@test.com".into()).into();
        assert!(matches!(err, Error::Conflict(_)));

        let err: Error = AccountError::InvalidCredentials.into();
        assert!(matches!(err, Error::Unauthorized(_)));

        let err: Error = AccountError::AccountInactive {
            status: AccountStatus::Disabled,
            reason: "test".into(),
        }
        .into();
        assert!(matches!(err, Error::Forbidden(_)));

        let err: Error = AccountError::Validation("bad".into()).into();
        assert!(matches!(err, Error::BadRequest(_)));

        let err: Error = AccountError::Storage("fail".into()).into();
        assert!(matches!(err, Error::Internal(_)));
    }
}
