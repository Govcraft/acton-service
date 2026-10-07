# acton-service-turso

Independent Turso/libsql connection management and append-only audit storage for acton-service.

`create_database(&TursoConfig)` supports local SQLite, remote connections and embedded replicas with the existing retry and synchronization settings. `storage::TursoAuditStorage::new(Arc<libsql::Database>)` constructs audit storage; call `initialize().await` to create its schema and immutability triggers. `database_error` classifies libsql failures.

The backend implements `acton_service_audit::storage::AuditStorage` and returns shared `StorageError` values. The `accounts` and `login-lockout` features enable their audit event parsers. Local backend tests run without another database driver or the facade.

The `acton-service` facade's `turso` feature preserves service integration, pool actors and compatible configuration/error APIs.
