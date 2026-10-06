//! Compatibility connection facade.
use crate::error::Result;
pub(crate) use acton_service_clickhouse::sanitize_url;

pub(crate) async fn create_client(
    config: &crate::config::ClickHouseConfig,
) -> Result<clickhouse::Client> {
    acton_service_clickhouse::clickhouse_backend::create_client(config)
        .await
        .map_err(Into::into)
}

use crate::error::Error;
/// Trait for writing analytical events to ClickHouse
///
/// Provides a standard pattern for sending append-only analytical data
/// (events, metrics, audit logs) to ClickHouse tables.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::prelude::*;
/// use clickhouse::Row;
/// use serde::Serialize;
///
/// #[derive(Row, Serialize)]
/// struct PageView {
///     timestamp: i64,
///     user_id: String,
///     path: String,
///     duration_ms: u64,
/// }
///
/// struct PageViewWriter {
///     client: clickhouse::Client,
/// }
///
/// impl AnalyticsWriter<PageView> for PageViewWriter {
///     fn client(&self) -> &clickhouse::Client {
///         &self.client
///     }
///     fn table_name(&self) -> &str {
///         "page_views"
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait AnalyticsWriter<T>: Send + Sync
where
    T: clickhouse::Row
        + clickhouse::RowOwned
        + clickhouse::RowWrite
        + serde::Serialize
        + Send
        + Sync,
{
    /// Get a reference to the ClickHouse client
    fn client(&self) -> &clickhouse::Client;

    /// Get the target table name
    fn table_name(&self) -> &str;

    /// Write a single row to the table
    async fn write_one(&self, row: T) -> Result<()> {
        let mut insert: clickhouse::insert::Insert<T> = self
            .client()
            .insert(self.table_name())
            .await
            .map_err(|e| Error::ClickHouse(format!("Failed to create insert: {}", e)))?;
        insert
            .write(&row)
            .await
            .map_err(|e| Error::ClickHouse(format!("Failed to write row: {}", e)))?;
        insert
            .end()
            .await
            .map_err(|e| Error::ClickHouse(format!("Failed to flush insert: {}", e)))?;
        Ok(())
    }

    /// Write a batch of rows to the table
    async fn write_batch(&self, rows: Vec<T>) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut insert: clickhouse::insert::Insert<T> = self
            .client()
            .insert(self.table_name())
            .await
            .map_err(|e| Error::ClickHouse(format!("Failed to create insert: {}", e)))?;
        for row in &rows {
            insert
                .write(row)
                .await
                .map_err(|e| Error::ClickHouse(format!("Failed to write row: {}", e)))?;
        }
        insert
            .end()
            .await
            .map_err(|e| Error::ClickHouse(format!("Failed to flush batch: {}", e)))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_clickhouse_error_maps_to_500_with_analytics_code() {
        use axum::response::IntoResponse;

        let err = Error::ClickHouse("connection refused".to_string());
        let response = err.into_response();

        assert_eq!(
            response.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "ClickHouse errors should be 500, not exposed as client errors"
        );
    }
    #[tokio::test]
    async fn test_clickhouse_error_response_body_contains_analytics_code() {
        use axum::response::IntoResponse;

        let err = Error::ClickHouse("query failed".to_string());
        let response = err.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["code"], "ANALYTICS_ERROR");
        // Internal details must NOT leak to the client
        assert!(
            !json["error"].as_str().unwrap().contains("query failed"),
            "Internal error details should not be exposed in response body"
        );
    }
}
