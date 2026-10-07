# acton-service-mssql

Independent Microsoft SQL Server pooling, query primitives and append-only audit storage for acton-service.

`create_pool(&DatabaseConfig)` creates an `MssqlPool`; `health_check`, `execute` and `query` operate on established pools. `DatabaseConfig::mssql_auth` selects connection-string credentials or integrated authentication. `storage::MssqlAuditStorage::new(pool)` constructs audit storage; call `initialize().await` before writing events. `database_error` classifies Tiberius failures.

The backend implements `acton_service_audit::storage::AuditStorage` and returns shared `StorageError` values. The `accounts` and `login-lockout` features enable their audit event parsers.

This package uses Tiberius and bb8 without depending on the facade. The `acton-service` facade's `mssql` feature retains actor orchestration, authentication/account storage integration and its existing public errors. Container-based facade integration tests live in the private integration harness.
