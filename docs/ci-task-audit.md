# CI tasks need smaller scopes, stable caches, and stronger backend evidence

Audited commit: `60ebd8a94618077f30e18f2fd22d2671bd881166`.
Evidence: [post-merge run 37550346996](https://github.com/Govcraft/acton-service/actions/runs/37550346996),
all 38 Rust profile logs, workflow definitions, Cargo features and target gates,
test bodies, Taskfile commands, branch protection, and live workflow/cache API state.
The [machine-readable measurements](ci-task-audit.json) accompany this report.
This audit changes documentation only. Recommendations below are not installed.

## What the measurements establish

The run passed all 44 jobs in **17m30s** and consumed **150.6 aggregate job minutes**.
Windows determined the finish time: it started 2m37s after workflow creation and
executed for 14m35s. Queueing, setup, compilation, and cache upload all contribute
to these job durations. They are not test execution times or billed minutes.

The profiles executed 8,669 passing test cases representing 1,204 distinct
package/binary/test identifiers. Repeated identifiers do not prove redundant
coverage: a test body and the library it calls can change under feature flags
or operating systems. Narrow profiles catch missing dependencies and fallback
behavior that `full` can mask.

Compilation dominates the expensive jobs. The SurrealDB adapter spent 4m06s
in Clippy compilation and 6m20s building tests; its six passing tests ran in
0.176s. Linux `full` spent 2m12s in Clippy compilation and 6m06s building tests;
1,034 tests ran in 22.816s, followed by 108 passing doctests in 78.14s.
Filtering a few test names will not remove the underlying compilation cost.

The catalog is a selected set of supported configurations. Running every catalog
entry does not test every possible Cargo feature combination or all advertised
examples. The HTMX example gap below demonstrates that distinction.

The PR default-job log records validation at temporary merge commit
`e12e69f3fbd340968c3bb45c93eb4c70baf13333`. Its GitHub tree SHA and both the PR
head/main merge tree SHAs are `02323df3548c8f44c575297dd1c2866da7c35441`.
This particular post-merge run therefore revalidated the same source tree.
That establishes a concrete opportunity to reuse verified same-tree evidence;
it is not permission to skip validation when another merge changes the tree.

## Fix cache retention before predicting warm-run speed

All 38 profile logs report `No cache found.` Each subsequently completed an
upload. Their combined compressed uploads were **21.356 GiB**. At audit time the
repository retained only **13 caches totaling 9.546 GiB**, all on `main`.
The newly uploaded default, minimal, and OAuth-without-cache entries were absent.
There are no cache-save warnings in the profile logs.

This is consistent with cache eviction, not a sustainably warmed matrix.
GitHub documents a default 10 GB repository limit and eviction of older caches
when new entries exceed the configured limit. PR caches are also scoped to their
merge ref and cannot seed `main` directly.
[GitHub cache reference](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching).
The storage-limit API returned HTTP 402, so the configured numeric limit could
not be independently read. No cache, billing, or repository setting was changed.

Each profile currently caches its own registry and dependency artifacts.
`rust-cache` also excludes workspace crate artifacts by default, which applies
to the newly extracted adapters as well as the facade.
[Action behavior](https://github.com/Swatinem/rust-cache).

Required changes: budget cache storage explicitly, stop copying registry content
into 38 independent entries, and prioritize the profiles used during regular
development. Evaluate a shared registry cache and a few compiler-artifact
families or content-addressed compiler caching. A shared immutable key without
a deliberate writer is insufficient: concurrent jobs do not merge their saved
directories. Enabling all workspace artifact caching indiscriminately would
increase the storage problem. Increasing storage is a separate spending choice,
not an assumption of this recommendation.

The earlier roughly five-minute development estimate remains unverified. Measure
a small representative PR and its merge after cache retention is corrected.

## Each Rust profile has been reviewed

All profiles run Clippy with `--all-targets -D warnings`. Unless marked compile
only, they also run Nextest. "Affected PR" means a change that exercises that
configuration, rather than every PR. Release qualification should retain each
distinct supported configuration after removing true duplicates and excluding
retired products. Standalone package checks catch dependency declarations and
feature unification problems; testing the facade does not run dependency tests.

| Profile | Job time | Passing tests | Value and decision |
| --- | ---: | ---: | --- |
| `default` | 3:01 | 211 | **Keep.** Normal consumer configuration and tracing bootstrap; ten passing doctests. |
| `minimal` | 2:54 | 210 | **Keep.** Absence of default observability dependencies and fallback behavior; equal test names would not make this the same build. |
| `full` | 11:15 | 1,034 | **Keep for qualification and relevant shared changes.** Broad feature interactions, framework examples, and 108 passing doctests. Avoid selecting it for unrelated work. |
| `ring` | 9:13 | 1,034 | **Keep alternative-provider coverage; reduce redundant execution scope.** The same test identifiers as `full` run with a different provider. Retain isolated broad compilation and provider-sensitive runtime tests; most provider-independent tests need not run twice. A name filter alone offers little compilation saving. |
| `windows` | 14:35 | 211 | **Keep native platform evidence; split its scope.** Default native tests cover Windows paths. Additional full, SQL Server, Windows-auth, and gRPC-harness checks are bundled serially even for a narrow affected PR. Select relevant checks for development; retain broad checks for qualification. |
| `audit-turso` | 4:05 | 299 | **Keep.** Facade audit integration with Turso and observability. Does not replace adapter-owned tests. Consider sharing a job/artifacts with `turso-adapter`. |
| `audit-surrealdb` | 10:18 | 375 | **Keep integration coverage; consolidate compilation.** SurrealDB facade plus audit/auth/JWT/observability. Share builds with the adapter where feature sets allow, while retaining an isolated package check. |
| `audit-clickhouse` | 3:14 | 284 | **Keep.** ClickHouse facade integration and its optional configuration/error paths. |
| `audit-otel` | 3:15 | 267 | **Keep.** Audit export with observability but no primary database. |
| `mssql` | 3:21 | 409 | **Keep.** SQL Server facade, auth/accounts/audit, and gRPC integration. Type/unit coverage complements the live container test. |
| `tls-no-grpc` | 3:03 | 375 | **Keep.** HTTP TLS without gRPC/auth assumptions, including live HTTP/TLS regressions. |
| `windows-auth` | 3:08 | 383 | **Keep.** mTLS proxy identity mapping and invalid-configuration behavior. This profile runs on Linux; it is distinct from native Windows validation. |
| `saml` | 3:49 | 308 | **Keep.** SAML without the full auth bundle and a signed assertion round trip. Linux uses a different signature backend from Windows; Windows currently gets compilation only. |
| `grpc-no-tls` | 3:18 | 232 | **Keep.** Transport without TLS/auth enabled; catches optional-feature coupling and router configuration regressions. |
| `grpc-tls` | 4:19 | 506 | **Keep.** gRPC plus TLS/auth/Cedar. Private live RPC tests supply separate connection-level evidence. |
| `oauth-no-cache` | 3:33 | 312 | **Keep.** In-memory OAuth state and no Redis feature. |
| `oauth-with-cache` | 4:07 | 320 | **Keep.** Redis-enabled OAuth/JWT branch. Green tests do not establish a live Redis round trip. |
| `otel-only` | 3:04 | 221 | **Keep.** OTLP metrics without Prometheus, including exporter feature restrictions. |
| `metrics` | 3:29 | 415 | **Keep.** Prometheus and OTLP with TLS, including scraping beside HTTPS. |
| `tokens` | 3:35 | 287 | **Keep.** Auth/JWT without the rest of `auth-full`; password/token behavior. |
| `frontend` | 2:31 | 266 | **Keep, fix example selection.** HTMX/templates/SSE/memory sessions with narrow dependencies. It omits the literal `htmx-full` flag required to compile `task-manager`. |
| `graphql` | 3:46 | 236 | **Keep.** Narrow GraphQL/Cedar transport, authorization, and versioning. |
| `audit-nodb` | 3:04 | 265 | **Keep.** Audit without database or observability; fallback branches differ from `audit-otel`. |
| `core` | 0:32 | 22 | **Keep.** Standalone driver-free configuration/error contracts. Its doctest invocation currently finds zero tests. |
| `core-mssql` | 0:37 | 22 | **Remove.** `mssql = []` has no source/build-script conditional behavior in core. It runs exactly the same 22 tests. The SQL Server adapter already enables the marker, so its existence is validated there. |
| `audit` | 0:50 | 47 | **Keep.** Standalone audit/hash-chain/query contracts without optional event variants. Zero doctests currently. |
| `audit-events` | 0:50 | 47 | **Keep.** Adds account/lockout event variants; test bodies include feature-gated assertions despite having the same identifiers as `audit`. |
| `postgres` | 1:35 | 1 | **Keep standalone validation, repair runtime evidence.** The passing test checks config fields; the real audit persistence/chain test is ignored. Zero doctests currently. |
| `postgres-ring` | 1:19 | 1 | **Keep isolated provider compilation; strengthen behavior.** One config test cannot validate a PostgreSQL TLS connection. The same live audit test is ignored. |
| `mssql-adapter` | 3:06 | 2 | **Keep standalone validation, use the existing container better.** Live audit chain test is ignored; the private container test covers schema initialization/accounts/keys/tokens, not audit corruption checks. Zero doctests currently. |
| `turso-adapter` | 1:31 | 22 | **Keep.** Actual local database persistence, hash-chain, and legacy-fixture behavior. Consolidate dependency compilation with facade checks where safe. Zero doctests currently. |
| `surrealdb-adapter` | 11:01 | 6 | **Keep real tests; consolidate expensive builds.** Includes memory-engine connection, audit corruption, and legacy archive round trips. Its ignored auth test uses an unsupported undefined-root setup and needs a provisioned-server replacement. Zero doctests currently. |
| `clickhouse-adapter` | 1:01 | 24 | **Keep standalone/config/row-contract checks.** No live ClickHouse persistence test was found. Add targeted server evidence before claiming backend qualification. Zero doctests currently. |
| `cli` | 2:39 | 10 | **Keep for changes affecting the maintained legacy tool; remove as a mandatory service release gate.** It is deprecated, unpublished (`publish = false`), and not a service release artifact. Its generated projects are not smoke-compiled. |
| `mssql-integration` | 5:34 | 1 | **Keep.** A real SQL Server container verifies pool health, schema initialization, and account/API-key/refresh-token persistence. Migrate the ignored audit corruption test into this harness. |
| `grpc-examples` | 1:38 | Compile only | **Keep compilation coverage, consider folding into transport jobs.** Compiles/lints all four generated examples, including Cedar without TLS. Other current harness profiles do not cover that precise combination. Nextest installation here is unnecessary. |
| `grpc-integration` | 3:24 | 2 | **Keep.** Live TLS/mTLS RPCs and rejection of a wrong CA under AWS-LC. Does not exercise the public `build_utils` wrapper or the Cedar example. |
| `grpc-integration-ring` | 2:52 | 2 | **Keep.** The same live connection behavior under isolated ring selection. This is valuable provider coverage. |

The seven public component doctest commands currently report **zero doctests**.
They cost fractions of a second after compilation. Consolidating them can retain
automatic coverage when examples are added; removing them saves almost no wall
time and requires a clear policy to enable doctests for future snippets. They are
low-priority cleanup, not the explanation for a 17-minute run.

## Each workflow task and setup step has been reviewed

| Task | Required value | Audit decision |
| --- | --- | --- |
| Change selector and immutable diff | Choose affected configurations from the actual change set; fall back on uncertainty. | **Keep.** Unknown paths/shared contracts require conservative coverage until a dependency map proves a smaller set. |
| Mandatory default/minimal baseline on every code change | Guard common facade behavior and configuration without default observability. | **Required when common code changes, not automatically for isolated code.** A Turso-only adapter edit or feature-gated OAuth edit does not change the code compiled by either baseline. Select them by actual feature/dependency ownership; retain both for shared changes and uncertainty. This saves runner work even when parallel wall time is unchanged. |
| All private-harness profiles on any harness path | Ensure examples and both transport/backends remain valid when shared harness setup changes. | **Narrow by owned files.** A gRPC `.proto` or TLS-test edit currently selects the unrelated SQL Server container test; a SQL Server test edit selects both RPC providers and gRPC examples. Preserve broad selection for shared setup/manifests, and select the transport-specific jobs for isolated files. |
| Exhaustive fallback for framework example edits | Avoid missing conditional example compilation. | **Use Cargo target metadata where ownership is known.** Current example changes select the entire matrix. Select configurations that actually compile the changed target and its conditional branches; keep full fallback for unknown inputs/templates until classified. |
| CI helper regression tests in `changes` and `format` | Prove selectors, skip rules, publication ordering, and SHA/registry safeguards. | **Keep coverage; duplicate invocation is unnecessary within one development run.** Format retains independent reusable-workflow coverage. Consolidate without weakening docs-only or qualification preflight; negligible runtime saving. |
| PR `edited` trigger | Reevaluate `[full-ci]` in title/body. | **Reduce.** Every ordinary description edit currently cancels/restarts the same expensive diff. Prefer marker application on the next push plus explicit manual full validation. A skipped gate from a metadata-only run must not stand in for the real pending gate. |
| Push validation on `main` | Test integration when the actual merged tree differs from the tested PR tree. | **Conditional value.** Branch protection has `strict=false`, so this cannot simply be removed. Reuse verified same-tree PR evidence or a merge-queue policy; otherwise run affected integration checks. Publication qualification remains independent. |
| Rustfmt | Workspace formatting contract. | **Keep.** Cheap and distinct from compilation/linting. |
| Actionlint | Validate workflow syntax, expressions, and shell usage. | **Keep where workflow/CI tooling changes; qualification also validates wiring.** It currently runs on every code PR; 25s total format-job cost is small. Do not lose linting on docs-deployment workflow changes, which currently skip the Rust workflow. |
| Ruff lint and format | CI helper correctness/style. | **Keep for helper changes and qualification.** Distinct lint/format checks; small cost. |
| Clippy `--all-targets -D warnings` | Lint active library, tests, binaries, and eligible examples. | **Keep.** Does not execute tests or compile examples whose literal `required-features` are absent. Avoid redundant `cargo check` for exactly the same target/platform/features. |
| Nextest | Execute runtime assertions with declared features. | **Keep for supported behavioral configurations.** Repeated cheap runtime tests are less important than repeated test-binary code generation. Never treat ignored tests as passing backend evidence. |
| Facade default/full doctests | Compile and run public API examples. | **Keep.** 10 and 108 pass respectively; 34 and 150 are ignored. Their green result makes no claim about ignored or website snippets. |
| Protoc installation | Generate private harness protobufs. | **Keep for harness profiles.** Facade-only full/ring/gRPC/SQL Server profiles no longer generate protobufs; their sole `build_utils` unit test discovers files. Remove unused installs there, or add a test that actually exercises public proto generation and install it only for that test. Windows still needs it for its harness check. |
| Kerberos native installation | Build Unix SQL Server integrated-auth dependencies. | **Keep for SQL Server configurations.** Required by the driver even when a particular test uses SQL credentials. |
| Toolchain/Clippy setup | Execute the declared compiler and lint contract. | **Keep.** Consider a controlled pinned toolchain with deliberate upgrades to avoid cache-wide invalidation; this changes the current stable-toolchain policy and needs its own decision. |
| Python setup and exact-SHA guards | Execute shared scripts consistently and reject revision mismatch. | **Keep.** Cheap guards preserve exact-commit qualification/publication. A setup-only optimization must not remove the SHA checks. |
| Nextest installation | Supply the test runner without compiling tooling. | **Keep for test profiles; skip for `grpc-examples`.** That profile never invokes it. |
| Dependency cache restore/save | Avoid rebuilding unchanged dependencies. | **Keep the purpose; redesign storage.** All 38 cold starts, 21.356 GiB uploads, and missing fresh entries show the current layout is not retaining its working set. |
| Persistent self-hosted target isolation | Avoid simultaneous incompatible profiles sharing build output. | **Keep when self-hosted execution is selected.** A single runner still serializes the matrix; qualification correctly uses hosted fan-out. |
| Documentation validation | Catch broken Next.js/Markdoc exports at the candidate revision. | **Keep on relevant PRs and releases.** Main also builds the exact same site for Pages deployment. Both measured Next.js builds took 38s; share one build artifact and let deployment consume it only after validation succeeds. |
| Pages upload/deployment | Publish the validated documentation site. | **Keep as a main-only deployment action.** It currently runs independently of `ci-gate`; publication may precede failed code validation. Preserve the successful gate when combining builds. |
| `cargo audit` | Lockfile-wide RustSec reporting, including dependencies inactive in a feature graph. | **Keep.** It is not exactly duplicated by graph-based `cargo deny`: the existing lockfile-only `rkyv` exception demonstrates different scopes. |
| `cargo deny` advisories/licenses/bans/sources | Enforce dependency graph policy, including optional features. | **Keep.** Distinct from tests and lockfile audit. Current duplicate-version warnings are allowed by policy, not suppressed lint failures. |
| SBOM generation/upload | Produce the complete dependency inventory for qualification/publication. | **Keep for release evidence.** Generating it on every dependency PR and nightly is optional for development; combined security job took only 25s. It is not the runtime bottleneck. |
| Weekly security schedule | Detect newly disclosed advisories without a code change. | **Keep scheduled coverage; consolidate schedule.** Nightly qualification already includes this job, making the extra Monday run redundant if nightly runs reliably. |
| Rust validation gate | Propagate every matrix failure/cancellation to callers. | **Keep.** Five seconds; required for independent reusable-workflow completion. |
| Development `ci-gate` | Enforce exact selected success/skip expectations under branch protection. | **Keep.** The repository explicitly requires this check. Seven seconds; it is not duplicate optional decoration. |
| Nightly full qualification | Check uncommon combinations and changed upstream tool/advisory state outside regular PR scope. | **Keep once per day.** Avoid repeating a qualification at the same SHA merely to warm caches; its cache writes must not evict normal development coverage. |
| Packaged publication dry run | Compile registry-style packages, check shipped fixtures/files and dependency order. | **Keep in release qualification.** Workspace tests cannot prove archive completeness or independent registry dependency resolution. The protobuf package install is currently unnecessary because this excludes the private generating harness; Kerberos remains needed. |
| Qualification gate | Require Rust, docs, security, and packaged verification at one SHA. | **Keep.** This is the release decision, not the development decision. |
| Release tag/version/main-ancestry resolution | Establish the exact publication candidate. | **Keep.** The description says signed tag, but CI does not verify its signature; signing currently occurs in local release preparation. Document that distinction or add actual verification with provisioned trusted keys. |
| First-publication bootstrap | Create new crate names before trusted publication is available. | **Keep as a temporary first-release path.** Not a development check. |
| Publication preflight and archive/SHA verification | Reject another commit's existing version; safely resume partial publication. | **Keep.** Both bootstrap and final publish need their own preflight because registry state can change between jobs. |
| OIDC credential acquisition after qualification | Obtain short-lived publication credentials only when publishing can proceed. | **Keep.** Building again with the credential active would waste its lifetime; current `--no-verify` publication relies on the earlier packaged qualification. |
| Claude review | Advisory AI review, not a deterministic merge requirement. | **Already disabled manually.** Last executions were in July. It contributes no time to this run; leave disabled unless its outcomes justify a separate opt-in policy. |
| Docs sync on merge | Propose documentation corrections for changed behavior. | **Already disabled manually.** Its five latest runs failed at the model action. It contributes no time now. Restore only with a working bounded process and appropriate documentation preflight; do not call it required release validation. |
| Claude mention handler | Respond to explicit `@claude` requests. | **Keep as opt-in assistance.** Active, but not scheduled for ordinary PRs and not a merge gate. |

## Each local Taskfile task has been reviewed

These tasks are opt-in developer commands, not jobs launched by `Build & Test`.
Removing them will not shorten hosted CI.

| Task | Value and decision |
| --- | --- |
| `build` | **Keep for local CLI use; repair freshness inputs.** Sources include CLI Rust/manifest only, but the binary depends on the facade/core, the workspace manifest/lock, and embedded templates. Changes to those can incorrectly skip a required rebuild. |
| `install` | **Keep opt-in.** Depends on build and replaces the local CLI. Correctness depends on fixing build freshness. Not a service CI requirement. |
| `clean` | **Keep as a recovery command.** It discards useful artifacts and must not be part of normal CI or release preparation. |
| `uninstall` | **Keep opt-in.** Removes the locally installed CLI; no CI purpose. |
| `changelog` | **Keep for release preparation.** Writes the changelog once; not a per-profile validation task. |
| `changelog-preview` | **Keep.** Read-only full/ranged preview, useful before writing a release changelog. |
| `changelog-unreleased` | **Optional convenience.** Equivalent to preview with `--unreleased`; keeping an alias costs no CI time. |
| `audit` | **Keep.** Matches CI's lockfile advisory check. |
| `deny` | **Keep; align scope and documented arguments.** Local command lacks CI's `--all-features`; its documented `task deny -- check advisories` renders a duplicate `check`. Use `task deny -- advisories` or alter the argument contract. |
| `sbom` | **Keep; label inventory scope accurately.** Local command omits CI's `--all-features --target all` and therefore does not generate equivalent release inventory. |
| `security` | **Keep as a convenience wrapper.** Runs audit, deny, and inventory once each; align deny/SBOM flags before treating it as CI-equivalent evidence. |
| `release-cli` | **Retire or rename.** The CLI is deprecated and cannot publish. A non-executing Cargo-release dry run confirmed that its patch bump would advance the workspace and all public packages to 0.47.1, despite the CLI-only description. A local install does not require a release tag/push. |
| `release-service` | **Keep.** Signed version/changelog/tag preparation followed by gated exact-commit publication. It is the intended supported release entry point. First-component bootstrap remains a separate documented dispatch. |
| `default` | **Optional convenience, reconsider the product it serves.** It builds/installs the deprecated CLI. It is not a service development check and should not be described as one. |

## Correct missing coverage before claiming tasks establish readiness

1. **HTMX example:** no profile enables the literal `htmx-full` feature required
   by `task-manager`. `full` and `frontend` enable its constituent features, which
   does not activate the umbrella feature itself. Add the umbrella to frontend
   validation and assert that every maintained example has an eligible profile.
2. **PostgreSQL persistence:** provide a dedicated ephemeral database and run the
   ignored audit persistence/corruption test deliberately. One passing config
   test under each crypto provider establishes compilation, not database behavior.
3. **SQL Server audit:** reuse the existing container to execute the currently
   ignored audit corruption test. Schema initialization/account persistence do
   not prove audit chain persistence and detection of tampered rows.
4. **SurrealDB authentication:** replace the ignored undefined-root memory-engine
   test with a supported authenticated server setup. Keep the working memory
   persistence tests.
5. **ClickHouse persistence:** add a bounded server-backed round trip for the
   advertised audit storage. The 24 tests establish config/row contracts.
6. **SAML on Windows:** broad native Windows validation compiles the alternative
   RustCrypto XML-signature backend but does not run the signed-assertion test
   there. Include that small behavioral scenario during qualification.
7. **Public proto helper:** harness protobuf generation calls `tonic-prost-build`
   directly. The public `build_utils` helper's unit test only discovers files.
   Add a generated-descriptor smoke test for the actual public helper if that API
   remains supported, then install protoc only for checks that use it.

## Recommended order of work

1. Fix cache layout and measure a small PR. The current working set cannot remain
   warm under the observed retention behavior. Preserve dependency/provider
   fingerprints and define who writes each shared cache.
2. Narrow selection for owned optional modules, individual adapters, private
   harness transports, and Cargo examples. Mandatory baseline builds and
   unrelated transport tests are unnecessary when their compiled inputs are
   proven unchanged. Keep conservative shared/unknown-input behavior.
3. Remove `core-mssql`; avoid unused protoc/Nextest installs; fold duplicated
   documentation builds into a validated Pages artifact. These are demonstrable
   duplicated or unused work. Zero-doctest invocations and the second 41-test
   helper run are much lower priority.
4. Separate default Windows behavior from broad Windows compile configurations,
   selecting only relevant platform checks for PRs. Keep complete native
   qualification; do not delete platform confidence to reach a timing target.
5. Consolidate adapter/facade builds, starting with SurrealDB. Run both packages'
   tests within a shared compatible build while retaining independent package
   compilation or packaged qualification to detect feature leakage.
6. Narrow ring runtime coverage to provider-sensitive behavior with a complete
   compile check. Keep live RPC/HTTP TLS evidence under both providers.
7. Remove deprecated CLI behavior from service publication criteria, retain
   targeted legacy checks when its code/API dependencies change, and resolve
   the CLI-only release task's shared-version side effect.
8. Repair the missing backend/example scenarios above. Use small targeted jobs
   on affected changes and qualification, not another unconditional PR matrix.
9. Reduce metadata-only reruns and reuse main validation only when the tested and
   merged trees are proven equivalent. Otherwise retain affected integration
   checks while branch protection permits a stale tested base.

Acceptance evidence for follow-up changes: every maintained example is selected;
each claimed live backend check actually executes; provider/platform differences
remain represented; intentionally skipped jobs cannot satisfy a required gate;
package verification still runs at the published SHA; and a normal PR's measured
latency/cache hit rate improves. Compare both wall time and aggregate runner
minutes. No faster-CI claim should rely solely on reducing the job count.
