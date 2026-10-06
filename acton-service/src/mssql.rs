//! Compatibility connection facade.
use crate::error::Result;
pub use acton_service_mssql::MssqlPool;

/// Create a SQL Server pool using the common database retry policy.
pub async fn create_pool(config: &crate::config::DatabaseConfig) -> Result<MssqlPool> {
    acton_service_mssql::mssql::create_pool(&config.into())
        .await
        .map_err(Into::into)
}

/// Check that SQL Server executes a query on a pooled connection.
pub async fn health_check(pool: &MssqlPool) -> Result<()> {
    acton_service_mssql::mssql::health_check(pool)
        .await
        .map_err(Into::into)
}

pub(crate) async fn execute(
    pool: &MssqlPool,
    sql: &str,
    params: &[&dyn tiberius::ToSql],
) -> Result<u64> {
    acton_service_mssql::mssql::execute(pool, sql, params)
        .await
        .map_err(Into::into)
}

pub(crate) async fn query(
    pool: &MssqlPool,
    sql: &str,
    params: &[&dyn tiberius::ToSql],
) -> Result<Vec<tiberius::Row>> {
    acton_service_mssql::mssql::query(pool, sql, params)
        .await
        .map_err(Into::into)
}
