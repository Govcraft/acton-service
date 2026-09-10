# Audit investigation queries

Add a backward compatible, bounded AuditStorage::query_filtered operation with explicit unsupported default for custom adapters. AuditQuery uses existing event kind, severity, timestamps and sequence representations. Filters include exact subject, request, service, status and normalized resource metadata. Exclusive sequence cursors and an inclusive snapshot ceiling keep paging stable; default newest-first, maximum 1000 rows. Validate limits and numeric storage ranges before I/O. Parameterize all filter values in PostgreSQL, SQL Server and SurrealDB; forward through lazy storage.

Add AuthTokenValidated with its own wire spelling; retain existing event spellings and legacy hash encodings. Emit it on bearer middleware success. Enrich denial producers with safe request metadata without logging tokens. Validate behavioral query boundaries, kind round trips and legacy hash fixtures. Run nextest and clippy across affected features, including backend integration tests when available. Additive public API warrants workspace minor version 0.43.0; coordinate publication separately.

Validation found duplicate shared imports when SQL Server and Redis-backed authorization are enabled together. Consolidate the pool module imports under a single feature union without changing actor behavior, then rerun the affected feature matrix.

The combined SurrealDB/JWT validation matrix exposed ambiguous transitive JWT crypto features. Explicitly install the workspace JWT provider at signing and validation boundaries while preserving an application-installed provider; rerun the existing token regression and complete audit suite.
