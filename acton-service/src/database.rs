//! Compatibility connection facade.

use crate::error::Result;

pub(crate) async fn create_pool(config: &crate::config::DatabaseConfig) -> Result<sqlx::PgPool> {
    acton_service_postgres::database::create_pool(&config.into())
        .await
        .map_err(Into::into)
}
