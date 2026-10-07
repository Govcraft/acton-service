# Reduce development CI work while preserving release evidence

The measured baseline is 17m30s and 150.6 aggregate job minutes. This change
implements the accompanying task audit, without publishing a release or changing
paid cache settings. The pending workspace version remains 0.47.0: these are CI
and test changes to an unreleased version, not another public API release.

## Workflow architecture

- Select profiles by owned feature, backend, harness file, and example target.
  Keep conservative fallback for shared contracts, manifests, and unknown paths.
  Remove automatic default/minimal checks for isolated optional code.
- Reuse successful development evidence only for an identical Git tree, matching
  selected coverage and validation configuration. Main still runs affected checks
  if no trusted completed run provides that evidence. Release qualification always
  validates its exact commit and packaged archives independently.
- Remove metadata-edit cancellation. Keep manual full validation and the full-ci
  marker on the next source update.
- Use one registry cache and a bounded compiler cache working set. Rare exhaustive
  profiles restore compatible cache entries but do not upload 38 competing caches.
  Keep only deliberate development writers and enforce a storage budget.
- Remove core-mssql, separate deprecated CLI from mandatory release qualification,
  split native Windows checks, narrow ring runtime while retaining broad ring
  compilation, and consolidate compatible adapter/facade compilation.
- Keep all supported configurations and strict gates. Run helper tests once in
  each workflow invocation. Validate workflow-only changes even when Rust is skipped.
- Build documentation once, upload the output, and deploy that output on main only
  after ci-gate succeeds. Remove the redundant weekly security schedule while
  retaining daily qualification and independent manual security checks.
- Install protoc only for protobuf generation; install Nextest only for runtime
  profiles. Keep Kerberos for SQL Server. Align Taskfile security scope, CLI build
  freshness, default service checks, and retire the misleading release-cli task.

## Rust coverage contracts and ownership

Rust worker `sql_coverage` owns PostgreSQL/MSSQL integration tests, integration
Cargo.toml dependency/feature edits, and Cargo.lock coordination. Reuse bounded
container fixtures to verify audit round trips, hash-chain integrity, and corruption
rejection. PostgreSQL checks must include a real TLS connection under both providers.

Rust worker `backend_coverage` owns SurrealDB/ClickHouse integration test files and
public protobuf-helper regression coverage. Use a provisioned authenticated
SurrealDB server and bounded ClickHouse persistence. Exercise the public helper
with generated descriptors. Request manifest changes from sql_coverage rather
than concurrently editing dependency files. No production API changes are intended.

Both workers first inspect current APIs and write a scoped plan before implementing.
Use existing errors and identifiers; no new production domain types are needed.
Run cargo check, Clippy with denied warnings, and Nextest for their configurations.
Fix underlying problems without lint suppressions. They share an isolated worktree
and must accommodate each other's changes.

The root owns Python orchestration, workflow files, profile catalog, Taskfile,
example-coverage assertions, SAML profile selection, and CI documentation. Frontend
validation enables literal htmx-full. Native Windows SAML executes the signed
assertion regression. Public component doctests are consolidated without dropping
future snippets. Signature policy documentation distinguishes signed release
preparation from the checks actually performed by CI.

## Acceptance evidence

Behavioral helper tests must cover precise selection, every maintained example,
qualification completeness, cache budget/writers, and fail-closed same-tree reuse.
Actionlint, Ruff, rustfmt, relevant Clippy and runtime tests must pass. Push a signed
Conventional Commit to a reviewable PR, validate the optimized full matrix, then
measure representative narrow hosted runs and report latency and aggregate work.
Do not merge this PR or publish crates without user authorization.
