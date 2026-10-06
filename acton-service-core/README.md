# acton-service-core

Driver-independent connection configuration and structured storage errors shared by the acton-service packages.

The `config` module exposes `DatabaseConfig`, `MssqlAuthMode`, `TursoConfig`, `TursoMode`, `SurrealDbConfig` and `ClickHouseConfig`. `StorageError`, `DatabaseError`, `DatabaseErrorKind` and `DatabaseOperation` describe storage failures without importing a database driver. Connection settings retain the facade's serialization defaults.

Independent adapters use these contracts directly. The `acton-service` facade preserves its existing configuration and error APIs, converting at the adapter boundary. Routing, actors and service lifecycle remain in the facade.
