# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Security

- **tls**: A `[tls]` or `[grpc.tls]` section with `enabled = true` whose
  certificate or key fails to load is now a hard startup failure. Previously
  the loader logged the error and returned `None`, and the listener came up
  in **plaintext** on whatever bind was configured — including a
  non-loopback one — while the application believed it was serving TLS. A
  section that says `enabled = true` is the operator's explicit statement of
  intended posture; silently serving a weaker posture than configured
  inverts the fail-safe direction. (#41)
- **auth**: Invalid PASETO or JWT token configuration is likewise fatal.
  It previously logged a warning and *skipped the authentication middleware
  entirely*, so a typo in the token config silently published every
  authenticated route unauthenticated. (#41)

### Features

- **builder**: `ServiceBuilder::try_build()` returns
  `Result<ActonService<T>>`, reporting the misconfigurations above at build
  time. The existing infallible `build()` is unchanged and remains the
  ergonomic path — it defers the same error to `serve()`, which now returns
  it before binding any listener. (#41)
- **tls**: `ServiceBuilder::with_tls_config()` and
  `with_grpc_tls_config()` accept a pre-built
  `Arc<rustls::ServerConfig>`. An application that has already loaded and
  validated its key material can hand the builder exactly the object it
  checked, eliminating the second read of the cert files and with it the
  time-of-check/time-of-use window in which renewal hooks or permission
  changes could alter the material between validation and listen. When set,
  the override wins over the corresponding config section. (#41)

### Notes

- The strict behavior is the default rather than an opt-in flag: there is no
  coherent posture in which "attempt TLS, but plaintext is acceptable" is
  the intended outcome of an enabled TLS section. Deployments that were
  unknowingly relying on the degrade will now fail to start — which is the
  point, and the failure names the cert path that could not be loaded.

## [acton-service-v0.28.0] - 2026-07-17

### Features

- **config**: Both the HTTP and gRPC listeners now honor a configurable
  bind address. `[service] bind` accepts any `IpAddr` (`0.0.0.0`,
  `127.0.0.1`, `::1`, …) and defaults to `0.0.0.0` for backward
  compatibility, so downstream services can finally expose a loopback-only
  surface without hand-rolling their own listener. `[grpc] bind` overrides
  the service-level bind for the separate-port gRPC listener and falls back
  to it when unset (`GrpcConfig::effective_bind`). (#38)
- **grpc**: Per-listener TLS for the separate-port gRPC surface via
  `[grpc.tls]` (requires the `tls` feature). When the section is present it
  is authoritative: `enabled = true` terminates TLS with its own
  certificate/key independently of the HTTP listener, `enabled = false`
  serves plaintext gRPC even when the shared `[tls]` is active (e.g. a
  loopback-only sidecar surface). When the section is omitted the gRPC
  listener falls back to the shared `[tls]` config, preserving prior
  behavior. Bad gRPC certificates are reported at build time. (#38)

### Notes

- Adding `bind`/`tls` fields to the public `ServiceConfig`/`GrpcConfig`
  structs is source-breaking for consumers that build them with a struct
  literal (no `#[non_exhaustive]`); hence the minor (0.x breaking) bump.
  Config files and deserialization are unaffected — every new field is
  optional or defaulted.

## [acton-service-v0.27.1] - 2026-07-11

### Fixes

- **config**: `config.example.toml` shipped an uncommented nested
  `[token.jwt]` table that fails deserialization with `missing field
  'format'`. `TokenConfig` is internally tagged on `format`, so the only
  parseable form is a flat `[token]` table with `format = "paseto" | "jwt"`
  and the variant fields inline. Both token examples are now commented out
  and rewritten in the flat form, and three regression tests lock the wire
  format: the tagged form round-trips through Figment, the nested table
  form is rejected, and `config.example.toml` itself must load under
  default features. (#31)

### Documentation

- **readme**: Refreshed the crate README from its ~v0.2 state to the
  current feature set: config-driven middleware, PASETO-first token
  authentication, the full auth/session/audit stack, Turso/SurrealDB/
  ClickHouse backends, the grouped feature-flag inventory, all bundled
  examples, and the real CLI surface. crates.io now renders this README.
  (#30)
- **site**: Remediated a full staleness audit of all 54 documentation
  pages against the v0.27 API: rewrote nine pages whose samples could not
  compile, corrected inverted readiness semantics and fictional config
  sections, added a SurrealDB page, and fixed two Markdoc rendering bugs
  that truncated code fences and leaked version helper tags. (#31)

### Internal

- **metadata**: The `homepage` key for both `acton-service` and
  `acton-cli` now points to the documentation site
  (https://govcraft.github.io/acton-service/) instead of the GitHub
  repository. `repository` is unchanged.

## [acton-service-v0.27.0] - 2026-05-28

### Breaking changes

- **audit**: The PASETO and JWT auth middleware no longer emit
  `AuthLoginFailed` (`auth.login.failed`) for unauthenticated or
  malformed-token requests on protected routes. That event is now
  reserved for application-level credential-submission failures (e.g.
  a `POST /auth/login` handler). The middleware emits two new kinds
  instead — `AuthTokenMissing` (`auth.token.missing`, Informational)
  when no bearer token is presented, and `AuthTokenInvalid`
  (`auth.token.invalid`, Warning) when a token fails validation.
  Downstream SIEM rules keyed on `auth.login.failed` from the middleware
  will go quiet for the unauthenticated-request case (the goal) and
  must switch to the new kinds. Fixes #13.

- **audit**: `AuditAccountNotification` now maps
  `AccountEvent::PasswordChanged` to the dedicated
  `AuthPasswordChanged` (`auth.password.changed`) kind at Notice
  severity. Previously it emitted `AccountUpdated` with
  `action: "password_changed"` metadata. SIEM rules that inspected the
  metadata to detect password changes must switch to the new kind.

- **audit**: `AuthLoginSuccess` now emits at Notice severity (was
  Informational). Many production log pipelines suppress
  Informational-level events by default, which silently dropped the
  success counterpart of every failure-driven login alert. Closes #19.

- **audit**: `AccountExpired` now emits at Warning severity (was
  Notice), aligning with `AccountDeleted` and other terminal account
  states. Closes #19.

### New emissions

- **audit/cedar**: The Cedar middleware now emits `AuthPermissionDenied`
  (`auth.permission.denied`, Warning) whenever a policy returns
  `Decision::Deny`. Both the HTTP middleware and the gRPC tower service
  emit. Closes part of #16.

- **audit/rate-limit**: The rate-limit middleware now emits
  `HttpRequestDenied` (`http.request.denied`, Warning) when
  `Error::RateLimitExceeded` fires. Other error variants (Redis
  connection failures, etc.) do not emit. Closes part of #16.

### Fixes

- **audit/storage**: All four storage backends (Postgres, ClickHouse,
  Turso, SurrealDB) now correctly round-trip every emitted event kind.
  Previously, `config.loaded`, `config.drift_detected`, every
  `account.*` kind (under the `accounts` feature), and the
  `login-lockout` `auth.account.locked` / `auth.account.unlocked`
  variants were silently downgraded to `AuditEventKind::Custom(...)` on
  query. Rust consumers matching on the typed variant missed the
  events; SIEM queries keyed on the stored string were unaffected.
  Closes #15.

- **audit**: `AuthTokenRevoked` events now carry `jti` in the event
  metadata. SIEM correlation by JTI and forensic queries against
  "every request that presented this revoked token" can anchor on the
  audit event directly. Closes #18.

- **audit/storage**: Storage parsers now emit a `tracing::warn!` when
  the catch-all wraps an unknown framework-owned event-kind string
  (`auth.*`, `http.*`, `account.*`, `config.*`) in `Custom`. Previously
  the catch-all was silent, which masked version skew between a newer
  emitter and an older reader — pattern matches on the typed variant
  would miss without any operator-facing signal. Closes #20.

### Documentation

- Updated `audit/page.md` to reflect the new emission set,
  severities, and `jti` metadata.
- Added "Audit Integration" sections to `cedar-auth/page.md` and
  `rate-limiting/page.md` describing the new automatic emissions.
- Added "Audit Emission" section to `token-auth/page.md` covering
  the middleware-emitted kinds and the `AuthLoginFailed` migration.

## [acton-service-v0.26.1] - 2026-05-18

### Fixes

- **surrealdb**: Derive `SurrealValue` on every struct read or bound through
  the SurrealDB storage backends. surrealdb 3.0's `IndexedResults::take<R>`
  requires `R: SurrealValue`, which broke 0.26.0 builds that combined
  `acton-service/surrealdb` with `auth`, `audit`, or `accounts`. Affected
  types: `AuditRecord`, `AuditRow`, `SigningKeyRecord`, `SigningKeyRow`,
  `AccountRecord`, `AccountRow`, and the shared types `ApiKey`,
  `RefreshTokenData`, `RefreshTokenMetadata` (the shared types are scoped
  in private inner modules so the `SurrealValue` import does not collide
  with `libsql::params::IntoValue::into_value` in the Turso storage
  submodules). Fixes #9.

### Internal

- **ci**: Install `protoc` in the Build & Test workflow so the
  `tonic-prost-build` step in `acton-service`'s build script succeeds on
  `ubuntu-latest` runners.

## [acton-service-v0.26.0] - 2026-05-17

### Breaking changes

- **crypto**: `aws-lc-rs` is now the default rustls `CryptoProvider`, with
  `ring` available as an opt-in alternative. Users building with
  `--no-default-features` must now explicitly enable exactly one of the new
  mutually-exclusive features `crypto-aws-lc-rs` or `crypto-ring`; the build
  fails with a `compile_error!` otherwise. Existing builds using default
  features get `crypto-aws-lc-rs` automatically and require no change.

  - Migration to retain prior behavior: `acton-service = { version = "...",
    default-features = false, features = ["http", "observability",
    "crypto-ring", ...] }`.
  - Migration to adopt the new default: no action; rebuild.
  - Rationale: aws-lc-rs unlocks a FIPS 140-3 path via its `fips` feature
    (ring has no FIPS validation), aligns with the rustls 0.23+, tonic 0.14+,
    and sqlx 0.8+ ecosystem default, and provides faster AEAD throughput on
    server hardware. See `acton-docs/docs/crypto-provider/` for details.

### Fixes

- **tls**: Eliminate a latent runtime panic in `load_server_config`. When
  the workspace pulled both `ring` (via `tokio-rustls`) and `aws-lc-rs`
  (transitively via `quinn-proto` and `jsonwebtoken`), `ServerConfig::
  builder()` panicked because no default `CryptoProvider` was installed.
  The new `acton_service::crypto::ensure_default_crypto_provider()` is
  invoked automatically before any server-config builder call and is also
  exposed for binaries that drive `reqwest`/`sqlx`/`tonic` TLS clients
  without going through the framework's TLS listener.

### Notes

- `aws-lc-rs` may still appear in `cargo tree` for `crypto-ring` builds
  because `quinn-proto` links it unconditionally. The *active* provider is
  whichever feature is enabled; the other is dead-ish code.
- `jsonwebtoken`'s `rust_crypto` feature pulls `aws-lc-rs` unconditionally.
  Unchanged by this release.

## [acton-service-v0.25.0] - 2026-05-15

### Features

- **graphql**: Add versioned GraphQL transport built on `async-graphql` +
  `async-graphql-axum`. Schemas are bound to `ApiVersion` via
  `VersionedGraphQLBuilder` and mounted at `/{base}/v{n}/graphql` under
  the existing versioned Axum router, so they inherit the framework
  middleware stack (auth, tracing, CORS, rate limiting, Cedar).
  GraphiQL is served on `GET`. PASETO/JWT `Claims` placed in request
  extensions propagate into the resolver `Context` automatically and are
  reachable via the `GraphQLContextExt::claims` accessor. New
  `graphql-cedar` feature adds `CedarResolverCheck` for resolver-level
  Cedar policy authorization that shares the same `CedarAuthz` instance
  the HTTP and gRPC middleware use. CLI scaffolding (`acton service new
  --graphql`, `acton service add graphql`) and Swagger UI integration
  (`openapi::graphql::add_paths_from_versioned`) round out the feature.

### Refactor

- **cedar**: Extract the policy-evaluation core out of the HTTP
  middleware and gRPC layer into a public `CedarAuthz::authorize`
  method, so all three transports (HTTP, gRPC, GraphQL) share one
  decision path including `fail_open` handling and cache wiring.

## [acton-service-v0.24.0] - 2026-05-10

### Breaking changes

- **deps**: Bump `surrealdb` from `2.6` to `3.0`. The `SurrealClient`
  type alias re-exports `surrealdb::Surreal`, so this is a public-API
  break for any consumer enabling the `surrealdb` feature. Code that
  constructs `surrealdb::opt::auth::Root` must now pass owned `String`s
  for `username`/`password`. Note that the embedded `mem://` engine is
  now strict in 3.0 and has no pre-defined root user; production
  deployments using real servers (ws/http) with pre-provisioned users
  are unaffected.
- **deps**: Bump `rusty_paseto` from `0.9` to `0.10`. The
  `PasetoAsymmetricPrivateKey`/`PasetoSymmetricKey` constructors now
  require `&Key<N>` instead of `&[u8]`; downstream code calling these
  types directly must route their bytes through `Key::from(...)` first.
- **deps**: Bump `rand` from `0.9` to `0.10`. `Rng::sample_iter` moved
  to the `RngExt` trait; code using the iterator form must import
  `rand::RngExt`.
- **deps**: Bump `askama` and `askama_web` from `0.15` to `0.16`. No
  source-level changes required at the acton-service layer, but
  template metadata and derive output changed across the major bump
  for crates using the `askama` feature.

### Miscellaneous

- **deps**: `cargo update` for all SemVer-compatible transitive bumps.


## [acton-service-v0.23.0] - 2026-04-26

### Breaking changes

- **deps**: Pin `sqlx` to the stable `0.8` line (was `0.9.0-alpha.1`) and
  add the `tls-rustls` feature (issue #8). This unblocks downstream crates
  pinned to `sqlx ^0.8` from sharing the `AppState` pool — previously the
  alpha-vs-stable major skew put two `sqlx` versions in the same binary
  and prevented `Arc<sqlx::PgPool>` from flowing across crate boundaries.
  Anyone embedding acton-service alongside another crate on the alpha
  must drop back to stable `0.8.x`. Adding `tls-rustls` lets the pool
  agent connect to managed Postgres URLs that use `?sslmode=require`
  (RDS, Cloud SQL, Neon, Supabase, Crunchy) instead of retrying forever
  and silently falling back to in-memory audit storage.
- **governor**: Route-rate-limit keys now match against the full pre-nest
  request path. Configurations that previously relied on bug #7 by writing
  post-nest keys (e.g. `"POST /uploads"` for a route registered under
  `add_version(ApiVersion::V1, ...)`) must be updated to the documented
  full path (e.g. `"POST /api/v1/uploads"`). The auto-applied middleware is
  attached to the outer router, so the URI it sees is the URI the client
  sent.

### Fixed

- **governor**: Auto-apply the rate-limit middleware from
  `[rate_limit]` config in `ServiceBuilder` (issue #7, bug 1). Previously the
  layer was never attached and users had to wire it manually despite docs
  claiming auto-apply.
- **governor**: Anonymous requests now fall back to per-IP rate limiting
  (issue #7, bug 2). Previously, requests with no claims and no matching
  per-route config silently passed through.
- **governor**: Route-key matching now sees the full pre-nest path
  (issue #7, bug 3). Doc-style keys like `"POST /api/v1/uploads"` now match
  as documented.
- **middleware**: Bypass token authentication for CORS preflight `OPTIONS`
  requests so browsers can negotiate cross-origin calls without a token.
- **service-builder**: Install the broker handle on `AppState` when actor
  extensions are registered without any pool agents, fixing
  `service_builder_initializes_broker_for_extensions_only`.

### Features

- **rate-limit**: Add `auto_apply` config knob (default `true`) to opt out
  of the auto-applied governor middleware.
- **rate-limit**: Add `trust_forwarded_headers` config knob (default
  `false`) to control IP resolution from `X-Forwarded-For` / `X-Real-IP`.
  Default-safe so direct-exposure deployments are not trivially spoofable.
- **token-auth**: Add `public_paths` to the token auth middleware
  configuration so selected routes can be exposed without authentication.
- **htmx**: Add frontend routes support to VersionedApiBuilder

### Documentation

- **rate-limiting**: Document auto-apply behavior, IP fallback resolution
  order, the `auto_apply` and `trust_forwarded_headers` config knobs, and
  the breaking change to route-key matching.
- Replace incorrect Router::new() examples with VersionedApiBuilder
- **htmx**: Add comprehensive HTMX, Askama, and SSE documentation

### Miscellaneous

- Update CHANGELOG for v0.10.0
- **docs**: Update version to 0.10.0
## [acton-service-v0.10.0] - 2026-01-12

### Documentation

- **auth**: Add comprehensive authentication module documentation
- **auth**: Add comprehensive authentication module documentation
- Update documentation for PASETO-first token authentication
- Reposition as backend framework that scales to microservices
- Update docs site metadata
- **tier-4**: Update example documentation
- **tier-3**: Update feature documentation
- **tier-2**: Update entry point documentation
- **tier-1**: Update Hero component messaging
- **tier-1**: Update docs homepage positioning
- **tier-1**: Update lib.rs crate documentation
- **tier-1**: Update acton-service Cargo.toml description
- **tier-1**: Reposition README as backend framework
- Sync all version files to 0.9.0
- Update version to 0.9.0

### Features

- **examples**: Add HTMX Task Manager example
- **htmx**: Add HTMX, Askama templates, and SSE support
- **session**: Add HTTP session management for HTMX/SSR applications
- **rate-limit**: Add per-route rate limiting with config-based setup
- **auth**: Add comprehensive authentication module
- Add PASETO as default token format with JWT feature-gated
## [acton-service-v0.9.0] - 2026-01-11

### Documentation

- **turso**: Add Turso/libsql database documentation
- **websocket**: Add WebSocket feature documentation

### Features

- **websocket**: Add feature-gated WebSocket support with room management
- Add git-cliff for automated changelog generation
## [acton-service-v0.8.0] - 2026-01-11

### Bug Fixes

- **observability**: Coordinate tracing init via shared Once guard
- **examples**: Correct ping-pong required-features
- **examples**: Correct ping-pong required-features and doctest

### Documentation

- **reactive-architecture**: Add Event Broker section
- Add migration guide for v0.7 to v0.8
- Add agent architecture notes to pool documentation
- Update quickstart and configuration with agent spawning
- Add TypeID Request IDs documentation
- Add BackgroundWorker guide
- Add Reactive Architecture guide
- Update documentation for TypeID-based request IDs
- Add web app integration guide for HTMX and session-based auth
- Update sponsor section
- **readme**: Add GitHub Sponsors link

### Features

- **turso**: Add Turso/libsql database support as feature-gated capability
- **database**: Unify env var and fix graceful shutdown
- **examples**: Add database example with Docker and migrations
- **versioning**: Make VersionedApiBuilder generic over custom config type
- **prelude**: Re-export common framework dependencies
- **examples**: Add BackgroundWorker example
- **agents**: Make agent-based pool management the default architecture
- **agents**: Add BackgroundWorker for managed task execution
- **agents**: Add JwtRevocationService with write-behind Redis persistence
- **state**: Add broker support for event-driven architecture
- **agents**: Add HealthMonitorAgent and reactive health updates
- **builder**: Integrate acton-reactive runtime with ServiceBuilder
- **agents**: Add acton-reactive pool agents for database, Redis, and NATS
- **ids**: Integrate mti crate for type-safe request identifiers

### Miscellaneous

- **deps**: Use published acton-reactive 7.0.0
- **deps**: Remove unused dependencies and fix example compilation
- Remove repo-specific FUNDING.yml (inherited from org)
- Add GitHub Sponsors funding configuration

### Refactoring

- Update to acton-reactive 0.7.0 with Actor naming
- **agents**: Simplify architecture to hide internal implementation
- **agents**: Make acton-reactive core dependency and internalize agents

### Tests

- **turso**: Add integration tests for local database and TursoDbAgent
## [acton-service-v0.7.0] - 2025-11-18

### Documentation

- **readme**: Add custom config extension documentation
- Add custom config extension documentation

### Features

- Add generic config extension support

### Miscellaneous

- **docs**: Bump version to 0.7.0
- Add GitHub release notes configuration
- Remove docs folder
- **docs**: Update version to 0.6.0 and add CLI experimental warning
## [acton-service-v0.6.0] - 2025-11-17

### Bug Fixes

- **docs**: Correct broken example links in README
- **docs**: Correct example file paths in tutorial
- **docs**: Use valid callout type in tutorial page
- **docs**: Correct Markdoc function call syntax in link tags
- **docs**: Simplify link tag and use githubUrl function
- **docs**: Use proper component for link tag instead of inline function
- **docs**: Use custom link tag for variable-based URLs
- **docs**: Remove backticks from link text to fix markdown rendering
- **docs**: Improve Markdoc variable interpolation in links
- **docs**: Use hardcoded GitHub URLs instead of Markdoc variables for example links
- **docs**: Add basePath to internal links in Markdoc transformer
- Correct file paths and documentation after examples reorganization
- **docs**: Resolve TypeScript module import error in markdoc config
- **docs**: Remove unsupported claims and irrelevant comparisons from comparison page
- **docs**: Enforce VersionedApiBuilder in all code examples
- **docs**: Remove non-existent middleware API, document automatic JWT configuration
- **docs**: Correct example file paths in api-versioning to match actual repository structure
- **docs**: Correct resilience API parameter types and method names
- **docs**: Replace non-existent API with config-based rate limiting approach
- **docs**: Remove non-existent .with_middleware() API, document automatic middleware
- **docs**: Correct version numbers and database access method in troubleshooting
- **docs**: Correct acton-service version in feature-flags from 0.3 to 0.2 (18 instances)
- **docs**: Add missing .await calls and fix method names in events page
- **docs**: Correct default HTTP port from 3000 to 8080 in cli-scaffolding
- **docs**: Correct Kubernetes health probe paths in faq
- **docs**: Correct database access methods from database() to db() with proper async/Option handling
- **docs**: Correct health check JSON response structures (5 instances)
- **docs**: Add missing return type annotations to main functions in comparison
- **docs**: Correct health endpoint paths in examples
- **docs**: Correct acton-service version in quickstart from 0.3 to 0.2
- **docs**: Make Fence language parameter optional with rust default
- **middleware**: Skip JWT and Cedar auth for health/ready endpoints
- **observability**: Respect RUST_LOG environment variable
- **service**: Apply middleware stack in ServiceBuilder
- **cedar**: Correct middleware execution order
- **cedar**: Resolve nested runtime error in auto-middleware
- **cedar**: Update path parameters to Axum 0.8 syntax
- **cedar**: Remove unnecessary cast in Redis cache

### Documentation

- Update README to reference online documentation
- **tutorial**: Add comprehensive production API tutorial
- **examples**: Update documentation to reflect reorganized examples structure
- Organize examples directory by feature category
- Replace hardcoded versions with Markdoc variables
- Add service discovery documentation page
- Centralize GitHub repository URL using DRY principle
- Add documentation site link to README (#2)
- Remove subjective language and unsupported claims from documentation
- Add documentation site link to README and repo description
- **high-priority**: Add Redis vs Governor decision guide and lazy_init explanation
- **critical**: Add missing content for JWT, database, and API versioning
- Add glossary, concepts page, and navigation headers to address curse of knowledge issues
- **jwt**: Clarify JWT token revocation is fully implemented
- **cedar**: Add Cedar authorization feature to documentation and clarify hot-reload status
- **cedar**: Improve Cedar example documentation and simplify test script
- **cedar**: Improve README with auto-setup and verified test commands
- **cedar**: Add comprehensive Cedar authorization example

### Features

- **docs**: Add tutorial to navigation menu
- **docs**: Implement proper Markdoc variable interpolation in link nodes
- **docs**: Add GitHub Pages deployment workflow (#1)
- **docs**: Add GitHub Pages deployment workflow
- **docs**: Add version display to logo and improve formatting
- **versioning**: Add automatic logging and metrics for deprecated API usage
- **docs**: Add Next.js documentation website with acton-service branding
- **cedar**: Add builder pattern and fix permission-based authorization
- **cedar**: Add customizable path normalizer with builder pattern
- **cedar**: Merge Cedar authorization implementation
- **framework**: Auto-apply JWT and Cedar middleware in ServiceBuilder
- **cedar**: Make example self-contained with auto-setup
- **cedar**: Export Cedar types in prelude module
- **cedar**: Add gRPC Tower Layer for Cedar authorization
- **cedar**: Implement HTTP authorization middleware
- **cedar**: Add CedarConfig to configuration system
- **cedar**: Add Cedar authorization feature flag and dependency

### Miscellaneous

- **docs**: Remove duplicate markdown files already in acton-docs site

### Refactoring

- **cedar**: Make middleware generic and framework-grade
## [acton-service-v0.5.2] - 2025-11-11

### Features

- **prelude**: Re-export Response type
## [acton-service-v0.5.1] - 2025-11-11

### Documentation

- **tutorial**: Add custom state and headers sections

### Features

- **prelude**: Re-export HeaderMap and HeaderValue
## [acton-service-v0.5.0] - 2025-11-11

### Bug Fixes

- **security**: Change CORS default from permissive to restrictive
## [acton-service-v0.4.0] - 2025-11-11

### Bug Fixes

- **cli**: Remove unexpected cfg from build.rs template
- **cli**: Ensure generated services compile and run
- **cli**: Update service templates to reflect actual implementation
- **config**: Prevent XDG directory creation and fix config template
- **cli**: Correct import generation for generated services
- Correct GitHub organization capitalization to Govcraft
- **build**: Remove unreachable code in compile_protos_with_descriptor
- **service**: Use loaded config in ServiceBuilder's AppState

### Documentation

- Add comprehensive onboarding documentation for improved developer experience
- **readme**: Reposition value propositions beyond API versioning
- **cli**: Enhance gRPC port configuration documentation and messaging
- Update documentation to reflect implemented features
- Add comprehensive README and MIT LICENSE
- **service**: Update ServiceBuilder docs to reflect automatic initialization

### Features

- **framework**: Add production-ready error messages and pool monitoring
- **middleware**: Implement JWT revocation with Redis backend
- **taskfile**: Add release-service task and rename release to release-cli
- **taskfile**: Add release task for versioning CLI
- **cli**: Implement user-customizable template system
- **build**: Add Taskfile for CLI build and installation
- **grpc**: Implement single-port HTTP + gRPC multiplexing
- **middleware**: Implement production-ready metrics middleware with OpenTelemetry
- **middleware**: Implement production-ready resilience patterns
- **observability**: Implement full OpenTelemetry OTLP integration
- **cli**: Implement shell completions command
- **cli**: Implement validate command with comprehensive service validation
- **cli**: Implement grpc command with comprehensive gRPC setup guide
- **cli**: Implement middleware command with comprehensive middleware guides
- **cli**: Implement add version command with comprehensive guidance
- **cli**: Implement dev run command
- **cli**: Implement dev logs command with helpful guidance
- **cli**: Implement dev health command
- **cli**: Implement generate proto command
- **cli**: Implement generate config command
- **cli**: Implement add endpoint, add worker, and generate deployment commands
- **cli**: Implement acton CLI with service scaffolding
- **grpc**: Add build utilities for proto compilation
- **examples**: Add event-driven microservice example
- **examples**: Add ping-pong HTTP to gRPC example
- **grpc**: Implement health check and reflection services (Phase 3)
- **grpc**: Implement Phase 2 middleware parity with HTTP
- **grpc**: Add basic gRPC infrastructure to acton-service framework
- **health**: Integrate proper health.rs handlers with dependency checking
- **service**: Add automatic config loading and tracing initialization
- **services**: Add production API gateway
- **services**: Add production backend service
- **examples**: Add API gateway with gRPC client
- **examples**: Add backend service with dual-protocol support
- **examples**: Add simple API examples
- **acton-service**: Add observability and API features
- **acton-service**: Add server runtime and state management
- **acton-service**: Add infrastructure integrations
- **acton-service**: Add middleware layer
- **acton-service**: Add error handling and response types
- **acton-service**: Add configuration module
- **acton-service**: Add library core and manifest
- Add gRPC protocol buffer definitions

### Miscellaneous

- **deps**: Restore acton-service version to 0.3.0
- **deps**: Use last published acton-service version (0.2.0)
- **deps**: Add version specification for acton-service dependency
- Add crates.io publication metadata
- **services**: Remove production service scaffolds
- Add workspace configuration and examples overview
- Add workspace dependency lock file
- Add project configuration files

### Performance

- **deps**: Optimize dependency features to reduce compile time and binary size

### Refactoring

- **cli**: Remove unused template generation functions

### Tests

- **grpc**: Add single-port example to verify HTTP + gRPC multiplexing
- **observability**: Add comprehensive tests and working example
<!-- generated by git-cliff -->
