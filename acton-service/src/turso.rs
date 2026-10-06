//! Compatibility connection facade.

use crate::error::Result;

pub(crate) async fn create_database(
    config: &crate::config::TursoConfig,
) -> Result<libsql::Database> {
    acton_service_turso::turso::create_database(config)
        .await
        .map_err(Into::into)
}
