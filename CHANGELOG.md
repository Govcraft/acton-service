# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **config(metrics)**: `[middleware.metrics.exporter]` opens a dedicated,
  plaintext listener that serves only `GET /metrics` (#129). The main
  listener's `/metrics` route inherits that listener's TLS, auth and
  middleware, which is right for an application surface and wrong for a scrape
  target: managed collectors — Fly.io's `[[metrics]]` block, GKE managed
  collection, the defaults of most `PodMonitor`/`ServiceMonitor` resources —
  speak plain HTTP to a declared port and expose no TLS knobs, so a service
  terminating TLS could not be scraped through any of them without stopping
  terminating TLS.

  The second socket serves the same bytes, from the same registry, through the
  same handler, so the two cannot drift. It carries no TLS, no authentication
  and no other middleware by design, and every path other than `GET /metrics`
  is a 404 — bind it to a private scrape network.

  Both `bind` and `port` are required; there is no default address to open
  unasked. A `port` of `0`, a port shared with the HTTP or separate-port gRPC
  listener, or the table appearing in a build without the `prometheus-metrics`
  feature are all refused before anything binds. The listener binds ahead of
  the service's own listeners, so a clash refuses to start, and drains after
  them, so the last scrape still observes the drain. Absent table, absent
  listener: nothing changes for a config that does not write it.

### Fixed

- **cli**: `acton new --observability` generated a `config.toml` whose
  `[middleware.metrics]` table set `export_interval_secs`, a key that has never
  existed. Since 0.36.0 made that table `deny_unknown_fields`, every service
  the CLI generated with observability enabled failed at startup on its own
  scaffold. The key is removed.
- **cli**: generated Kubernetes manifests declared a `metrics` container port,
  a Service port and a `ServiceMonitor` scraping 9090 — a port nothing had ever
  listened on. The generated config now writes a matching
  `[middleware.metrics.exporter]` table, and the generated project enables the
  `prometheus-metrics` feature that table requires, so the manifest describes
  something real.

## [acton-service-v0.36.0] - 2026-08-05

A single fix, and a breaking one: `[middleware.metrics]` now reaches the
instruments it claimed to configure, and the keys it could never honour fail
startup instead of being ignored. Anyone whose config sets only `enabled` is
unaffected, and the boundaries a service already had do not move.

### Fixed

- **config(metrics)**: `[middleware.metrics] latency_buckets_ms` now reaches the
  histogram. It was parsed, carried into a second struct of the same name, and
  then dropped: the layer was built as
  `HTTPMetricsLayerBuilder::builder().with_meter(meter).build()`, with nothing
  else from the configuration reaching it. An operator who tuned buckets to
  their SLO thresholds got the defaults back, with no warning at startup or in
  the scrape.

  Passing the boundaries would not have been enough on its own. The seconds
  histogram view added in #107 matches on unit, and a view that matches an
  instrument *overrules* that instrument's own boundaries — the SDK consults
  them only when no view matched. The view now declines
  `http.server.request.duration`, which is the instrument that carries a
  configured value.

  The default for the key is the view's own boundary set, so a config that omits
  it keeps exactly the buckets it had.

### Changed

- **config(metrics)**: `[middleware.metrics]` is `deny_unknown_fields`, and
  `include_path`, `include_method` and `include_status` are removed. All three
  parsed and did nothing; they now fail startup, as does `service_name`, which
  was never a key of this table. The metrics layer records
  `http.request.method`, `http.route` and `http.response.status_code` because
  the semantic conventions require them, and a service is named once under
  `[service]`.
- **config(metrics)**: `middleware::metrics::MetricsConfig` is now a re-export of
  `config::MetricsConfig` rather than a second type with a hand-written
  conversion between them. The conversion is where the configured values were
  lost. `with_service_name`, `with_include_*` and `latency_buckets_as_duration`
  are gone; `with_latency_buckets` is `with_latency_buckets_ms`.
- **server**: the startup banner reports HTTP metrics as enabled only when they
  are. A present `[middleware.metrics]` table with `enabled = false` was
  announced as enabled, sending operators looking for a scrape that was never
  going to appear.
- **metrics**: `metric_names` and `metric_labels` now name what the layer emits.
  `HTTP_SERVER_REQUEST_COUNT` named an instrument that has never existed — the
  count is the duration histogram's `_count` — and the size constants and every
  label used pre-1.0 semantic-convention spellings (`http.method`,
  `http.status_code`) that no instrument records. A dashboard built on them
  queried nothing. Both modules are now asserted against a live scrape rather
  than against their own literals, which is all the previous tests did.

## [acton-service-v0.35.0] - 2026-08-03

Upgrades to acton-reactive 9.0 and closes the gaps that version exposed in how
this crate used the framework. Two of them were live defects: supervised actor
extensions were never actually restarted, and three public query messages could
not be answered at all.

Breaking, and unavoidably so: acton-reactive types appear in this crate's public
API and are re-exported through its prelude, so its 8.1 → 9.0 major bump is a
breaking change for downstream code regardless of anything else here.

### Fixed

- **extensions**: `ActorExtension::restart_policy` now takes effect. The
  spawner called the legacy `supervise()`, which neither read the declared
  policy nor registers a blueprint — acton-reactive reports such a child's
  termination but can never recreate it. The documented "automatic restart on
  failure" therefore never happened. Extensions are now registered through
  `supervise_with`, handing `A::configure` over as the restart blueprint.
- **agents**: `CheckRotation`, `ForceRotation` and the audit `CleanupTrigger`
  handlers spawned detached work with `tokio::spawn` and did not await it, so
  the work escaped shutdown and supervision and two cycles could overlap. They
  now return `Reply::pending`; the spawn that remains only crosses the
  `Send`-but-not-`Sync` boundary that `#[async_trait]` storage futures impose,
  and its `JoinHandle` is awaited.
- **agents**: The key-rotation and audit-retention tickers ran forever holding
  cloned state, surviving shutdown. Both now stop on a `CancellationToken`
  cancelled from `before_stop`, matching what the pool agents already did.

### Added

- **websocket / agents**: `GetRoomInfo`, `GetTaskStatus` and
  `GetAllTaskStatuses` implement `Request`, so `ask` resolves them. All three
  were public and handled via `reply_envelope()`, but before 9.0 a caller
  outside the actor system had no way to receive the reply, which made them
  unusable.
- **agents**: Pool agents answer `GetPoolHealth` (sample the current state) and
  `WaitForPoolReady` (park until the first connection attempt settles). This
  makes `ComponentHealth` and `HealthStatus` — previously exported and used
  nowhere — functional, and pool agents now retain the failure reason instead
  of only logging it.
- **agents**: `BackgroundWorker::wait_for_task` waits for a task to finish
  rather than polling for it.
- **state**: `AppState::supervised_actor` exposes an extension's
  `SupervisedChild` for inspecting supervision state or waiting out a restart.

### Changed

- **background-worker**: The task registry moved into the actor. It previously
  lived outside as an `Arc<DashMap<String, TaskInfo>>` whose entries wrapped
  status and join handles in `Arc<Mutex<..>>`, written by spawned tasks and
  read by handlers that never touched their own model. Spawned work now reports
  its outcome as a message, so status has exactly one writer, and shutdown
  drains through a `TaskTracker`.
- **tests**: Actor tests synchronise on barriers rather than sleeps —
  `ask` for mailbox order, `FlushBroadcasts` for broker fan-out,
  `WaitForPoolReady` for pool startup, and watch channels where the signal
  originates downstream of a mailbox. Several tests that asserted only "no
  panic" now assert the state they name.

### Breaking

- `AppState::actor` returns `Option<ActorHandle>` rather than
  `Option<&ActorHandle>`, because a restart replaces the actor and a cached
  handle would silently address a dead one. Resolve it per use.
- `BackgroundWorker` queries are async and fallible: `get_task_status` becomes
  `task_status` returning `Result<Option<TaskStatus>, AskError>`, and
  `task_count`, `running_task_count`, `has_task` and `cleanup_finished_tasks`
  likewise return `Result`. `cancel` now returns the task's final status.
- `GetTaskStatus` answers with `Option<TaskStatusResponse>`, so an unknown task
  is no longer indistinguishable from a pending one.
- Minimum `acton-reactive` is 9.0.

## [acton-service-v0.34.1] - 2026-08-03

A TLS listener compiled without the `grpc` feature dropped every HTTP/2
client on the floor and logged nothing about it (#120). The listener
advertises `h2` in ALPN, but the server behind it was built without HTTP/2
support, so any ALPN-honouring client — curl at its defaults, browsers, Go's
`net/http` — took the offer, sent the HTTP/2 connection preface, and had its
connection closed before a span ever opened. Nothing appeared in the server
log at any level, and the client-side error looks like a TLS fault, so an
operator following an mTLS runbook reads it as "my certificates are wrong".

### Fixed

- **tls**: Enable axum's `http2` feature so the HTTP listener serves the
  protocol it advertises. `http2` is not in axum's default set, and it is
  what turns on `hyper-util/http2` beneath `axum::serve`; without it that
  auto server is HTTP/1.1-only while `load_server_config` offers
  `[h2, http/1.1]`. Services compiling `grpc` were never affected — tonic
  enables the same hyper-util feature transitively — which is what confined
  the defect to TLS-without-gRPC builds and hid it from the `full` test legs.
  `reqwest` without its own `http2` feature negotiates `http/1.1` and kept
  working throughout, which is why no existing test saw it.
- **http**: A side effect worth knowing about: the plaintext listener now also
  answers an h2c client that connects with prior knowledge, because the same
  hyper-util auto server sniffs the preface whether or not TLS is in front of
  it. Nothing that worked before behaves differently — HTTP/1.1 clients are
  untouched, and there is no h2c upgrade dance, only prior knowledge.

### Added

- **tests**: `tls_alpn_http2.rs` drives the TLS listener with a hand-rolled
  h2 client over `tokio-rustls` and requires the connection preface to be
  answered with a SETTINGS frame, alongside an HTTP/1.1 leg that must keep
  working. It deliberately adds no HTTP client dependency: anything that
  speaks h2 turns `hyper-util/http2` on for the build under test and would
  mask the very regression it exists to catch. CI runs it on the
  `tls-no-grpc` combination, which is now the one feature leg that runs
  tests as well as clippy.

## [acton-service-v0.34.0] - 2026-08-02

Closes every open issue on the tracker. The headline is mutual-TLS caller
authorization (#109): until now `[tls].client_ca_path` decided whose
certificates were *accepted* and nothing decided which of those callers
could *proceed*, so every principal a fleet CA had ever issued to could
reach every mTLS route of every peer. Alongside it, a root-level catch-all
that no longer requires the `htmx` feature (#110), and fixes for three
defects that were each invisible in their own way: a latency histogram that
looked healthy while unable to distinguish a fast request from a slow one
(#107), operator config mistakes reported as internal framework errors
(#105), and a gRPC channel that could stall forever on a peer that connected
and then went quiet (#103).

`Error` is now `#[non_exhaustive]`, which is why this is 0.34.0.

### Added

- **caller_auth**: Transport-level admission control for mutual-TLS callers,
  gated on `tls`. `CallerSan` names a caller from its leaf certificate's
  `subjectAltName` (DNS or URI, byte-exact within its kind; no wildcard,
  suffix or subdomain matching, and a wildcard SAN inside a certificate is
  dropped rather than matched). `CallerAllowlist` cannot be constructed
  empty, because an allowlist admitting everyone is how this control fails
  open. `authorize(policy, leaf_der, bearer_present)` is the whole decision
  as a pure function naming no transport type, so HTTP and gRPC share it.
  Configured under `[caller_auth]` with `mode`, `allowlist` and
  `public_paths`.
- **caller_auth**: `CallerAuthMode` of `bearer` | `mtls` | `mtls-or-bearer`.
  The third exists so a fleet can cut over caller-by-caller: a caller with
  no certificate yet, or one not yet allowlisted, keeps working on its
  token. Under that mode an allowlisted certificate waives the token
  requirement for that request; under `mtls` the certificate is an
  *additional* requirement and a configured token layer still demands a
  valid token.
- **caller_auth**: Configuration that would look like protection without
  being it is refused at startup rather than ignored: a certificate mode on
  a listener with no `client_ca_path` (`[tls]` and `[grpc.tls]` are checked
  separately and the error names which one to fix), an `allowlist` under
  `mode = "bearer"` where nothing would consult it, an empty `allowlist`
  under a certificate mode, and unknown keys in the section.
- **routing**: `VersionedApiBuilder::with_fallback` and
  `with_fallback_service` — a root-level catch-all with no feature gate. The
  only prior route to one was `with_frontend_routes`, which is
  `#[cfg(feature = "htmx")]`, so a proxy with no frontend and no templates
  had to compile `axum-htmx` to attach a catch-all or drop off
  `ServiceBuilder` entirely and lose the whole middleware stack. Two methods
  because a transparent proxy forwards to a `tower::Service`, not an axum
  handler. Applied after health, frontend and versioned routes, so the
  catch-all shadows nothing.
- **client-tls**: `ClientIdentityConfig::connect_timeout_secs` and the
  exported `DEFAULT_CLIENT_CONNECT_TIMEOUT` (30s), mirroring the listener's
  `handshake_timeout_secs`. One budget spans both the TCP connect and the
  handshake rather than one each, since a stall can sit in either and two
  independent bounds would let a peer burn both in sequence. A configured
  `0` resolves to the default rather than failing every connection.
- **error**: `Error::Tls(String)`, displayed as
  `TLS configuration error: {0}`.

### Changed

- **Breaking**: `Error` is now `#[non_exhaustive]`. Adding the `Tls` variant
  to a 52-variant exhaustive enum is itself breaking, so this is the free
  window to make every future variant additive.
- **observability**: Histograms declared with `.with_unit("s")` are now
  bucketed with seconds-scaled boundaries via a metric view. This enables
  `opentelemetry_sdk`'s `spec_unstable_metrics_views` feature, the only
  route to registering a view; the `unstable` marker is the OTel Rust SDK's
  flag for spec features not yet stabilized, so an SDK upgrade could move
  this API.
- **client-tls**: The `grpc_channel` docs no longer tell callers to wrap
  their first RPC in `tokio::time::timeout`. That was a workaround for the
  missing bound this release adds.
- **ci**: Clippy now runs on ten feature combinations rather than two. The
  `audit-storage` matrix gained a lint step, and a new lint-only
  `feature-combos` job covers seven combinations no test job compiles
  (`minimal`, `tls-no-grpc`, `grpc-no-tls`, `tokens`, `frontend`, `graphql`,
  `audit-nodb`). Lints in cfg-gated code were previously unreachable by CI,
  and `--all-features` is not a substitute because the crate's
  mutual-exclusion `compile_error!` guards reject it by design.

### Fixed

- **observability**: Seconds-valued histograms were bucketed against the
  SDK's default boundaries, `[0, 5, 10, 25, …, 7500, 10000]`, which are
  chosen for milliseconds. Every observation under five seconds landed in
  the first bucket. Nothing warned, and the exposition looked well-formed.
  The new boundaries are the OTel semantic-convention set for
  `http.server.request.duration` extended downward with two sub-5ms values,
  not replaced, so existing dashboards and alerts still line up. The view
  selects on the instrument's *unit* rather than its name, so any histogram
  following the semantic conventions is bucketed correctly without its
  author knowing the view exists.
- **tls**: Boot-time TLS configuration failures were wrapped in
  `Error::Internal`, whose `Display` is `Internal server error: {0}`. A bad
  path or an unparseable PEM told an operator to suspect the framework
  instead of their own `cert_path`. 27 construction sites now yield
  `Error::Tls`: 13 in `tls.rs`, 14 in `client_tls.rs` for outbound identity
  material. Runtime and OS faults stay `Internal`. HTTP mapping is
  unchanged at 500, with the operator detail staying in the log rather than
  going out on the wire.
- **client-tls**: `grpc_channel` had no connect or handshake bound. #97
  switched it to `Channel::new` to bypass tonic's scheme-inspecting
  connector wrapper, but that wrapper is also where tonic applies
  `Endpoint::connect_timeout`, so the setting was silently dropped and tonic
  exposes no getter to read it back. A peer that completed the TCP connect
  and then stalled the handshake held the attempt open indefinitely.
- **client-tls**: The `tls` field and `tls_config_with_alpn` were ungated
  though their only non-test consumer is `#[cfg(feature = "grpc")]`,
  producing dead-code warnings under `tls`-without-`grpc`.
- **audit**: `let mut kinds` in `event.rs` where nothing extends it unless
  `login-lockout` or `accounts` is enabled.
- **docs**: Two unresolved rustdoc links in `checks.rs` pointing at
  `crate::service` instead of `crate::service_builder`.

## [acton-service-v0.33.0] - 2026-07-24

A healthy dependency can now speak. `CheckOutcome::Ready` carries no
message, which left no way for a readiness check to affirm *what* is
healthy — e.g. the verified identity of a mounted artifact — without
abusing `Degraded` (and reporting `healthy: false` for a healthy concern).

### Added

- `CheckOutcome::ReadyWithMessage(String)`: healthy, with an
  operator-facing affirmation rendered as the dependency's `message` on
  `/ready`. `healthy` stays `true`; the endpoint's answer is unaffected;
  liveness treats it as alive.

### Changed

- **Breaking**: adding a variant to the exhaustive `CheckOutcome` enum
  requires downstream exhaustive matches to add an arm — hence 0.33.0.

## [acton-service-v0.32.0] - 2026-07-24

Lets a service tell the truth on its probe endpoints. Until now `/health`
could never fail and `/ready` probed only framework-managed backends
(database, cache, events); a service whose real readiness lives in
application state — a writer task, a consensus quorum, a sidecar — had no
way to surface it. Additive only; services registering no checks behave
exactly as before.

### Added

- **checks**: App-defined probe checks, registered on the builder:
  `ServiceBuilder::with_readiness_check(name, || async { … })` folds into
  `/ready`, `with_liveness_check` into `/health`; both repeatable. A check
  returns `CheckOutcome::Ready`, `Degraded(message)` (rendered as an
  unhealthy dependency in the readiness body **without** flipping overall
  readiness — visible to operators, invisible to the load balancer), or
  `Unready(message)` (flips the endpoint to 503). Liveness treats `Degraded`
  as alive — liveness is binary, and a degraded-but-working process must not
  be killed.
- **checks**: All of an endpoint's registered checks run concurrently under
  **one shared deadline** (`with_check_deadline`, default 2s) — N stalled
  checks cost one deadline, not N; a check unresolved at the deadline
  reports `Unready("check timed out")`.
- **state**: `AppState::health_checks()` exposes the installed check set;
  checks are installed by `build()` whether the state was framework-built or
  caller-provided.

### Changed

- **health**: `/health` answers 503 with `status: "unhealthy"` when a
  registered liveness check fails. With no registered checks it cannot fail,
  exactly as before. `/ready`'s response shape is unchanged; app-defined
  checks appear as additional entries in `dependencies`.

## [acton-service-v0.31.2] - 2026-07-24

Hardens the audit trail for deployments that treat the exported stream as the
audit record. Three fixes, found while wiring the `audit` feature into a
downstream admin plane: the chain hash ignored half the event, the drift
endpoint mounted itself unauthenticated, and the audit source IP trusted
client-supplied headers. No breaking API changes; two new config keys, both
default-off.

### Fixed

- **audit**: The chain hash now covers the whole event. The legacy scheme
  omitted `metadata`, source IP, user agent, request ID, and duration — a
  tampered role list or forged origin verified as intact. New events seal
  under a self-describing `v2:`-prefixed hash with length-framed fields;
  the version travels inside the hash string itself, so storage schemas are
  unchanged and chains spanning the upgrade verify per event. Metadata is
  hashed as canonical recursively-key-sorted JSON, stable across
  `preserve_order` feature unification and JSON-normalizing stores. A
  mid-chain downgrade forgery is caught by the successor's `previous_hash`.
- **audit**: The syslog export now emits every hash input (event id,
  canonical RFC 3339 timestamp, kind, severity, service, user agent,
  canonical metadata, previous hash), so a chain rebuilt from exported lines
  alone recomputes and verifies — required when syslog is the only durable
  store. `AuditEventKind::from_wire` provides the Display-inverse a verifier
  needs; a round-trip test pins export → parse → verify.
- **audit**: `GET /admin/config/drift` is no longer mounted unconditionally.
  It carries no authentication and sits on the outer router, outside any
  route-level middleware, so compiling the `audit` feature in exposed config
  fingerprints and drift sections to any caller. It now mounts only when the
  new `[audit] drift_endpoint_enabled` flag (default `false`) is set.
- **middleware**: The request context no longer hardcodes trust in
  `X-Forwarded-For` / `X-Real-IP` when resolving the client IP that audit
  events record. Trust is now `[service] trust_forwarded_headers` (default
  `false`: the direct TCP/TLS peer wins, so a direct client cannot falsify
  its recorded origin). The header-only fallback used by hand-assembled
  routers records no IP at all rather than a spoofable one. Rate limiting
  keeps its own independent flag.

## [acton-service-v0.31.1] - 2026-07-20

Makes gRPC-over-TLS work end to end. Two complementary bugs left an
acton-service gRPC client unable to reach an acton-service gRPC TLS listener
through tonic's TLS plumbing; both are fixed here. No API changes.

### Fixed

- **tls**: The TLS listener now advertises ALPN (`[h2, http/1.1]`).
  `load_server_config` previously set no `alpn_protocols`, so the listener
  answered no ALPN offer and strict gRPC clients — a `tonic` `ClientTlsConfig`
  without `assume_http2`, grpcurl, or any non-Rust stack — failed with
  `H2NotNegotiated`. Clients that offer no ALPN are unaffected. (#98)
- **client-tls**: `ClientIdentitySource::grpc_channel` no longer fails the first
  RPC with `HttpsUriWithoutTlsSupport`. With any tonic TLS feature compiled in
  (which `crypto-aws-lc-rs`/`crypto-ring` always pull), tonic wrapped the custom
  rotating connector in a scheme-inspecting connector that rejected the `https`
  URI before the connector ran. The channel is now built through
  `tonic::transport::Channel::new`, tonic's lower-level custom-connector entry
  point, which skips that wrapper while preserving the endpoint's per-RPC
  timeout, keep-alive, concurrency, HTTP/2 tuning and user agent.
  `Endpoint::connect_timeout` is not honoured on this path (a tonic limitation);
  the method's docs note the caller-side remedy. (#97)

## [acton-service-v0.31.0] - 2026-07-20

Hardens the 0.30.0 mutual-TLS surface. The headline fix closes a
pre-authentication denial of service on the TLS accept path; the rest are
follow-ups from the same security review. No intentional breaking API changes,
though `[tls]`/`[grpc.tls]` sections now reject unknown keys, so a config that
carried a typo'd or stray field there will fail to parse instead of silently
ignoring it.

### Fixed

- **tls**: The TLS handshake no longer runs inline on the `accept` path. It
  previously completed inside the `axum::serve::Listener::accept` future with no
  timeout, so handshakes were serialized listener-wide and a single peer that
  connected but never sent a `ClientHello` parked the accept future forever,
  blocking every new connection — an unauthenticated, pre-verification denial of
  service that mutual TLS did not defend against. A background pump task now owns
  the listener and spawns each handshake as its own task behind a
  `tokio::time::timeout`, with completed streams handed back through a bounded
  channel. Handshakes run concurrently again and the timeout is a per-connection
  bound. Rotated credentials are still captured per handshake. (#94)

### Added

- **tls**: `handshake_timeout_secs` on `[tls]` and `[grpc.tls]` caps how long a
  handshake may take before the connection is dropped (default 10s; `0` is
  rejected at build time). (#94)
- **tls**: Peer-IP rate limiting and request-context IP resolution now recover
  the remote address from `TlsConnectInfo` as well as `ConnectInfo<SocketAddr>`,
  so they work on directly-terminated TLS listeners without a fronting proxy.
  (#95)

### Changed

- **tls**: `TlsConfig` now denies unknown fields, so a mistyped reload trigger
  (for example `reload_interval_sec`) is reported at startup instead of silently
  leaving rotation disarmed. (#95)
- **tls**: The reload poll baseline is captured when the source loads its
  credentials rather than when the poll task spawns, so a rotation that lands
  during startup is picked up on the first tick instead of being missed until the
  next one. (#95)
- **tls**: Credential file reads on the poll tick and SIGHUP handler run on the
  blocking pool, so a slow or networked secret mount cannot stall a runtime
  worker. (#95)
- **tls**: A `client_auth_optional = true` set without `client_ca_path` now logs
  a warning at startup rather than being silently ignored. (#95)

### Security

- **client-tls**: The decoded DER private key in the outbound client identity is
  zeroized on drop, closing a window where its bytes were freed un-wiped even
  though the PEM copies were already zeroized. (#95)

## [acton-service-v0.30.0] - 2026-07-20

The mutual-TLS release: inbound client-certificate verification, an outbound
client identity, and in-place rotation of every credential the framework
touches — server certificates, client identities, and the trust anchors both
are verified against — with no socket rebind, no connection-pool loss, and no
restart. There are no breaking changes; every addition is opt-in.

### Added

- **tls**: Optional client-certificate verification for inbound TLS. Setting
  `client_ca_path` on `[tls]` or `[grpc.tls]` verifies peers against that CA
  bundle with `WebPkiClientVerifier`; `client_auth_optional = true` admits
  connections without a certificate while still rejecting invalid ones.
  Verified peer certificates reach handlers through the new `TlsConnectInfo`
  connect-info type, which also gives TLS listeners a real remote address for
  the first time. Absent a CA bundle, behaviour is unchanged. (#68)
- **tls**: `TlsConfigSource`, an `ArcSwap`-backed server-credential handle
  read per handshake, so `reload()` installs rotated certificates without
  rebinding the socket. Installed via `with_tls_config_source` /
  `with_grpc_tls_config_source`; the existing setters keep their signatures
  as static sources. Reload is fail-closed: a failed read keeps the
  last-good credentials serving, logs at ERROR, and returns the error. (#68)
- **tls**: Four ways to trigger a server-credential reload, layered on one
  shared implementation: `ServiceBuilder::with_tls_reload(hook)` hands the
  hook a `TlsReloadHandle` over every reloadable source;
  `ActonService::tls_config_source()` / `grpc_tls_config_source()` are
  unopinionated accessors for callers whose lifecycle does not fit a
  callback; `reload_interval_secs` opts into a poll that fingerprints file
  contents (never mtimes, which `cp -p` preserves) and reloads only on
  change, with failed ticks keeping the baseline so half-written
  certificates self-heal; and `reload_on_sighup` reloads every reloadable
  source from one Unix signal handler, without touching the SIGINT/SIGTERM
  shutdown path. The standalone `Server::serve` path builds a reloadable
  source and installs the same config-driven triggers. When `[grpc.tls]` is
  absent the gRPC listener shares the `[tls]` source, and the handle
  deduplicates that case (`TlsConfigSource::ptr_eq` is public). (#79)
- **client-tls**: A client-side mutual-TLS identity for outbound calls, the
  outbound mirror of `[tls]`. `ClientIdentityConfig` names the certificate,
  key, and optional peer-CA bundle (`root_ca_path`, additive to the webpki
  roots unless `exclusive_roots` pins trust to the bundle alone), and the
  new `client_tls` module turns it into a rustls `ClientConfig`, a
  `reqwest::Identity` or `ClientBuilder`, or a tonic `ClientTlsConfig`.
  Every entry point validates eagerly — including an explicit `keys_match`
  check — so a parseable-but-mismatched pair fails at configuration time
  rather than on the first live handshake, and the concatenated PEM buffer
  reqwest requires is zeroized on drop. (#71)
- **client-tls**: `ClientIdentitySource`, a rotatable outbound identity that
  swaps in place: rustls consults a `ResolvesClientCert` resolver once per
  handshake, so `reload()` is a pointer store and nothing above the TLS
  layer is rebuilt. `client()` returns one stable `reqwest::Client` for the
  source's lifetime — caching it is correct, the connection pool survives a
  rotation, and `grpc_channel(Endpoint)` builds a tonic channel that rotates
  the same way. A reload rereads everything the config names, identity and
  peer trust anchors together, all-or-nothing: a new certificate alongside
  an unreadable CA bundle installs neither, and any failure keeps the
  last-good credentials, logs at ERROR, and returns the error. (#71, #90)

## [acton-service-v0.29.0] - 2026-07-19

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
- **grpc**: gRPC routes now receive framework-managed token authentication
  and Cedar authorization. Merged axum routers do not inherit each other's
  layers, so the gRPC surface was previously mounted with *none* of the
  HTTP-side auto-applied middleware: a service configuring `[token]` and
  `[cedar]` served every gRPC method unauthenticated and unauthorized unless
  the developer hand-wired interceptors per service. With `[token]`
  configured, a `GrpcTokenAuthLayer` now validates the `authorization`
  metadata and injects `Claims`; with `[cedar]` enabled, each method is
  authorized as `Action::"/package.Service/Method"`. Health
  (`grpc.health.v1.Health`) and reflection services stay credential-free for
  infrastructure probes, and `public_paths` prefixes are honored for
  intentionally public methods. Deployments that relied on open gRPC
  alongside a configured `[token]` section must list those methods in
  `public_paths`. (#36)
- **cedar**: An enabled `[cedar]` section whose initialization fails (e.g.
  unreadable or invalid policy file) is now a hard startup failure surfaced
  via `try_build()`/`serve()`. Previously it logged a warning and served
  every route with **no policy enforcement at all** — the same
  silently-weaker-than-configured class as the TLS and token-auth degrades
  fixed by #41. (#36)

### Breaking changes

- **observability**: The OpenTelemetry stack moves 0.30 → 0.31
  (`opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp` 0.31;
  `tracing-opentelemetry` 0.32). These crates are mutually version-locked,
  so consumers pinned to 0.30 must move together with this bump. The
  archived `tower-otel-http-metrics` dependency is replaced by the official
  `opentelemetry-instrumentation-tower`, which also removes a duplicate
  tonic from the dependency tree. (#42)
- **resilience**: The retry configuration is removed —
  `retry_enabled`, `retry_max_attempts`, `retry_base_delay_ms`,
  `retry_max_delay_ms`, and the `with_retry()` / `with_retry_max_attempts()`
  builder methods. It never had a consumer: retrying means replaying a
  request, and an inbound `Request<Body>` wraps a stream that is consumed
  once, while buffering every body to make it replayable would be a
  memory-exhaustion risk on a public endpoint. Retry belongs on outbound
  client stacks, and the docs now say so instead of promising a layer that
  cannot ship. (#32)
- **resilience**: `bulkhead_max_queued` is renamed to
  `bulkhead_max_wait_ms`. The old key had no consumer and no counterpart
  concept in the bulkhead layer; the new one maps to `max_wait_duration`,
  which the layer does support and TOML previously could not set. (#32)
- **resilience**: `circuit_breaker_layer()` loses its type parameters and
  the spurious `Req: Clone` bound. The old first parameter was the
  *response* type mislabeled as the request, so any caller who followed the
  old doctest's turbofish was constructing a nonsense layer. (#33)
- **grpc**: `build()` now refuses when gRPC services are registered but can
  never be served — `config.grpc` absent or `enabled = false` previously
  discarded the registered routes silently and started HTTP-only, with
  every RPC failing at the client as an opaque transport error. The error
  is reported through the same deferred path as the TLS and Cedar checks,
  distinguishes the two causes, and states the remedy. Any code this breaks
  was already not serving gRPC. `GrpcConfig` also gains the previously
  missing `Default` impl. (#53)
- **features**: `full` now includes `audit` and `oauth` — both were
  missing, which is why CI never compiled the audit subsystem and several
  shipped audit bugs went undetected. Because audit logging is enabled by
  default when compiled in, services built with `full` start producing
  audit events on upgrade; set `[audit] enabled = false` to opt out. (#26)

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
- **grpc**: `GrpcTokenAuthLayer` — an HTTP-level, `NamedService`-forwarding
  token authentication layer for manual gRPC stack composition (validates
  bearer tokens via any `TokenValidator`, injects `Claims` into request
  extensions, answers failures with `UNAUTHENTICATED`). The framework
  applies it automatically when `[token]` is configured; the type is public
  for hand-rolled stacks. (#36)
- **examples**: `cedar-grpc` — a runnable end-to-end example of
  framework-managed gRPC authentication + Cedar authorization, with demo
  tokens printed at startup and grpcurl commands for the allow, deny, and
  unauthenticated paths. (#36)
- **observability**: New `prometheus-metrics` feature — a `GET /metrics`
  Prometheus text-exposition endpoint mounted on the main listener
  alongside `/health` and `/ready`, backed by an `opentelemetry-prometheus`
  pull reader on the shared `SdkMeterProvider`. Push (`otel-metrics`) and
  scrape (`prometheus-metrics`) are independently selectable; enabling both
  feeds two readers from one meter provider. `ServiceBuilder` initializes
  the meter provider and applies the HTTP metrics layer automatically from
  `[middleware.metrics]`. (#42)
- **audit**: Every declared `AuditEventKind` is now actually emitted.
  `TypedSession<AuthSession>::logout()` emits `AuthLogout` with the
  request's resolved `AuditSource`; new decorators wrap app-constructed
  auth storage so emission lives in one place per family instead of per
  backend — `AuditedRefreshStorage` (`AuthTokenRefresh` on rotation),
  `AuditedApiKeyStorage` (`AuthApiKeyCreated` / `AuthApiKeyRevoked`), and
  `AuditedOAuthProvider` (`AuthOAuthCallback` on code-exchange success and
  failure). The governor rate limiter now emits `HttpRequestDenied`,
  matching the token-bucket limiter. (#16)

### Notes

- The strict behavior is the default rather than an opt-in flag: there is no
  coherent posture in which "attempt TLS, but plaintext is acceptable" is
  the intended outcome of an enabled TLS section. Deployments that were
  unknowingly relying on the degrade will now fail to start — which is the
  point, and the failure names the cert path that could not be loaded.

### Fixes

- **cedar**: The gRPC `CedarAuthzLayer`/`CedarAuthzService` is now usable.
  The previous `Service` impl was generic over `tonic::Request` with
  `Error = Status` — bounds no tonic generated server (or anything else)
  satisfies — so the layer could not wrap any service, and it read the
  method from the `:path` metadata key, which HTTP/2 pseudo-headers never
  populate. The service now operates at the HTTP level
  (`http::Request<B>` → `http::Response<B>`, gRPC statuses for denials),
  takes the method from the request URI, and forwards `NamedService` from
  the inner service, so a wrapped service registers cleanly with
  `GrpcServicesBuilder::add_service`. (#36)
- **grpc**: `LoggingLayer`, `GrpcTracingLayer`, and `GrpcRateLimitLayer`
  are now usable — they shared the exact defect class fixed for the Cedar
  layer above: `Service` impls bound on `tonic::Request` with
  `Error = Status`, which nothing satisfies, making all three publicly
  exported layers dead code, plus the same `:path`-metadata read that can
  never yield the method. All three now operate at the HTTP level with
  forwarding `NamedService` impls and take the method from the request URI.
  `GrpcTracingService` also instruments the request future with its span
  instead of holding a span guard across an await. (#52)
- **grpc**: `GrpcRateLimitLayer` actually rate limits. It was a placeholder
  that emitted a trace log and passed every request through regardless of
  configuration. It now enforces a governor token bucket
  (`requests_per_period` per `period_secs`, bursts up to `burst_size`)
  shared across every service the layer wraps, answering excess requests
  with `RESOURCE_EXHAUSTED`; health and reflection methods are exempt so
  infrastructure probes are never throttled. (#52)
- **audit**: Database-backed audit storage is now actually attached. The
  builder previously hardcoded the audit logger's storage to `None`, so a
  service enabling `audit` plus a database feature got tracing/syslog-only
  auditing and persisted nothing — the append-only backends were fully
  implemented but never wired. `ServiceBuilder::build()` now selects the
  backend matching the enabled database feature (PostgreSQL, Turso,
  SurrealDB, or ClickHouse) and attaches it. (#34)
- **audit**: Storage resolves lazily rather than eagerly. Pool agents connect
  asynchronously and are still unconnected when the audit agent spawns, so
  reading a connected pool at build time would observe `None` and latch a
  storage-less logger permanently. The selected backend now holds the shared
  pool handle and constructs the concrete storage — running its append-only
  DDL exactly once — on first use after the pool connects.
- **audit**: The agent now waits (up to 30s) for storage readiness before
  initializing the hash chain. Previously an unready backend caused the chain
  to restart at sequence 0, which would fork the chain and collide with
  persisted sequence numbers on the first append.
- **audit**: Events emitted before the hash chain finishes its async
  initialization are no longer silently dropped. They are buffered in agent
  state (bounded at 1024) and sealed in emission order the moment the chain
  loads, ahead of anything received later — so the `ConfigLoaded`
  compliance event emitted during `build()` reliably reaches storage in a
  fresh process, and early auth events survive the window while a lazy
  DB-backed pool connects. On overflow the drop is reported through the
  storage failure tracker (threshold/cooldown/webhook alerting), never
  silently. Persistence also moves from a task-per-event to a single
  sequential writer, so chain seal order is now guaranteed write order.
  (#61)
- **audit**: `AuditSource` is resolved once per request by a new
  `RequestContext` middleware (first `X-Forwarded-For` hop, then
  `X-Real-IP`, then the TCP peer address) and read from request extensions
  by every audit emission point. Token-failure events previously built
  their source from raw headers before enrichment ran and recorded blank
  client IPs and request IDs. (#17)
- **builder**: Every remaining `block_in_place` bridge in
  `ServiceBuilder::build()` — background worker spawn, actor extensions,
  Cedar initialization, Redis session initialization, and key rotation —
  is now guarded by a runtime-flavor check. On a current-thread runtime
  each records a subsystem-named startup error suggesting a multi-threaded
  runtime or disabling the subsystem, surfaced by `try_build()`/`serve()`,
  instead of panicking deep inside tokio with an unactionable message. The
  audit agent and broker received the same guard via #26. (#54)
- **resilience**: The circuit breaker and bulkhead actually work on axum
  routers. Beyond the reported unsatisfiable `Req: Clone` bound (fixed by
  the `tower-resilience` 0.4.7 → 0.10.0 upgrade), the layers were silently
  inert: axum re-invokes `Layer::layer` per request and `build()` minted
  fresh state on each application, so the breaker's failure window reset
  between requests and every request received its own full set of bulkhead
  permits. Both now share state via `build_with_handle()`. A new
  `http_circuit_breaker_layer()` ships a 5xx-aware classifier, since
  inbound axum routes are infallible and the default `Err`-counting
  classifier can never fire; `apply_resilience()` wires classifier, 503
  error handling, and bulkhead-inside-breaker ordering. The regression
  test asserts the breaker actually opens end-to-end, not merely that the
  stack compiles. (#33)
- **resilience**: `[middleware.resilience]` is now applied. Its only
  consumer used to be a startup log line announcing protection that was
  never attached; `ServiceBuilder` now wires circuit breaker and bulkhead
  from the section via `apply_resilience`, reports what was actually
  applied, and warns when the section is present but the `resilience`
  feature is compiled out. (#32)
- **grpc**: Health reflection works. `GrpcServicesBuilder::build()` now
  registers the `grpc.health.v1.Health` descriptor with the reflection
  service, so `.with_health().with_reflection()` yields a health service
  that grpcurl and other reflection-driven clients can discover — it was
  previously routable but invisible. (#53)
- **examples**: The `single-port` gRPC example actually serves gRPC. It
  mutated `config.grpc` through an `Option` that `Config::default()` leaves
  `None`, so the enabling code never executed and every documented grpcurl
  command failed against an HTTP-only service; it also skipped the health
  service by building without state and routed its documented GET to the
  wrong handler. All documented commands are now verified against a
  running instance. (#53)
- **oauth**: `generate_state()` compiles again under rand 0.10 — a latent
  API break that CI never caught because `oauth` was missing from the
  `full` feature set. (#16)

### Internal

- **ci**: The `full` matrix leg finally compiles the audit subsystem
  (see the `full` feature change above), and a new `audit-storage` job
  covers each persistent backend — Turso, SurrealDB, and ClickHouse — in
  its valid feature combination. (#26)
- **ci**: protoc is installed from the Ubuntu archive instead of GitHub
  release downloads, removing a rate-limited external dependency from
  every run. (#48)
- **cli**: `acton-cli` is marked `publish = false`; the crate is a project
  scaffolding tool consumed from the repository, not a library surface
  worth versioning on crates.io. (#58)

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
