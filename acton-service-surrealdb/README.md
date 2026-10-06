# acton-service-surrealdb

Independent SurrealDB client management and append-only audit storage for acton-service.

`create_client(&SurrealDbConfig)` creates a `SurrealClient` using the URL's protocol and configured retry/authentication settings. The existing SDK supports WebSocket, HTTP and in-memory connections. `storage::SurrealAuditStorage::new(Arc<SurrealClient>)` constructs audit storage; call `initialize().await` to create its schema and permissions. `database_error` classifies SDK failures.

The backend implements `acton_service_audit::storage::AuditStorage` and returns shared `StorageError` values. The `accounts` and `login-lockout` features enable their audit event parsers. In-memory backend tests run without the facade or other drivers.

The `acton-service` facade's `surrealdb` feature preserves service integration, pool actors and compatible error-returning wrappers.
