//! PostgreSQL account storage backend

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::AccountStorage;
use crate::accounts::types::{Account, AccountId, AccountStatus};
use crate::error::Error;

/// PostgreSQL-backed account storage
pub struct PgAccountStorage {
    pool: PgPool,
}

impl PgAccountStorage {
    /// Create a new PostgreSQL account storage and initialize the schema
    pub async fn new(pool: PgPool) -> Result<Self, Error> {
        let storage = Self { pool };
        storage.initialize().await?;
        Ok(storage)
    }

    async fn initialize(&self) -> Result<(), Error> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS accounts (
                id VARCHAR(36) PRIMARY KEY,
                email VARCHAR(255) NOT NULL UNIQUE,
                username VARCHAR(255),
                password_hash TEXT,
                status VARCHAR(32) NOT NULL DEFAULT 'pending_verification',
                roles JSONB NOT NULL DEFAULT '[]',
                email_verified BOOLEAN NOT NULL DEFAULT FALSE,
                email_verified_at TIMESTAMPTZ,
                last_login_at TIMESTAMPTZ,
                locked_at TIMESTAMPTZ,
                locked_reason TEXT,
                disabled_at TIMESTAMPTZ,
                disabled_reason TEXT,
                expires_at TIMESTAMPTZ,
                password_changed_at TIMESTAMPTZ,
                failed_login_count INTEGER NOT NULL DEFAULT 0,
                metadata JSONB,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Internal(format!("Failed to create accounts table: {}", e)))?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_accounts_email ON accounts(email)")
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Internal(format!("Failed to create email index: {}", e)))?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_accounts_status ON accounts(status)")
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Internal(format!("Failed to create status index: {}", e)))?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_accounts_username ON accounts(username) WHERE username IS NOT NULL")
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Internal(format!("Failed to create username index: {}", e)))?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_accounts_expires_at ON accounts(expires_at) WHERE expires_at IS NOT NULL")
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Internal(format!("Failed to create expires_at index: {}", e)))?;

        Ok(())
    }
}

/// Internal row type for sqlx mapping
#[derive(sqlx::FromRow)]
struct AccountRow {
    id: String,
    email: String,
    username: Option<String>,
    password_hash: Option<String>,
    status: String,
    roles: serde_json::Value,
    email_verified: bool,
    email_verified_at: Option<DateTime<Utc>>,
    last_login_at: Option<DateTime<Utc>>,
    locked_at: Option<DateTime<Utc>>,
    locked_reason: Option<String>,
    disabled_at: Option<DateTime<Utc>>,
    disabled_reason: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    password_changed_at: Option<DateTime<Utc>>,
    failed_login_count: i32,
    metadata: Option<serde_json::Value>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<AccountRow> for Account {
    fn from(row: AccountRow) -> Self {
        let id = row.id.parse().unwrap_or_else(|_| AccountId::new());

        let status = row
            .status
            .parse()
            .unwrap_or(AccountStatus::PendingVerification);

        let roles: Vec<String> = serde_json::from_value(row.roles).unwrap_or_default();

        Account {
            id,
            email: row.email,
            username: row.username,
            password_hash: row.password_hash,
            status,
            roles,
            email_verified: row.email_verified,
            email_verified_at: row.email_verified_at,
            last_login_at: row.last_login_at,
            locked_at: row.locked_at,
            locked_reason: row.locked_reason,
            disabled_at: row.disabled_at,
            disabled_reason: row.disabled_reason,
            expires_at: row.expires_at,
            password_changed_at: row.password_changed_at,
            failed_login_count: row.failed_login_count as u32,
            metadata: row.metadata,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[async_trait]
impl AccountStorage for PgAccountStorage {
    async fn create(&self, account: &Account) -> Result<(), Error> {
        let roles_json = serde_json::to_value(&account.roles).unwrap_or_default();

        sqlx::query(
            r#"
            INSERT INTO accounts (
                id, email, username, password_hash, status, roles,
                email_verified, email_verified_at, last_login_at,
                locked_at, locked_reason, disabled_at, disabled_reason,
                expires_at, password_changed_at, failed_login_count,
                metadata, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
            "#,
        )
        .bind(account.id.as_str())
        .bind(&account.email)
        .bind(&account.username)
        .bind(&account.password_hash)
        .bind(account.status.to_string())
        .bind(&roles_json)
        .bind(account.email_verified)
        .bind(account.email_verified_at)
        .bind(account.last_login_at)
        .bind(account.locked_at)
        .bind(&account.locked_reason)
        .bind(account.disabled_at)
        .bind(&account.disabled_reason)
        .bind(account.expires_at)
        .bind(account.password_changed_at)
        .bind(account.failed_login_count as i32)
        .bind(&account.metadata)
        .bind(account.created_at)
        .bind(account.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Internal(format!("Failed to create account: {}", e)))?;

        Ok(())
    }

    async fn get_by_id(&self, id: &str) -> Result<Option<Account>, Error> {
        let row = sqlx::query_as::<_, AccountRow>("SELECT * FROM accounts WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| Error::Internal(format!("Failed to get account by id: {}", e)))?;

        Ok(row.map(Into::into))
    }

    async fn get_by_email(&self, email: &str) -> Result<Option<Account>, Error> {
        let row = sqlx::query_as::<_, AccountRow>("SELECT * FROM accounts WHERE email = $1")
            .bind(email)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| Error::Internal(format!("Failed to get account by email: {}", e)))?;

        Ok(row.map(Into::into))
    }

    async fn get_by_username(&self, username: &str) -> Result<Option<Account>, Error> {
        let row = sqlx::query_as::<_, AccountRow>("SELECT * FROM accounts WHERE username = $1")
            .bind(username)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| Error::Internal(format!("Failed to get account by username: {}", e)))?;

        Ok(row.map(Into::into))
    }

    async fn update(&self, account: &Account) -> Result<(), Error> {
        let roles_json = serde_json::to_value(&account.roles).unwrap_or_default();

        sqlx::query(
            r#"
            UPDATE accounts SET
                email = $2, username = $3, password_hash = $4, status = $5,
                roles = $6, email_verified = $7, email_verified_at = $8,
                last_login_at = $9, locked_at = $10, locked_reason = $11,
                disabled_at = $12, disabled_reason = $13, expires_at = $14,
                password_changed_at = $15, failed_login_count = $16,
                metadata = $17, updated_at = $18
            WHERE id = $1
            "#,
        )
        .bind(account.id.as_str())
        .bind(&account.email)
        .bind(&account.username)
        .bind(&account.password_hash)
        .bind(account.status.to_string())
        .bind(&roles_json)
        .bind(account.email_verified)
        .bind(account.email_verified_at)
        .bind(account.last_login_at)
        .bind(account.locked_at)
        .bind(&account.locked_reason)
        .bind(account.disabled_at)
        .bind(&account.disabled_reason)
        .bind(account.expires_at)
        .bind(account.password_changed_at)
        .bind(account.failed_login_count as i32)
        .bind(&account.metadata)
        .bind(Utc::now())
        .execute(&self.pool)
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
        let now = Utc::now();
        let status_str = status.to_string();

        match status {
            AccountStatus::Disabled => {
                sqlx::query(
                    "UPDATE accounts SET status = $2, disabled_at = $3, disabled_reason = $4, updated_at = $3 WHERE id = $1",
                )
                .bind(id)
                .bind(&status_str)
                .bind(now)
                .bind(reason)
                .execute(&self.pool)
                .await
                .map_err(|e| Error::Internal(format!("Failed to update status: {}", e)))?;
            }
            AccountStatus::Locked => {
                sqlx::query(
                    "UPDATE accounts SET status = $2, locked_at = $3, locked_reason = $4, updated_at = $3 WHERE id = $1",
                )
                .bind(id)
                .bind(&status_str)
                .bind(now)
                .bind(reason)
                .execute(&self.pool)
                .await
                .map_err(|e| Error::Internal(format!("Failed to update status: {}", e)))?;
            }
            AccountStatus::Active => {
                sqlx::query(
                    "UPDATE accounts SET status = $2, locked_at = NULL, locked_reason = NULL, disabled_at = NULL, disabled_reason = NULL, updated_at = $3 WHERE id = $1",
                )
                .bind(id)
                .bind(&status_str)
                .bind(now)
                .execute(&self.pool)
                .await
                .map_err(|e| Error::Internal(format!("Failed to update status: {}", e)))?;
            }
            _ => {
                sqlx::query("UPDATE accounts SET status = $2, updated_at = $3 WHERE id = $1")
                    .bind(id)
                    .bind(&status_str)
                    .bind(now)
                    .execute(&self.pool)
                    .await
                    .map_err(|e| Error::Internal(format!("Failed to update status: {}", e)))?;
            }
        }

        Ok(())
    }

    async fn list(
        &self,
        status_filter: Option<AccountStatus>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Account>, Error> {
        let rows = if let Some(status) = status_filter {
            sqlx::query_as::<_, AccountRow>(
                "SELECT * FROM accounts WHERE status = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
            )
            .bind(status.to_string())
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query_as::<_, AccountRow>(
                "SELECT * FROM accounts ORDER BY created_at DESC LIMIT $1 OFFSET $2",
            )
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await
        }
        .map_err(|e| Error::Internal(format!("Failed to list accounts: {}", e)))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn count(&self, status_filter: Option<AccountStatus>) -> Result<u64, Error> {
        let count: (i64,) = if let Some(status) = status_filter {
            sqlx::query_as("SELECT COUNT(*) FROM accounts WHERE status = $1")
                .bind(status.to_string())
                .fetch_one(&self.pool)
                .await
        } else {
            sqlx::query_as("SELECT COUNT(*) FROM accounts")
                .fetch_one(&self.pool)
                .await
        }
        .map_err(|e| Error::Internal(format!("Failed to count accounts: {}", e)))?;

        Ok(count.0 as u64)
    }

    async fn delete(&self, id: &str) -> Result<bool, Error> {
        let result = sqlx::query("DELETE FROM accounts WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Internal(format!("Failed to delete account: {}", e)))?;

        Ok(result.rows_affected() > 0)
    }

    async fn record_login(&self, id: &str) -> Result<(), Error> {
        sqlx::query(
            "UPDATE accounts SET last_login_at = $2, failed_login_count = 0, updated_at = $2 WHERE id = $1",
        )
        .bind(id)
        .bind(Utc::now())
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Internal(format!("Failed to record login: {}", e)))?;

        Ok(())
    }

    async fn find_expired(&self, limit: usize) -> Result<Vec<Account>, Error> {
        let rows = sqlx::query_as::<_, AccountRow>(
            "SELECT * FROM accounts WHERE expires_at IS NOT NULL AND expires_at < $1 AND status != 'expired' ORDER BY expires_at ASC LIMIT $2",
        )
        .bind(Utc::now())
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Internal(format!("Failed to find expired accounts: {}", e)))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn find_inactive(
        &self,
        cutoff: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<Account>, Error> {
        let rows = sqlx::query_as::<_, AccountRow>(
            "SELECT * FROM accounts WHERE status = 'active' AND (last_login_at IS NULL OR last_login_at < $1) ORDER BY last_login_at ASC NULLS FIRST LIMIT $2",
        )
        .bind(cutoff)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Internal(format!("Failed to find inactive accounts: {}", e)))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }
}
