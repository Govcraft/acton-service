# Development checks and release qualification

Pull requests validate the configurations their changes can affect. Releases
qualify every supported configuration at the exact commit being published.

## Development

`Build & Test` selects configurations from owned feature modules, backend packages,
private harness files, and declared example targets. Resolved backend graphs are
separate: SurrealDB profiles do not enable PostgreSQL, SQL Server, or ClickHouse,
and each other backend profile selects its own optional driver. Before compilation,
a resolved Cargo graph guard rejects mixed database drivers and requires exactly
the selected SQLx TLS provider. The shared harness
uses per-backend feature flags, including per-backend container modules. Isolated optional changes do
not rebuild default/minimal consumers whose inputs have not changed. Shared
contracts, manifests, and unknown files retain conservative complete selection.
Stable workspace version-only edits use semantic manifest/lockfile comparison:
only workspace versions and compatible local version requirements may change.
External dependencies, checksums, feature flags, and all other configuration must
match. This path requires `cargo metadata --locked` and rebuilds versioned docs;
it skips compilation and repeated dependency-policy work. Malformed or ambiguous
changes fall back to normal validation. Release qualification remains exhaustive.
Each behavioral profile runs Clippy with denied warnings and Nextest. Compile-only
profiles retain provider/platform coverage; designated profiles also run doctests.
The seven component crates currently have no Rustdoc examples, so they skip empty
doctest builds. Selection tests require an explicit doctest profile when a
component adds documentation code, including indirect or block documentation.
For composite jobs, doctests use the combined feature graph to avoid rebuilding
the backend SDK under a standalone graph.

| Change | Selected work |
| --- | --- |
| OAuth implementation | OAuth with and without cache |
| gRPC implementation | Transport with/without TLS, examples, both provider RPCs, native Windows compilation |
| Turso adapter | Isolated adapter lint, adapter tests and facade integration in one job |
| SQL Server fixture | That live container test |
| PostgreSQL adapter | Standalone adapter, both provider TLS/database scenarios and facade integration |
| HTMX example | Frontend including the literal htmx-full feature |
| Shared contracts/manifests or unknown source | Complete development catalog, including dependent legacy CLI |

Use `[full-ci]` in a PR title/body before the next push, or dispatch `Build & Test`
for immediate complete development validation, including the legacy CLI. Editing PR descriptions does not
cancel an in-progress source validation. The old `[skip-matrix]` marker cannot
bypass checks. Documentation-only changes skip Rust; workflow-only documentation
changes still run Actionlint and CI helper lint/format checks.

The required `ci-gate` verifies successful selection and every selected job.
Missing outputs, failures, cancellations, and unexpected skips fail the gate.
New commits cancel earlier feedback for that PR; qualification retains its own run.
The Rust matrix cancels sibling jobs at its first failure. Nextest 0.9.146 is
downloaded as a checksummed prebuilt binary with fallback compilation disabled;
compile-only profiles do not install it. Cargo still compiles the project test
binaries that Nextest executes.

Main reuses a successful same-repository PR run only when GitHub's commit-tree
API and retained evidence agree with the actual merged Git tree, the recorded
coverage contains every selected profile/check, and required docs artifacts still
exist. Failed, incomplete, fork, expired, or insufficient runs are rejected.
Unavailable evidence falls back to affected merge validation. Qualification does
not reuse this development evidence: publication requires fresh exact-commit
checks and packaged verification. Documentation is built once and Pages consumes
that validated artifact only after ci-gate succeeds on main.

Inspect selection and run a profile locally:

```sh
python3 scripts/ci/plan.py --paths acton-service/src/auth/oauth/state.rs
python3 scripts/ci/run_profile.py oauth-no-cache
python3 scripts/ci/run_profile.py audit-turso
python3 -m unittest discover -s scripts/ci/tests -v
```

The profile catalog is `scripts/ci/profiles.py`. Add a profile and path mapping
when introducing a subsystem, and include selection tests for the new boundary.
Unknown source paths deliberately receive exhaustive validation.

## Compiler cache policy

CI pins Rust 1.99.0 in rust-toolchain.toml and workflow setup. Upgrade these together
and review the new compiler in qualification. CI disables incremental compilation
and debug info for development/test profiles, reducing upload size and codegen work.
Local builds retain their usual debugging configuration.

One preflight job populates a shared lockfile-keyed registry cache. Six deliberate
compiler writers populate default, full, ring, SurrealDB, frontend, and Windows
families; other jobs restore compatible artifacts without racing to save partial
snapshots. Dependencies and unchanged workspace libraries are retained, including
the expensive SurrealDB adapter. Test executables, docs, and incremental output are
excluded. A writer skips oversized uploads above 4 GiB uncompressed; the measured
full-profile footprint is 2.3 GiB before compression. Retention
keeps the newest entry per family/ref within an 8 GiB total CI budget and removes
obsolete v0-rust caches; unrelated caches are untouched. This leaves room within
the observed repository storage for documentation/dependency tooling. Cache misses
always compile normally and never weaken validation. Live SQL Server, SurrealDB, and ClickHouse scenarios run in their composite backend
jobs during qualification; their fixture-only profiles remain available for narrow
fixture edits. The ring cache writer uses narrow TLS runtime artifacts, while the
broad ring compile profile restores them. Qualification cannot fill
storage with 38 independent registry/compiler copies.

`task` now runs normal service formatting, lint, and runtime checks. Legacy CLI
build/install remains opt-in, its freshness includes shared source and embedded
templates, and release-cli was retired because its inherited workspace version
could bump service packages. Local security tasks use CI's full dependency scope.

## Component packages

The public `acton-service` facade retains its existing module paths, feature
flags, storage traits, and error types. It owns integration wiring and actor
lifecycle. Its wrappers translate driver-independent errors at the boundary.

`acton-service-core` owns shared storage configuration and structured errors.
`acton-service-audit` owns audit events, hashing, query types, and storage
contracts. Neither depends on a database driver or the facade. PostgreSQL,
SQL Server, Turso, SurrealDB, and ClickHouse each have an independent adapter
package with connection construction, audit storage, and backend tests.

Cargo unifies features of the shared audit package. An application using both
the facade and a standalone adapter with `accounts` or `login-lockout` enabled
will see those additional `AuditEventKind` variants through the facade as well.
Enable the corresponding facade features when mixing these packages, and update
exhaustive enum matches if adopting this new combination.

Moved implementations emit tracing events under their component module targets.
The broad `acton_service` filter also covers these names. Filters targeting a
specific old module should include its new target, for example
`acton_service_postgres::database` for PostgreSQL connection construction.

Database-specific actor orchestration and authentication/account integration
remain in the facade. Cross-subsystem integration checks therefore remain
necessary even when an adapter's own unit tests pass.

The private `acton-service-integration-tests` package contains container tests
and gRPC examples. It is never published. Protobuf example generation and the
SQL Server container dependency are kept out of ordinary facade compilation.

```sh
cargo nextest run -p acton-service-turso --locked
cargo nextest run -p acton-service-integration-tests --features mssql --locked
cargo nextest run -p acton-service-integration-tests --features grpc,tls --locked
cargo nextest run -p acton-service-integration-tests --no-default-features --features grpc,tls,crypto-ring --locked
cargo run -p acton-service-integration-tests --example ping-pong --features grpc
```

Live PostgreSQL, SQL Server, SurrealDB, and ClickHouse integration profiles require Docker. gRPC examples require
`protoc`. Linux SQL Server checks require the Kerberos development libraries.

## Qualification and publishing

`Release qualification` runs nightly at 05:00 UTC and can be dispatched manually
with an exact commit SHA. It runs the complete service catalog, split native Windows checks,
doctests, documentation, dependency advisories/license/source policy, SBOM
generation, and a workspace publication dry run.

Exhaustive jobs use hosted runners so one self-hosted runner cannot serialize
the release matrix. Development Linux jobs can use `ACTON_LINUX_RUNNER` for
same-repository work. Fork PRs use hosted runners. Persistent local Cargo
artifacts are isolated by profile to prevent Cargo lock contention.

`Publish acton-service` resolves a release tag once, verifies the workspace
version and main ancestry, and passes its immutable SHA to qualification.
Publishing depends on successful qualification and checks out that same SHA.
Authentication occurs after validation. A green run at another SHA is not
release evidence, and qualification failures or skips prevent publication.

The release helper derives publishable packages and their dependency order from
Cargo metadata. Private packages are excluded; path dependencies must have
registry version requirements. Cargo's multi-package dry run validates new
interdependent packages together before any upload. Actual publishing avoids
rebuilding the already qualified packages after obtaining a short-lived token.

```sh
python3 scripts/ci/release.py order
python3 scripts/ci/release.py dry-run --allow-dirty  # local review only
gh workflow run qualification.yml --ref main -f sha=COMMIT_SHA
gh workflow run release.yml --ref main -f tag=acton-service-v0.47.0
```

`task release-service -- minor` prepares signed public-workspace release commits
and tags with `cargo-release --no-publish`, pushes them, and dispatches the gated
GitHub workflow. It does not publish directly from a developer machine. CI validates tag format,
version, and ancestry; it does not verify a tag signature against a trusted key
registry. Signing is currently enforced by local release preparation.

For the first component release, crates.io requires an API token because trusted
publishing can only be configured after a crate exists. Set the GitHub secret
`CRATES_IO_BOOTSTRAP_TOKEN` to a short-lived token scoped to the seven new package
names. Dispatch `release.yml` with `bootstrap-components=true`. After exhaustive
qualification succeeds, this option publishes the components with that token,
then publishes the facade with its existing OIDC configuration. The bootstrap
token is never used to publish the facade.

After that first release, configure trusted publishing on the new packages using
GitHub owner `Govcraft`, repository `acton-service`, workflow `release.yml`, and
no GitHub environment; revoke/remove the bootstrap token. Subsequent releases
use the default OIDC-only route. The existing facade configuration does not
automatically grant rights to new package names.

Retries skip a published version only after verifying its registry checksum and
embedded clean Git revision match the qualified SHA. A version published from
another commit aborts the release before any upload. All public workspace
packages share the release version; the private CLI and integration harness
are excluded from publication.

The release tradeoff is explicit: an uncommon combination may fail at nightly
qualification after a focused development PR has passed. That failure must be
repaired before the commit can be published.
