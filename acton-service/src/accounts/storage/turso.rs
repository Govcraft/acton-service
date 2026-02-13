//! Turso/libsql account storage backend

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;

use super::AccountStorage;
use crate::accounts::types::{Account, AccountId, AccountStatus};
use crate::error::Error;

/// Turso-backed account storage
pub struct TursoAccountStorage {
    db: Arc<libsql::Database>,
}

impl TursoAccountStorage {
    /// Create a new Turso account storage and initialize the schema
    pub async fn new(db: Arc<libsql::Database>) -> Result<Self, Error> {
        let storage = Self { db };
        storage.initialize().await?;
        Ok(storage)
    }

    async fn initialize(&self) -> Result<(), Error> {
        let conn = self
            .db
            .connect()
            .map_err(|e| Error::Internal(format!("Failed to connect for accounts init: {}", e)))?;

        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS accounts (
                id TEXT PRIMARY KEY,
                email TEXT NOT NULL UNIQUE,
                username TEXT,
                password_hash TEXT,
                status TEXT NOT NULL DEFAULT 'pending_verification',
                roles TEXT NOT NULL DEFAULT '[]',
                email_verified INTEGER NOT NULL DEFAULT 0,
                email_verified_at TEXT,
                last_login_at TEXT,
                locked_at TEXT,
                locked_reason TEXT,
                disabled_at TEXT,
                disabled_reason TEXT,
                expires_at TEXT,
                password_changed_at TEXT,
                failed_login_count INTEGER NOT NULL DEFAULT 0,
                metadata TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
            (),
        )
        .await
        .map_err(|e| Error::Internal(format!("Failed to create accounts table: {}", e)))?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_accounts_email ON accounts(email)",
            (),
        )
        .await
        .map_err(|e| Error::Internal(format!("Failed to create email index: {}", e)))?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_accounts_status ON accounts(status)",
            (),
        )
        .await
        .map_err(|e| Error::Internal(format!("Failed to create status index: {}", e)))?;

        Ok(())
    }

    fn conn(&self) -> Result<libsql::Connection, Error> {
        self.db
            .connect()
            .map_err(|e| Error::Internal(format!("Failed to connect: {}", e)))
    }
}

fn parse_datetime(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn row_to_account(row: &libsql::Row) -> Result<Account, Error> {
    let map_err = |field: &str, e: libsql::Error| {
        Error::Internal(format!("Failed to read field '{}': {}", field, e))
    };

    let id_str: String = row.get(0).map_err(|e| map_err("id", e))?;
    let id = id_str.parse().unwrap_or_else(|_| AccountId::new());

    let email: String = row.get(1).map_err(|e| map_err("email", e))?;
    let username: Option<String> = row.get(2).map_err(|e| map_err("username", e))?;
    let password_hash: Option<String> = row.get(3).map_err(|e| map_err("password_hash", e))?;
    let status_str: String = row.get(4).map_err(|e| map_err("status", e))?;
    let status = status_str
        .parse()
        .unwrap_or(AccountStatus::PendingVerification);
    let roles_str: String = row.get(5).map_err(|e| map_err("roles", e))?;
    let roles: Vec<String> = serde_json::from_str(&roles_str).unwrap_or_default();
    let email_verified_int: i64 = row.get(6).map_err(|e| map_err("email_verified", e))?;
    let email_verified = email_verified_int != 0;
    let email_verified_at: Option<String> =
        row.get(7).map_err(|e| map_err("email_verified_at", e))?;
    let last_login_at: Option<String> = row.get(8).map_err(|e| map_err("last_login_at", e))?;
    let locked_at: Option<String> = row.get(9).map_err(|e| map_err("locked_at", e))?;
    let locked_reason: Option<String> = row.get(10).map_err(|e| map_err("locked_reason", e))?;
    let disabled_at: Option<String> = row.get(11).map_err(|e| map_err("disabled_at", e))?;
    let disabled_reason: Option<String> = row.get(12).map_err(|e| map_err("disabled_reason", e))?;
    let expires_at: Option<String> = row.get(13).map_err(|e| map_err("expires_at", e))?;
    let password_changed_at: Option<String> =
        row.get(14).map_err(|e| map_err("password_changed_at", e))?;
    let failed_login_count: i64 = row.get(15).map_err(|e| map_err("failed_login_count", e))?;
    let metadata_str: Option<String> = row.get(16).map_err(|e| map_err("metadata", e))?;
    let created_at_str: String = row.get(17).map_err(|e| map_err("created_at", e))?;
    let updated_at_str: String = row.get(18).map_err(|e| map_err("updated_at", e))?;

    Ok(Account {
        id,
        email,
        username,
        password_hash,
        status,
        roles,
        email_verified,
        email_verified_at: email_verified_at.and_then(|s| parse_datetime(&s)),
        last_login_at: last_login_at.and_then(|s| parse_datetime(&s)),
        locked_at: locked_at.and_then(|s| parse_datetime(&s)),
        locked_reason,
        disabled_at: disabled_at.and_then(|s| parse_datetime(&s)),
        disabled_reason,
        expires_at: expires_at.and_then(|s| parse_datetime(&s)),
        password_changed_at: password_changed_at.and_then(|s| parse_datetime(&s)),
        failed_login_count: failed_login_count as u32,
        metadata: metadata_str.and_then(|s| serde_json::from_str(&s).ok()),
        created_at: parse_datetime(&created_at_str).unwrap_or_else(Utc::now),
        updated_at: parse_datetime(&updated_at_str).unwrap_or_else(Utc::now),
    })
}

fn opt_dt(dt: &Option<DateTime<Utc>>) -> Option<String> {
    dt.map(|d| d.to_rfc3339())
}

#[async_trait]
impl AccountStorage for TursoAccountStorage {
    async fn create(&self, account: &Account) -> Result<(), Error> {
        let conn = self.conn()?;
        let roles_json = serde_json::to_string(&account.roles).unwrap_or_else(|_| "[]".into());
        let metadata_json = account
            .metadata
            .as_ref()
            .map(|m| serde_json::to_string(m).unwrap_or_default());

        conn.execute(
            r#"
            INSERT INTO accounts (
                id, email, username, password_hash, status, roles,
                email_verified, email_verified_at, last_login_at,
                locked_at, locked_reason, disabled_at, disabled_reason,
                expires_at, password_changed_at, failed_login_count,
                metadata, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
            "#,
            libsql::params![
                account.id.as_str(),
                account.email.as_str(),
                account.username.as_deref(),
                account.password_hash.as_deref(),
                account.status.to_string(),
                roles_json,
                account.email_verified as i64,
                opt_dt(&account.email_verified_at),
                opt_dt(&account.last_login_at),
                opt_dt(&account.locked_at),
                account.locked_reason.as_deref(),
                opt_dt(&account.disabled_at),
                account.disabled_reason.as_deref(),
                opt_dt(&account.expires_at),
                opt_dt(&account.password_changed_at),
                account.failed_login_count as i64,
                metadata_json,
                account.created_at.to_rfc3339(),
                account.updated_at.to_rfc3339(),
            ],
        )
        .await
        .map_err(|e| Error::Internal(format!("Failed to create account: {}", e)))?;

        Ok(())
    }

    async fn get_by_id(&self, id: &str) -> Result<Option<Account>, Error> {
        let conn = self.conn()?;
        let mut rows = conn
            .query("SELECT * FROM accounts WHERE id = ?1", libsql::params![id])
            .await
            .map_err(|e| Error::Internal(format!("Failed to get account: {}", e)))?;

        match rows.next().await {
            Ok(Some(row)) => Ok(Some(row_to_account(&row)?)),
            Ok(None) => Ok(None),
            Err(e) => Err(Error::Internal(format!("Failed to read row: {}", e))),
        }
    }

    async fn get_by_email(&self, email: &str) -> Result<Option<Account>, Error> {
        let conn = self.conn()?;
        let mut rows = conn
            .query(
                "SELECT * FROM accounts WHERE email = ?1",
                libsql::params![email],
            )
            .await
            .map_err(|e| Error::Internal(format!("Failed to get account by email: {}", e)))?;

        match rows.next().await {
            Ok(Some(row)) => Ok(Some(row_to_account(&row)?)),
            Ok(None) => Ok(None),
            Err(e) => Err(Error::Internal(format!("Failed to read row: {}", e))),
        }
    }

    async fn get_by_username(&self, username: &str) -> Result<Option<Account>, Error> {
        let conn = self.conn()?;
        let mut rows = conn
            .query(
                "SELECT * FROM accounts WHERE username = ?1",
                libsql::params![username],
            )
            .await
            .map_err(|e| Error::Internal(format!("Failed to get account by username: {}", e)))?;

        match rows.next().await {
            Ok(Some(row)) => Ok(Some(row_to_account(&row)?)),
            Ok(None) => Ok(None),
            Err(e) => Err(Error::Internal(format!("Failed to read row: {}", e))),
        }
    }

    async fn update(&self, account: &Account) -> Result<(), Error> {
        let conn = self.conn()?;
        let roles_json = serde_json::to_string(&account.roles).unwrap_or_else(|_| "[]".into());
        let metadata_json = account
            .metadata
            .as_ref()
            .map(|m| serde_json::to_string(m).unwrap_or_default());

        conn.execute(
            r#"
            UPDATE accounts SET
                email = ?2, username = ?3, password_hash = ?4, status = ?5,
                roles = ?6, email_verified = ?7, email_verified_at = ?8,
                last_login_at = ?9, locked_at = ?10, locked_reason = ?11,
                disabled_at = ?12, disabled_reason = ?13, expires_at = ?14,
                password_changed_at = ?15, failed_login_count = ?16,
                metadata = ?17, updated_at = ?18
            WHERE id = ?1
            "#,
            libsql::params![
                account.id.as_str(),
                account.email.as_str(),
                account.username.as_deref(),
                account.password_hash.as_deref(),
                account.status.to_string(),
                roles_json,
                account.email_verified as i64,
                opt_dt(&account.email_verified_at),
                opt_dt(&account.last_login_at),
                opt_dt(&account.locked_at),
                account.locked_reason.as_deref(),
                opt_dt(&account.disabled_at),
                account.disabled_reason.as_deref(),
                opt_dt(&account.expires_at),
                opt_dt(&account.password_changed_at),
                account.failed_login_count as i64,
                metadata_json,
                Utc::now().to_rfc3339(),
            ],
        )
        .await
        .map_err(|e| Error::Internal(format!("Failed to update account: {}", e)))?;

        Ok(())
    }

    async fn update_status(
        &self,
        id: &str,
        status: AccountStatus,
        reason: Option<&str>,
    ) -> Result<(), Error> {
        let conn = self.conn()?;
        let now = Utc::now().to_rfc3339();
        let status_str = status.to_string();

        match status {
            AccountStatus::Disabled => {
                conn.execute(
                    "UPDATE accounts SET status = ?2, disabled_at = ?3, disabled_reason = ?4, updated_at = ?3 WHERE id = ?1",
                    libsql::params![id, status_str, now, reason],
                )
                .await
            }
            AccountStatus::Locked => {
                conn.execute(
                    "UPDATE accounts SET status = ?2, locked_at = ?3, locked_reason = ?4, updated_at = ?3 WHERE id = ?1",
                    libsql::params![id, status_str, now, reason],
                )
                .await
            }
            AccountStatus::Active => {
                conn.execute(
                    "UPDATE accounts SET status = ?2, locked_at = NULL, locked_reason = NULL, disabled_at = NULL, disabled_reason = NULL, updated_at = ?3 WHERE id = ?1",
                    libsql::params![id, status_str, now],
                )
                .await
            }
            _ => {
                conn.execute(
                    "UPDATE accounts SET status = ?2, updated_at = ?3 WHERE id = ?1",
                    libsql::params![id, status_str, now],
                )
                .await
            }
        }
        .map_err(|e| Error::Internal(format!("Failed to update status: {}", e)))?;

        Ok(())
    }

    async fn list(
        &self,
        status_filter: Option<AccountStatus>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Account>, Error> {
        let conn = self.conn()?;
        let mut accounts = Vec::new();

        let mut rows = if let Some(status) = status_filter {
            conn.query(
                "SELECT * FROM accounts WHERE status = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
                libsql::params![status.to_string(), limit as i64, offset as i64],
            )
            .await
        } else {
            conn.query(
                "SELECT * FROM accounts ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
                libsql::params![limit as i64, offset as i64],
            )
            .await
        }
        .map_err(|e| Error::Internal(format!("Failed to list accounts: {}", e)))?;

        while let Ok(Some(row)) = rows.next().await {
            accounts.push(row_to_account(&row)?);
        }

        Ok(accounts)
    }

    async fn count(&self, status_filter: Option<AccountStatus>) -> Result<u64, Error> {
        let conn = self.conn()?;

        let mut rows = if let Some(status) = status_filter {
            conn.query(
                "SELECT COUNT(*) FROM accounts WHERE status = ?1",
                libsql::params![status.to_string()],
            )
            .await
        } else {
            conn.query("SELECT COUNT(*) FROM accounts", ()).await
        }
        .map_err(|e| Error::Internal(format!("Failed to count accounts: {}", e)))?;

        match rows.next().await {
            Ok(Some(row)) => {
                let count: i64 = row
                    .get(0)
                    .map_err(|e| Error::Internal(format!("Failed to read count: {}", e)))?;
                Ok(count as u64)
            }
            _ => Ok(0),
        }
    }

    async fn delete(&self, id: &str) -> Result<bool, Error> {
        let conn = self.conn()?;
        let affected = conn
            .execute("DELETE FROM accounts WHERE id = ?1", libsql::params![id])
            .await
            .map_err(|e| Error::Internal(format!("Failed to delete account: {}", e)))?;

        Ok(affected > 0)
    }

    async fn record_login(&self, id: &str) -> Result<(), Error> {
        let conn = self.conn()?;
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "UPDATE accounts SET last_login_at = ?2, failed_login_count = 0, updated_at = ?2 WHERE id = ?1",
            libsql::params![id, now],
        )
        .await
        .map_err(|e| Error::Internal(format!("Failed to record login: {}", e)))?;

        Ok(())
    }

    async fn find_expired(&self, limit: usize) -> Result<Vec<Account>, Error> {
        let conn = self.conn()?;
        let now = Utc::now().to_rfc3339();
        let mut accounts = Vec::new();

        let mut rows = conn
            .query(
                "SELECT * FROM accounts WHERE expires_at IS NOT NULL AND expires_at < ?1 AND status != 'expired' ORDER BY expires_at ASC LIMIT ?2",
                libsql::params![now, limit as i64],
            )
            .await
            .map_err(|e| Error::Internal(format!("Failed to find expired: {}", e)))?;

        while let Ok(Some(row)) = rows.next().await {
            accounts.push(row_to_account(&row)?);
        }

        Ok(accounts)
    }

    async fn find_inactive(
        &self,
        cutoff: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<Account>, Error> {
        let conn = self.conn()?;
        let cutoff_str = cutoff.to_rfc3339();
        let mut accounts = Vec::new();

        let mut rows = conn
            .query(
                "SELECT * FROM accounts WHERE status = 'active' AND (last_login_at IS NULL OR last_login_at < ?1) ORDER BY last_login_at ASC LIMIT ?2",
                libsql::params![cutoff_str, limit as i64],
            )
            .await
            .map_err(|e| Error::Internal(format!("Failed to find inactive: {}", e)))?;

        while let Ok(Some(row)) = rows.next().await {
            accounts.push(row_to_account(&row)?);
        }

        Ok(accounts)
    }
}
