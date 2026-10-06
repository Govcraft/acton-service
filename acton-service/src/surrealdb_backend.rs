//! Compatibility connection facade.
use crate::error::Result;
pub use acton_service_surrealdb::{sanitize_url, SurrealClient};

pub(crate) async fn create_client(
    config: &crate::config::SurrealDbConfig,
) -> Result<SurrealClient> {
    acton_service_surrealdb::surrealdb_backend::create_client(config)
        .await
        .map_err(Into::into)
}
