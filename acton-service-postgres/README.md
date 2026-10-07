# acton-service-postgres

Independent PostgreSQL connection pooling and append-only audit storage for acton-service.

`create_pool(&DatabaseConfig)` establishes a SQLx `PgPool` with the configured retry policy. `storage::PgAuditStorage::new(pool)` constructs persistent audit storage; call `initialize().await` to create its schema, indexes and immutability rules. The backend implements `acton_service_audit::storage::AuditStorage` and returns shared `StorageError` values. `database_error` classifies SQLx failures.

The default TLS provider is `crypto-aws-lc-rs`. Select Ring with `--no-default-features --features crypto-ring`. The `accounts` and `login-lockout` features enable their audit event parsers.

This package depends on core contracts, audit logic and SQLx. The `acton-service` facade's `database` and `postgres` features provide the existing service integration and compatible error-returning wrappers.
