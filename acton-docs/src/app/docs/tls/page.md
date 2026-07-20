---
title: TLS / HTTPS
nextjs:
  metadata:
    title: TLS / HTTPS
    description: Terminating TLS directly in acton-service — server certificates, mutual TLS, and rotating credentials without a restart.
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---

acton-service can terminate TLS directly, without a reverse proxy in front,
using `rustls`. This requires the `tls` feature:

```toml
acton-service = { version = "{% $version.acton %}", features = ["tls"] }
```

TLS also needs exactly one rustls crypto provider enabled — see
[Crypto Provider](/docs/crypto-provider) if you disabled default features.

## Enabling the HTTPS listener

Add a `[tls]` section to your configuration:

```toml
[tls]
enabled = true
cert_path = "./certs/server.pem"       # PEM-encoded certificate chain
key_path = "./certs/server-key.pem"    # PEM-encoded private key
```

`enabled` defaults to `true` once the section is present. A `[tls]` section
whose certificate or key cannot be loaded is a hard failure at startup — the
service refuses to bind rather than silently falling back to plaintext.

## Mutual TLS (verifying client certificates)

Point `client_ca_path` at a PEM bundle of CA certificates to require
connecting clients to present a certificate signed by one of them:

```toml
[tls]
enabled = true
cert_path = "./certs/server.pem"
key_path = "./certs/server-key.pem"
client_ca_path = "./certs/client-ca.pem"
client_auth_optional = false
```

- `client_ca_path` omitted (the default) accepts all clients without a
  certificate — server-only TLS.
- `client_auth_optional = true` still accepts clients that present no
  certificate, but any certificate that *is* presented must verify —
  "optional" means "absent is allowed," never "invalid is allowed." Handlers
  can distinguish the two cases through `TlsConnectInfo`. This flag is
  ignored unless `client_ca_path` is set.

This is calling **into** this service. To present a client certificate when
this service calls another mutual-TLS peer, see the `client_tls` module
described under the [`tls` feature flag](/docs/feature-flags#tls).

## gRPC TLS

The separate-port gRPC listener has its own optional `[grpc.tls]` section:

```toml
[grpc]
enabled = true
use_separate_port = true
port = 9090

[grpc.tls]
enabled = true
cert_path = "./certs/grpc.pem"
key_path = "./certs/grpc.key"
```

When `[grpc.tls]` is present it is authoritative for the gRPC listener:
`enabled = false` serves plaintext gRPC even while `[tls]` is active, useful
for a loopback-only gRPC surface. When `[grpc.tls]` is absent, the gRPC
listener falls back to the shared `[tls]` credentials — reloading either one
rotates both.

## Rotating credentials without a restart

Credentials loaded from `[tls]` / `[grpc.tls]` can be rotated while the
service keeps running: a reload replaces what the *next* handshake uses,
while connections already established are undisturbed. A reload that fails —
missing, unparseable, or half-written files — is logged at `ERROR` and leaves
the previous certificate serving; rotation can never take the listener down
or downgrade it to plaintext.

There are four ways to trigger a reload, from most to least automatic.

### Poll the files

Set `reload_interval_secs` to have the service hash the credential files on
an interval and reload only when their *contents* change:

```toml
[tls]
enabled = true
cert_path = "./certs/server.pem"
key_path = "./certs/server-key.pem"
reload_interval_secs = 300
```

Change is detected by hashing file bytes, not by comparing modification
times — `cp -p` and most certificate-management tooling preserve mtimes
across a real rotation, which an mtime check would miss. A tick whose files
are missing, unreadable, or half-written is retried on the next tick rather
than treated as a rotation, so an in-progress write heals itself. Omit the
field to disable polling; `0` is rejected at startup rather than busy-looping.

### Reload on SIGHUP

```toml
[tls]
reload_on_sighup = true
```

Setting this on *either* `[tls]` or `[grpc.tls]` installs one handler that
reloads every reloadable listener — a signal that rotated only half the
surfaces would be confusing to reason about during an incident. Unix only;
on other platforms a configured value is reported at `WARN` during startup
and otherwise ignored. Pairs well with systemd `ExecReload` or a certbot
deploy hook.

### Register a hook (`ServiceBuilder::with_tls_reload`)

For triggers the config-driven options above don't model — a Vault lease
renewal, a Kubernetes secret watch, an admin endpoint — register a callback.
`ActonService::serve` calls it once, as the listeners come up, with a
`TlsReloadHandle` over every reloadable source the service resolved:

```rust,ignore
let service = ServiceBuilder::new()
    .with_config(config)
    .with_routes(routes)
    .with_tls_reload(|handle| {
        tokio::spawn(async move {
            let mut events = watch_secret_store().await;
            while events.next().await.is_some() {
                for (listener, result) in handle.reload_all() {
                    if let Err(e) = result {
                        tracing::error!("{listener} TLS reload failed: {e}");
                    }
                }
            }
        });
    })
    .build();

service.serve().await?;
```

This is the preferred way to drive rotation from your own code: `serve()`
calls the hook itself, so there's no ordering to get wrong. The hook is
skipped, with a `WARN` explaining why, when no reloadable source resolved —
TLS disabled, or every source injected as an already-loaded `ServerConfig`.

### Hold the source yourself (`tls_config_source` / `grpc_tls_config_source`)

The escape hatch for lifecycles that don't fit a callback. `serve()` consumes
the service, so clone the handle out **before** calling it:

```rust,ignore
let service = builder.build();
let tls = service.tls_config_source();          // before serve()
tokio::spawn(async move {
    if let Some(tls) = tls {
        watch_for_new_certs().await;
        let _ = tls.reload();
    }
});
service.serve().await?;                          // consumes the service
```

`grpc_tls_config_source()` is the gRPC twin. When `[grpc.tls]` is absent it
returns the same source as `tls_config_source()`; use `TlsConfigSource::ptr_eq`
to tell that case apart from two genuinely distinct certificates.

### The plain `Server::serve` path

Services that use `acton_service::server::Server` directly (no
`ServiceBuilder`) get the same `reload_interval_secs` and `reload_on_sighup`
config-driven triggers, through the same shared implementation — one config
file produces the same rotation behavior on either path. There is no
`with_tls_reload` equivalent here, since `Server` has no builder to register
a hook on; a service that needs to reload from a custom trigger should use
`ServiceBuilder`.

## Handshake timeout

Each TLS handshake runs concurrently, off the listener's accept path, bounded
by a per-connection timeout. A peer that completes the TCP connect but never
sends a ClientHello only ties up its own handshake task until the timeout
elapses — it cannot stall accepting or handshaking any other connection.

```toml
[tls]
enabled = true
cert_path = "./certs/server.pem"
key_path = "./certs/server-key.pem"
handshake_timeout_secs = 10
```

Omit the field to use the built-in default of 10 seconds. `0` is rejected at
startup rather than failing every handshake instantly. `[grpc.tls]` accepts
the same field for the separate-port gRPC listener; when absent there, it
inherits the `[tls]` value.

{% callout type="note" title="Unknown keys are rejected" %}
`[tls]` and `[grpc.tls]` reject unrecognized keys at startup instead of
silently ignoring them — a misspelled field like `reload_interval_sec`
(missing the trailing `s`) now fails to parse rather than quietly disarming
certificate rotation.
{% /callout %}

## Related

- [Feature Flags](/docs/feature-flags#tls) — what the `tls` feature enables, including outbound mutual TLS
- [Crypto Provider](/docs/crypto-provider) — selecting the rustls crypto backend TLS depends on
- [Production Checklist](/docs/production) — TLS setup at deployment time
