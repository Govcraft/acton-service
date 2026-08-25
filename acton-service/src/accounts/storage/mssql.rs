//! Microsoft SQL Server account storage.
use super::AccountStorage;
use crate::{
    accounts::types::{Account, AccountId, AccountStatus},
    error::Error,
    mssql::{execute, query, MssqlPool},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// SQL Server-backed account storage.
pub struct MssqlAccountStorage {
    pool: MssqlPool,
}
impl MssqlAccountStorage {
    /// Creates storage and initializes its schema.
    pub async fn new(pool: MssqlPool) -> Result<Self, Error> {
        execute(&pool,"IF OBJECT_ID(N'accounts',N'U') IS NULL CREATE TABLE accounts(id NVARCHAR(255) PRIMARY KEY,email NVARCHAR(255) NOT NULL UNIQUE,username NVARCHAR(255) NULL,password_hash NVARCHAR(MAX) NULL,status NVARCHAR(32) NOT NULL,roles NVARCHAR(MAX) NOT NULL,email_verified BIT NOT NULL,email_verified_at DATETIMEOFFSET NULL,last_login_at DATETIMEOFFSET NULL,locked_at DATETIMEOFFSET NULL,locked_reason NVARCHAR(MAX) NULL,disabled_at DATETIMEOFFSET NULL,disabled_reason NVARCHAR(MAX) NULL,expires_at DATETIMEOFFSET NULL,password_changed_at DATETIMEOFFSET NULL,failed_login_count INT NOT NULL,metadata NVARCHAR(MAX) NULL,created_at DATETIMEOFFSET NOT NULL,updated_at DATETIMEOFFSET NOT NULL)",&[]).await?;
        Ok(Self { pool })
    }
}
fn text(row: &tiberius::Row, name: &str) -> Result<String, Error> {
    row.get::<&str, _>(name)
        .map(str::to_owned)
        .ok_or_else(|| Error::Internal(format!("missing {name}")))
}
fn decode(row: &tiberius::Row) -> Result<Account, Error> {
    let id = text(row, "id")?
        .parse::<AccountId>()
        .map_err(|error| Error::Internal(error.to_string()))?;
    let status = text(row, "status")?
        .parse::<AccountStatus>()
        .map_err(|error| Error::Internal(error.to_string()))?;
    Ok(Account {
        id,
        email: text(row, "email")?,
        username: row.get::<&str, _>("username").map(str::to_owned),
        password_hash: row.get::<&str, _>("password_hash").map(str::to_owned),
        status,
        roles: serde_json::from_str(text(row, "roles")?.as_str()).unwrap_or_default(),
        email_verified: row.get("email_verified").unwrap_or(false),
        email_verified_at: row.get("email_verified_at"),
        last_login_at: row.get("last_login_at"),
        locked_at: row.get("locked_at"),
        locked_reason: row.get::<&str, _>("locked_reason").map(str::to_owned),
        disabled_at: row.get("disabled_at"),
        disabled_reason: row.get::<&str, _>("disabled_reason").map(str::to_owned),
        expires_at: row.get("expires_at"),
        password_changed_at: row.get("password_changed_at"),
        failed_login_count: row.get::<i32, _>("failed_login_count").unwrap_or(0) as u32,
        metadata: row
            .get::<&str, _>("metadata")
            .and_then(|value| serde_json::from_str(value).ok()),
        created_at: row
            .get("created_at")
            .ok_or_else(|| Error::Internal("missing created_at".to_string()))?,
        updated_at: row
            .get("updated_at")
            .ok_or_else(|| Error::Internal("missing updated_at".to_string()))?,
    })
}
async fn one(pool: &MssqlPool, sql: &str, param: &str) -> Result<Option<Account>, Error> {
    query(pool, sql, &[&param])
        .await?
        .first()
        .map(decode)
        .transpose()
}
#[async_trait]
impl AccountStorage for MssqlAccountStorage {
    async fn create(&self, a: &Account) -> Result<(), Error> {
        let roles = serde_json::to_string(&a.roles).map_err(|e| Error::Internal(e.to_string()))?;
        let metadata = a
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let status = a.status.to_string();
        let failed = a.failed_login_count as i32;
        execute(&self.pool,"INSERT INTO accounts(id,email,username,password_hash,status,roles,email_verified,email_verified_at,last_login_at,locked_at,locked_reason,disabled_at,disabled_reason,expires_at,password_changed_at,failed_login_count,metadata,created_at,updated_at) VALUES(@P1,@P2,@P3,@P4,@P5,@P6,@P7,@P8,@P9,@P10,@P11,@P12,@P13,@P14,@P15,@P16,@P17,@P18,@P19)",&[&a.id.as_str(),&a.email,&a.username,&a.password_hash,&status,&roles,&a.email_verified,&a.email_verified_at,&a.last_login_at,&a.locked_at,&a.locked_reason,&a.disabled_at,&a.disabled_reason,&a.expires_at,&a.password_changed_at,&failed,&metadata,&a.created_at,&a.updated_at]).await?;
        Ok(())
    }
    async fn get_by_id(&self, id: &str) -> Result<Option<Account>, Error> {
        one(&self.pool, "SELECT * FROM accounts WHERE id=@P1", id).await
    }
    async fn get_by_email(&self, email: &str) -> Result<Option<Account>, Error> {
        one(&self.pool, "SELECT * FROM accounts WHERE email=@P1", email).await
    }
    async fn get_by_username(&self, username: &str) -> Result<Option<Account>, Error> {
        one(
            &self.pool,
            "SELECT * FROM accounts WHERE username=@P1",
            username,
        )
        .await
    }
    async fn update(&self, a: &Account) -> Result<(), Error> {
        let roles = serde_json::to_string(&a.roles).map_err(|e| Error::Internal(e.to_string()))?;
        let metadata = a
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let status = a.status.to_string();
        let failed = a.failed_login_count as i32;
        let now = Utc::now();
        execute(&self.pool,"UPDATE accounts SET email=@P2,username=@P3,password_hash=@P4,status=@P5,roles=@P6,email_verified=@P7,email_verified_at=@P8,last_login_at=@P9,locked_at=@P10,locked_reason=@P11,disabled_at=@P12,disabled_reason=@P13,expires_at=@P14,password_changed_at=@P15,failed_login_count=@P16,metadata=@P17,updated_at=@P18 WHERE id=@P1",&[&a.id.as_str(),&a.email,&a.username,&a.password_hash,&status,&roles,&a.email_verified,&a.email_verified_at,&a.last_login_at,&a.locked_at,&a.locked_reason,&a.disabled_at,&a.disabled_reason,&a.expires_at,&a.password_changed_at,&failed,&metadata,&now]).await?;
        Ok(())
    }
    async fn update_status(
        &self,
        id: &str,
        status: AccountStatus,
        reason: Option<&str>,
    ) -> Result<(), Error> {
        let now = Utc::now();
        let value = status.to_string();
        let sql=match status{AccountStatus::Disabled=>"UPDATE accounts SET status=@P2,disabled_at=@P3,disabled_reason=@P4,updated_at=@P3 WHERE id=@P1",AccountStatus::Locked=>"UPDATE accounts SET status=@P2,locked_at=@P3,locked_reason=@P4,updated_at=@P3 WHERE id=@P1",AccountStatus::Active=>"UPDATE accounts SET status=@P2,locked_at=NULL,locked_reason=NULL,disabled_at=NULL,disabled_reason=NULL,updated_at=@P3 WHERE id=@P1",_=>"UPDATE accounts SET status=@P2,updated_at=@P3 WHERE id=@P1"};
        execute(&self.pool, sql, &[&id, &value, &now, &reason]).await?;
        Ok(())
    }
    async fn list(
        &self,
        status: Option<AccountStatus>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Account>, Error> {
        let limit = limit as i64;
        let offset = offset as i64;
        let rows = if let Some(status) = status {
            let value = status.to_string();
            query(&self.pool,"SELECT * FROM accounts WHERE status=@P1 ORDER BY created_at DESC OFFSET @P2 ROWS FETCH NEXT @P3 ROWS ONLY",&[&value,&offset,&limit]).await?
        } else {
            query(&self.pool,"SELECT * FROM accounts ORDER BY created_at DESC OFFSET @P1 ROWS FETCH NEXT @P2 ROWS ONLY",&[&offset,&limit]).await?
        };
        rows.iter().map(decode).collect()
    }
    async fn count(&self, status: Option<AccountStatus>) -> Result<u64, Error> {
        let rows = if let Some(status) = status {
            let value = status.to_string();
            query(
                &self.pool,
                "SELECT COUNT_BIG(*) AS count FROM accounts WHERE status=@P1",
                &[&value],
            )
            .await?
        } else {
            query(
                &self.pool,
                "SELECT COUNT_BIG(*) AS count FROM accounts",
                &[],
            )
            .await?
        };
        Ok(rows
            .first()
            .and_then(|r| r.get::<i64, _>("count"))
            .unwrap_or(0) as u64)
    }
    async fn delete(&self, id: &str) -> Result<bool, Error> {
        Ok(execute(&self.pool, "DELETE FROM accounts WHERE id=@P1", &[&id]).await? > 0)
    }
    async fn record_login(&self, id: &str) -> Result<(), Error> {
        execute(&self.pool,"UPDATE accounts SET last_login_at=SYSDATETIMEOFFSET(),failed_login_count=0,updated_at=SYSDATETIMEOFFSET() WHERE id=@P1",&[&id]).await?;
        Ok(())
    }
    async fn find_expired(&self, limit: usize) -> Result<Vec<Account>, Error> {
        let limit = limit as i64;
        query(&self.pool,"SELECT TOP (@P1) * FROM accounts WHERE expires_at<SYSDATETIMEOFFSET() AND status<>'expired' ORDER BY expires_at",&[&limit]).await?.iter().map(decode).collect()
    }
    async fn find_inactive(
        &self,
        cutoff: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<Account>, Error> {
        let limit = limit as i64;
        query(&self.pool,"SELECT TOP (@P2) * FROM accounts WHERE status='active' AND (last_login_at IS NULL OR last_login_at<@P1) ORDER BY CASE WHEN last_login_at IS NULL THEN 0 ELSE 1 END,last_login_at",&[&cutoff,&limit]).await?.iter().map(decode).collect()
    }
}
