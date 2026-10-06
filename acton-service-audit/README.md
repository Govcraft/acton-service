# acton-service-audit

Audit events, TypeID identifiers, BLAKE3 hash chains and persistence contracts, usable without an HTTP framework or database driver.

Use `AuditEvent`, `AuditEventKind`, `AuditSeverity`, `AuditSource` and `AuditChain` to construct and seal events. `verify_chain` checks tampering and sequence continuity. The `storage` module provides bounded `AuditQuery` filters, verification results, `AuditStorage` and deferred schema initialization contracts. Storage operations return `acton_service_core::StorageError`.

The `accounts` and `login-lockout` features expose the corresponding event variants. The `acton-service` facade reexports event and chain types and retains its original storage trait, middleware, actors and service error type. Independent database adapters implement this package's persistence contracts.
