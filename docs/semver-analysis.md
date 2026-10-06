# Semver Analysis Report

**Generated:** 2026-10-06T23:24:00Z
**Project:** acton-service
**Current Version:** 0.47.0 (uncommitted planned release)
**Last Release Tag:** acton-service-v0.46.0
**Commits Analyzed:** 0 after release; working tree compared to released commit 7fdacbac0b3781e08bc3737eb2ff32d38d3dbca7

## Recommendation

**Suggested Bump:** MINOR (project version-number convention; conservative pre-1.0 compatibility boundary)
**Suggested New Version:** 0.47.0

### Rationale

Keep the proposed 0.47.0 architectural release. Independent packages and additional Cargo features add functionality while compatibility wrappers retain existing facade signatures. Cargo treats a change from 0.46 to 0.47 as a compatibility boundary. If evaluated exclusively as compatible additions under Cargo's pre-1.0 guideline, 0.46.1 would be sufficient; 0.47.0 is conservative and already planned.

## Detailed Analysis

### Major Changes (Breaking)

- None confirmed in existing facade feature combinations after restoring unconditional availability of `acton_service::config::ClickHouseConfig`. The initial extraction added a `clickhouse` feature condition to that previously unconditional item; this was corrected during review. [Cargo guideline: removing public items, including adding conditional compilation](https://doc.rust-lang.org/cargo/reference/semver.html#item-remove).
- `DatabaseConfig` remains facade-owned. Its conditional `mssql_auth` field is preserved, avoiding a breaking addition to a publicly constructible struct when non-MSSQL facade features are selected. [Cargo guideline: adding public fields when no private fields exist](https://doc.rust-lang.org/cargo/reference/semver.html#struct-add-public-field-when-no-private).

### Minor Changes (New Features)

- Seven independently consumable core, audit, and storage packages provide new public entry points.
- Additional public configuration/error re-exports expand item availability. Existing imports remain valid.
- New optional adapter Cargo features and error conversions are additive. [Cargo guidelines: adding public items](https://doc.rust-lang.org/cargo/reference/semver.html#item-new), [adding a Cargo feature](https://doc.rust-lang.org/cargo/reference/semver.html#cargo-feature-add), and [adding dependencies](https://doc.rust-lang.org/cargo/reference/semver.html#cargo-add-dep).

### Possibly-Breaking Changes (Requires Judgment)

- `AuditEventKind` is exhaustive and is now defined in the shared audit package. Enabling `accounts` or `login-lockout` directly on a standalone adapter or audit dependency can expand the variants visible through the facade, even when its corresponding facade feature is disabled. Consumers combining the new packages with facade exhaustive matches should use a unified feature selection. Existing users depending only on the facade retain their previous feature combinations. Adding `non_exhaustive` to the facade enum would itself break existing exhaustive matches. [Cargo guidelines: adding enum variants](https://doc.rust-lang.org/cargo/reference/semver.html#enum-variant-new) and [adding non_exhaustive](https://doc.rust-lang.org/cargo/reference/semver.html#attr-non-exhaustive).
- Public type re-exports change their defining package. Undocumented `type_name` output can change; no well-defined representation guarantees were found for moved types.
- Example names moved to the unpublished integration harness, so old package-specific example commands need the documented replacements. This affects development tooling, rather than facade library signatures.

### Patch Changes (Bug Fixes, Internal)

- Selective PR validation, exact-commit release qualification, nightly coverage, and concurrency changes alter CI behavior rather than Rust public API.
- Example protocol compilation and container test dependencies move into an unpublished integration harness.
- Storage behavior remains unchanged: comparisons of the extracted five storage implementations found no substantive implementation changes.

## Commits Reviewed

| Commit | Summary | Category |
|--------|---------|----------|
| Working tree | Selective CI and independent storage/audit/core extraction | Additive architectural change |

## Files Changed

- `acton-service/src/config.rs`: pool configuration re-exports and facade-to-core database conversion preserve facade struct fields and serde behavior.
- `acton-service/src/error.rs`: database enums re-export from core; facade error and database error types remain facade-owned. Driver classification and messages are preserved by adapter-owned functions; conversion from shared storage errors retains facade categories and context.
- `acton-service/src/audit/{config,event,id,chain}.rs`: extracted definitions are identical aside from internal module paths and helper visibility.
- `acton-service/src/audit/storage/mod.rs`: facade `AuditQuery` fields, defaults, validation, matching, and the complete `AuditStorage` trait compare exactly equal to released source.
- `acton-service/src/audit/storage/{pg,mssql,turso,surrealdb_impl,clickhouse_impl}.rs`: wrappers preserve constructors, `initialize(&self) -> Result<(), acton_service::Error>`, and trait methods. Their inner types remain private.
- `acton-service/src/clickhouse_backend.rs`: existing `AnalyticsWriter` stays facade-owned with unchanged signatures and bounds.

## Notes

- Required local reference `/home/rodzilla/projects/references/rust_semver_guidelines/semver.md` was absent. Fresh official [Cargo SemVer guidelines](https://doc.rust-lang.org/cargo/reference/semver.html) were used as fallback.
- The last tag resolves to HEAD, so there are no post-release commits to categorize. This review covers the actively edited working tree.
- Review is source-based. No Cargo build, Clippy, Nextest, API-diff tool, or source modification was performed by this reviewer. Compilation and test validation belong to the implementation agent.
- New package initial versions and their release ordering should follow the shared workspace version.
