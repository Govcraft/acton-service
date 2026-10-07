# acton-service-clickhouse

Independent ClickHouse client management and append-only analytical audit storage for acton-service.

`create_client(&ClickHouseConfig)` constructs and checks a ClickHouse client using the configured retry policy. `storage::ClickHouseAuditStorage::new(client)` constructs audit storage; call `initialize().await` to create its MergeTree schema. The backend implements `acton_service_audit::storage::AuditStorage` and returns shared `StorageError` values. The `accounts` and `login-lockout` features enable their audit event parsers.

This package depends on core contracts, audit logic and the ClickHouse driver. The `acton-service` facade's `clickhouse` feature retains actor orchestration, `AnalyticsWriter`, HTTP error conversion and its existing public storage wrappers.
