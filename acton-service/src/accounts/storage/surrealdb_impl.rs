//! SurrealDB account storage backend

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::AccountStorage;
use crate::accounts::types::{Account, AccountId, AccountStatus};
use crate::error::Error;
use crate::surrealdb_backend::SurrealClient;

/// SurrealDB-backed account storage
pub struct SurrealDbAccountStorage {
    client: Arc<SurrealClient>,
}

impl SurrealDbAccountStorage {
    /// Create a new SurrealDB account storage and initialize the schema
    pub async fn new(client: Arc<SurrealClient>) -> Result<Self, Error> {
        let storage = Self { client };
        storage.initialize().await?;
        Ok(storage)
    }

    async fn initialize(&self) -> Result<(), Error> {
        self.client
            .query(
                r#"
                DEFINE TABLE IF NOT EXISTS accounts SCHEMAFULL;
                DEFINE FIELD IF NOT EXISTS email ON accounts TYPE string;
                DEFINE FIELD IF NOT EXISTS username ON accounts TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS password_hash ON accounts TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS status ON accounts TYPE string;
                DEFINE FIELD IF NOT EXISTS roles ON accounts TYPE string;
                DEFINE FIELD IF NOT EXISTS email_verified ON accounts TYPE bool;
                DEFINE FIELD IF NOT EXISTS email_verified_at ON accounts TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS last_login_at ON accounts TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS locked_at ON accounts TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS locked_reason ON accounts TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS disabled_at ON accounts TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS disabled_reason ON accounts TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS expires_at ON accounts TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS password_changed_at ON accounts TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS failed_login_count ON accounts TYPE int;
                DEFINE FIELD IF NOT EXISTS metadata ON accounts TYPE option<string>;
                DEFINE FIELD IF NOT EXISTS created_at ON accounts TYPE string;
                DEFINE FIELD IF NOT EXISTS updated_at ON accounts TYPE string;
                DEFINE INDEX IF NOT EXISTS idx_accounts_email ON accounts FIELDS email UNIQUE;
                DEFINE INDEX IF NOT EXISTS idx_accounts_status ON accounts FIELDS status;
                "#,
            )
            .await
            .map_err(|e| Error::Internal(format!("Failed to initialize accounts schema: {}", e)))?;

        Ok(())
    }
}

#[derive(Serialize)]
struct AccountRecord {
    email: String,
    username: Option<String>,
    password_hash: Option<String>,
    status: String,
    roles: String,
    email_verified: bool,
    email_verified_at: Option<String>,
    last_login_at: Option<String>,
    locked_at: Option<String>,
    locked_reason: Option<String>,
    disabled_at: Option<String>,
    disabled_reason: Option<String>,
    expires_at: Option<String>,
    password_changed_at: Option<String>,
    failed_login_count: i64,
    metadata: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize)]
struct AccountRow {
    id: serde_json::Value,
    email: String,
    username: Option<String>,
    password_hash: Option<String>,
    status: String,
    roles: String,
    email_verified: bool,
    email_verified_at: Option<String>,
    last_login_at: Option<String>,
    locked_at: Option<String>,
    locked_reason: Option<String>,
    disabled_at: Option<String>,
    disabled_reason: Option<String>,
    expires_at: Option<String>,
    password_changed_at: Option<String>,
    failed_login_count: i64,
    metadata: Option<String>,
    created_at: String,
    updated_at: String,
}

fn parse_dt(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn opt_dt(dt: &Option<DateTime<Utc>>) -> Option<String> {
    dt.map(|d| d.to_rfc3339())
}

/// Extract the record key from a SurrealDB record ID value.
/// SurrealDB returns IDs in various formats depending on version.
fn extract_id(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => {
            // "accounts:acct_xxx" -> "acct_xxx"
            s.split(':').last().unwrap_or(s).to_string()
        }
        serde_json::Value::Object(obj) => obj
            .get("id")
            .and_then(|v| match v {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Object(inner) => inner
                    .get("String")
                    .and_then(|s| s.as_str().map(String::from)),
                _ => None,
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

impl From<AccountRow> for Account {
    fn from(row: AccountRow) -> Self {
        let id_str = extract_id(&row.id);
        let id = id_str.parse().unwrap_or_else(|_| AccountId::new());
        let status = row
            .status
            .parse()
            .unwrap_or(AccountStatus::PendingVerification);
        let roles: Vec<String> = serde_json::from_str(&row.roles).unwrap_or_default();

        Account {
            id,
            email: row.email,
            username: row.username,
            password_hash: row.password_hash,
            status,
            roles,
            email_verified: row.email_verified,
            email_verified_at: row.email_verified_at.and_then(|s| parse_dt(&s)),
            last_login_at: row.last_login_at.and_then(|s| parse_dt(&s)),
            locked_at: row.locked_at.and_then(|s| parse_dt(&s)),
            locked_reason: row.locked_reason,
            disabled_at: row.disabled_at.and_then(|s| parse_dt(&s)),
            disabled_reason: row.disabled_reason,
            expires_at: row.expires_at.and_then(|s| parse_dt(&s)),
            password_changed_at: row.password_changed_at.and_then(|s| parse_dt(&s)),
            failed_login_count: row.failed_login_count as u32,
            metadata: row.metadata.and_then(|s| serde_json::from_str(&s).ok()),
            created_at: parse_dt(&row.created_at).unwrap_or_else(Utc::now),
            updated_at: parse_dt(&row.updated_at).unwrap_or_else(Utc::now),
        }
    }
}

fn to_record(account: &Account) -> AccountRecord {
    AccountRecord {
        email: account.email.clone(),
        username: account.username.clone(),
        password_hash: account.password_hash.clone(),
        status: account.status.to_string(),
        roles: serde_json::to_string(&account.roles).unwrap_or_else(|_| "[]".into()),
        email_verified: account.email_verified,
        email_verified_at: opt_dt(&account.email_verified_at),
        last_login_at: opt_dt(&account.last_login_at),
        locked_at: opt_dt(&account.locked_at),
        locked_reason: account.locked_reason.clone(),
        disabled_at: opt_dt(&account.disabled_at),
        disabled_reason: account.disabled_reason.clone(),
        expires_at: opt_dt(&account.expires_at),
        password_changed_at: opt_dt(&account.password_changed_at),
        failed_login_count: account.failed_login_count as i64,
        metadata: account
            .metadata
            .as_ref()
            .map(|m| serde_json::to_string(m).unwrap_or_default()),
        created_at: account.created_at.to_rfc3339(),
        updated_at: account.updated_at.to_rfc3339(),
    }
}

#[async_trait]
impl AccountStorage for SurrealDbAccountStorage {
    async fn create(&self, account: &Account) -> Result<(), Error> {
        let record = to_record(account);
        let record_id = account.id.to_string();

        self.client
            .query("CREATE type::thing('accounts', $id) CONTENT $data")
            .bind(("id", record_id))
            .bind(("data", record))
            .await
            .map_err(|e| Error::Internal(format!("Failed to create account: {}", e)))?;

        Ok(())
    }

    async fn get_by_id(&self, id: &str) -> Result<Option<Account>, Error> {
        let mut result = self
            .client
            .query("SELECT * FROM type::thing('accounts', $id)")
            .bind(("id", id.to_string()))
            .await
            .map_err(|e| Error::Internal(format!("Failed to get account: {}", e)))?;

        let rows: Vec<AccountRow> = result
            .take(0)
            .map_err(|e| Error::Internal(format!("Failed to deserialize account: {}", e)))?;

        Ok(rows.into_iter().next().map(Into::into))
    }

    async fn get_by_email(&self, email: &str) -> Result<Option<Account>, Error> {
        let mut result = self
            .client
            .query("SELECT * FROM accounts WHERE email = $email LIMIT 1")
            .bind(("email", email.to_string()))
            .await
            .map_err(|e| Error::Internal(format!("Failed to get account by email: {}", e)))?;

        let rows: Vec<AccountRow> = result
            .take(0)
            .map_err(|e| Error::Internal(format!("Failed to deserialize account: {}", e)))?;

        Ok(rows.into_iter().next().map(Into::into))
    }

    async fn get_by_username(&self, username: &str) -> Result<Option<Account>, Error> {
        let mut result = self
            .client
            .query("SELECT * FROM accounts WHERE username = $username LIMIT 1")
            .bind(("username", username.to_string()))
            .await
            .map_err(|e| Error::Internal(format!("Failed to get account by username: {}", e)))?;

        let rows: Vec<AccountRow> = result
            .take(0)
            .map_err(|e| Error::Internal(format!("Failed to deserialize account: {}", e)))?;

        Ok(rows.into_iter().next().map(Into::into))
    }

    async fn update(&self, account: &Account) -> Result<(), Error> {
        let record = to_record(account);
        let record_id = account.id.to_string();

        self.client
            .query("UPDATE type::thing('accounts', $id) CONTENT $data")
            .bind(("id", record_id))
            .bind(("data", record))
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
        let now = Utc::now().to_rfc3339();
        let status_str = status.to_string();
        let reason_owned = reason.map(|s| s.to_string());

        match status {
            AccountStatus::Disabled => {
                self.client
                    .query("UPDATE type::thing('accounts', $id) SET status = $status, disabled_at = $now, disabled_reason = $reason, updated_at = $now")
                    .bind(("id", id.to_string()))
                    .bind(("status", status_str))
                    .bind(("now", now))
                    .bind(("reason", reason_owned))
                    .await
            }
            AccountStatus::Locked => {
                self.client
                    .query("UPDATE type::thing('accounts', $id) SET status = $status, locked_at = $now, locked_reason = $reason, updated_at = $now")
                    .bind(("id", id.to_string()))
                    .bind(("status", status_str))
                    .bind(("now", now))
                    .bind(("reason", reason_owned))
                    .await
            }
            AccountStatus::Active => {
                self.client
                    .query("UPDATE type::thing('accounts', $id) SET status = $status, locked_at = NONE, locked_reason = NONE, disabled_at = NONE, disabled_reason = NONE, updated_at = $now")
                    .bind(("id", id.to_string()))
                    .bind(("status", status_str))
                    .bind(("now", now))
                    .await
            }
            _ => {
                self.client
                    .query("UPDATE type::thing('accounts', $id) SET status = $status, updated_at = $now")
                    .bind(("id", id.to_string()))
                    .bind(("status", status_str))
                    .bind(("now", now))
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
        let mut result = if let Some(status) = status_filter {
            self.client
                .query("SELECT * FROM accounts WHERE status = $status ORDER BY created_at DESC LIMIT $limit START $offset")
                .bind(("status", status.to_string()))
                .bind(("limit", limit as i64))
                .bind(("offset", offset as i64))
                .await
        } else {
            self.client
                .query("SELECT * FROM accounts ORDER BY created_at DESC LIMIT $limit START $offset")
                .bind(("limit", limit as i64))
                .bind(("offset", offset as i64))
                .await
        }
        .map_err(|e| Error::Internal(format!("Failed to list accounts: {}", e)))?;

        let rows: Vec<AccountRow> = result
            .take(0)
            .map_err(|e| Error::Internal(format!("Failed to deserialize accounts: {}", e)))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn count(&self, status_filter: Option<AccountStatus>) -> Result<u64, Error> {
        #[derive(Deserialize)]
        struct CountRow {
            count: i64,
        }

        let mut result = if let Some(status) = status_filter {
            self.client
                .query("SELECT count() AS count FROM accounts WHERE status = $status GROUP ALL")
                .bind(("status", status.to_string()))
                .await
        } else {
            self.client
                .query("SELECT count() AS count FROM accounts GROUP ALL")
                .await
        }
        .map_err(|e| Error::Internal(format!("Failed to count accounts: {}", e)))?;

        let rows: Vec<CountRow> = result.take(0).unwrap_or_default();
        Ok(rows.first().map(|r| r.count as u64).unwrap_or(0))
    }

    async fn delete(&self, id: &str) -> Result<bool, Error> {
        // Check existence first
        let exists = self.get_by_id(id).await?.is_some();
        if !exists {
            return Ok(false);
        }

        self.client
            .query("DELETE type::thing('accounts', $id)")
            .bind(("id", id.to_string()))
            .await
            .map_err(|e| Error::Internal(format!("Failed to delete account: {}", e)))?;

        Ok(true)
    }

    async fn record_login(&self, id: &str) -> Result<(), Error> {
        let now = Utc::now().to_rfc3339();

        self.client
            .query("UPDATE type::thing('accounts', $id) SET last_login_at = $now, failed_login_count = 0, updated_at = $now")
            .bind(("id", id.to_string()))
            .bind(("now", now))
            .await
            .map_err(|e| Error::Internal(format!("Failed to record login: {}", e)))?;

        Ok(())
    }

    async fn find_expired(&self, limit: usize) -> Result<Vec<Account>, Error> {
        let now = Utc::now().to_rfc3339();

        let mut result = self
            .client
            .query("SELECT * FROM accounts WHERE expires_at IS NOT NONE AND expires_at < $now AND status != 'expired' ORDER BY expires_at ASC LIMIT $limit")
            .bind(("now", now))
            .bind(("limit", limit as i64))
            .await
            .map_err(|e| Error::Internal(format!("Failed to find expired: {}", e)))?;

        let rows: Vec<AccountRow> = result
            .take(0)
            .map_err(|e| Error::Internal(format!("Failed to deserialize: {}", e)))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn find_inactive(
        &self,
        cutoff: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<Account>, Error> {
        let cutoff_str = cutoff.to_rfc3339();

        let mut result = self
            .client
            .query("SELECT * FROM accounts WHERE status = 'active' AND (last_login_at IS NONE OR last_login_at < $cutoff) ORDER BY last_login_at ASC LIMIT $limit")
            .bind(("cutoff", cutoff_str))
            .bind(("limit", limit as i64))
            .await
            .map_err(|e| Error::Internal(format!("Failed to find inactive: {}", e)))?;

        let rows: Vec<AccountRow> = result
            .take(0)
            .map_err(|e| Error::Internal(format!("Failed to deserialize: {}", e)))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }
}
