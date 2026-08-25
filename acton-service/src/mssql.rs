//! Microsoft SQL Server connection pooling and query primitives.

use std::time::Duration;

use bb8::Pool;
use bb8_tiberius::ConnectionManager;

use crate::{
    config::DatabaseConfig,
    error::{Error, Result},
};

/// A cloneable Microsoft SQL Server connection pool.
pub type MssqlPool = Pool<ConnectionManager>;

/// Creates a SQL Server pool using the common database retry policy.
pub async fn create_pool(config: &DatabaseConfig) -> Result<MssqlPool> {
    let mut attempt = 0_u32;
    let base_delay = Duration::from_secs(config.retry_delay_secs);

    loop {
        match try_create_pool(config).await {
            Ok(pool) => return Ok(pool),
            Err(error) if attempt >= config.max_retries => return Err(error),
            Err(error) => {
                attempt += 1;
                let multiplier = 2_u32.saturating_pow(attempt.saturating_sub(1));
                let delay = base_delay.saturating_mul(multiplier);
                tracing::warn!(attempt, %error, ?delay, "SQL Server connection failed; retrying");
                tokio::time::sleep(delay).await;
            }
        }
    }
}

async fn try_create_pool(config: &DatabaseConfig) -> Result<MssqlPool> {
    let connection_config = connection_config(config)?;
    let manager = ConnectionManager::build(connection_config).map_err(|error| {
        Error::Internal(format!(
            "invalid SQL Server connection configuration: {error}"
        ))
    })?;
    Pool::builder()
        .max_size(config.max_connections)
        .min_idle(Some(config.min_connections))
        .connection_timeout(Duration::from_secs(config.connection_timeout_secs))
        .build(manager)
        .await
        .map_err(|error| Error::Internal(format!("failed to connect to SQL Server: {error}")))
}

fn connection_config(config: &DatabaseConfig) -> Result<tiberius::Config> {
    let mut connection = tiberius::Config::from_ado_string(&config.url).map_err(|error| {
        Error::Internal(format!(
            "invalid SQL Server connection configuration: {error}"
        ))
    })?;

    if let Some(auth) = auth_method(config.mssql_auth) {
        connection.authentication(auth);
    }

    Ok(connection)
}

fn auth_method(mode: crate::config::MssqlAuthMode) -> Option<tiberius::AuthMethod> {
    match mode {
        crate::config::MssqlAuthMode::ConnectionString => None,
        crate::config::MssqlAuthMode::Integrated => Some(tiberius::AuthMethod::Integrated),
    }
}

/// Checks that SQL Server accepts and executes a query on a pooled connection.
pub async fn health_check(pool: &MssqlPool) -> Result<()> {
    let mut connection = pool.get().await.map_err(|error| {
        Error::Internal(format!("failed to acquire SQL Server connection: {error}"))
    })?;
    connection
        .simple_query("SELECT 1")
        .await
        .map_err(|error| Error::Internal(format!("SQL Server health query failed: {error}")))?;
    Ok(())
}

/// Executes a SQL Server statement and returns the affected-row count.
pub(crate) async fn execute(
    pool: &MssqlPool,
    sql: &str,
    params: &[&dyn tiberius::ToSql],
) -> Result<u64> {
    let mut connection = pool.get().await.map_err(pool_error)?;
    connection
        .execute(sql, params)
        .await
        .map(|result| result.total())
        .map_err(query_error)
}

/// Executes a SQL Server query and materializes its first result set.
pub(crate) async fn query(
    pool: &MssqlPool,
    sql: &str,
    params: &[&dyn tiberius::ToSql],
) -> Result<Vec<tiberius::Row>> {
    let mut connection = pool.get().await.map_err(pool_error)?;
    let rows = connection
        .query(sql, params)
        .await
        .map_err(query_error)?
        .into_first_result()
        .await
        .map_err(query_error)?;
    Ok(rows)
}

fn pool_error(error: bb8::RunError<bb8_tiberius::Error>) -> Error {
    Error::Internal(format!("failed to acquire SQL Server connection: {error}"))
}

fn query_error(error: tiberius::error::Error) -> Error {
    Error::Internal(format!("SQL Server operation failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MssqlAuthMode;

    fn config(auth: MssqlAuthMode) -> DatabaseConfig {
        DatabaseConfig {
            url: "server=tcp:sql.internal,1433;database=app;user=sa;password=secret;TrustServerCertificate=true".to_string(),
            max_connections: 5,
            min_connections: 1,
            connection_timeout_secs: 5,
            max_retries: 0,
            retry_delay_secs: 1,
            optional: false,
            lazy_init: false,
            mssql_auth: auth,
        }
    }

    #[test]
    fn integrated_auth_overrides_connection_string_credentials() {
        assert!(connection_config(&config(MssqlAuthMode::Integrated)).is_ok());
        assert!(matches!(
            auth_method(MssqlAuthMode::Integrated),
            Some(tiberius::AuthMethod::Integrated)
        ));
    }

    #[test]
    fn connection_string_auth_is_preserved() {
        assert!(connection_config(&config(MssqlAuthMode::ConnectionString)).is_ok());
        assert!(auth_method(MssqlAuthMode::ConnectionString).is_none());
    }
}
