# Development checks and release qualification

Pull requests validate the configurations their changes can affect. Releases
qualify every supported configuration at the exact commit being published.

## Development

`Build & Test` selects profiles from the changed paths. Rust changes always run
default and minimal Linux checks, then the relevant package and feature checks.
Each selected profile runs Clippy with warnings denied and Nextest; designated
profiles also run doctests. Documentation builds when its site sources change.
Dependency changes also run security policy checks.

Examples:

| Change | Profiles beyond default/minimal |
| --- | --- |
| OAuth state/provider implementation | OAuth with and without cache |
| gRPC implementation | gRPC with and without TLS, examples, live TLS harness tests |
| Turso adapter | Standalone Turso, facade audit with Turso |
| SQL Server adapter | Standalone SQL Server, facade integration, live container test, Windows |
| Core/audit contracts, shared config/builder/state, dependency manifests | All profiles |
| Unknown file or unreliable diff | All profiles |

Use `[full-ci]` in a PR title or body to request all profiles. The old
`[skip-matrix]` marker no longer bypasses Rust checks. Documentation-only changes
still skip Rust checks intentionally.

The required `ci-gate` check verifies that selection succeeded and every selected
job passed. A missing output, failed/cancelled job, or unexpected skip fails the
gate. The existing branch-protection check name is retained.

Pull requests test GitHub's merged revision. Pushes to `main` check the combined
code again, so independently green PRs cannot hide an integration failure.
New commits cancel earlier runs for the same PR. Main and release qualification
runs finish independently.

Inspect selection and run a profile locally:

```sh
python3 scripts/ci/plan.py --paths acton-service/src/auth/oauth/state.rs
python3 scripts/ci/run_profile.py oauth-no-cache
python3 scripts/ci/run_profile.py turso-adapter
python3 -m unittest discover -s scripts/ci/tests -v
```

The profile catalog is `scripts/ci/profiles.py`. Add a profile and path mapping
when introducing a subsystem, and include selection tests for the new boundary.
Unknown source paths deliberately receive exhaustive validation.

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

The SQL Server integration profile requires Docker. gRPC examples require
`protoc`. Linux SQL Server checks require the Kerberos development libraries.

## Qualification and publishing

`Release qualification` runs nightly at 05:00 UTC and can be dispatched manually
with an exact commit SHA. It runs the complete Rust catalog, Windows checks,
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
GitHub workflow. It does not publish directly from a developer machine.

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
