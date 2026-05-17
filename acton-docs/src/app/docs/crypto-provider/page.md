---
title: Crypto Provider
nextjs:
  metadata:
    title: Crypto Provider
    description: How acton-service selects the rustls CryptoProvider — aws-lc-rs is the default, with ring as an opt-in alternative. Includes FIPS guidance and migration notes.
---

acton-service uses `rustls` for all TLS — the HTTPS listener, the `reqwest`
HTTP client, the `sqlx` Postgres connector, and the `tonic` gRPC stack. Since
rustls 0.23, the cryptographic primitives have been pluggable via the
[`CryptoProvider`](https://docs.rs/rustls/latest/rustls/crypto/struct.CryptoProvider.html)
trait. The framework ships with two providers and picks one at compile time.

---

## Default: aws-lc-rs

`crypto-aws-lc-rs` is enabled by default. It uses
[`aws-lc-rs`](https://crates.io/crates/aws-lc-rs), AWS's Rust bindings to
[AWS-LC](https://github.com/aws/aws-lc), which is a fork of BoringSSL
maintained for AWS production use.

It is the default because:

- **FIPS 140-3 capable.** `aws-lc-rs` exposes a `fips` feature that links the
  FIPS 140-3 validated AWS-LC module. `ring` has no FIPS validation. For
  FedRAMP, FISMA, or DoD workloads, this is the only viable path.
- **Ecosystem-aligned.** rustls 0.23+, tonic 0.14+, and sqlx 0.8+ all prefer
  `aws-lc-rs` when both providers are available.
- **Faster on server hardware.** AWS's tuned AES-GCM and ChaCha20 assembly
  outperforms `ring` by roughly 1.5–2× on x86_64 with AES-NI — a measurable
  win on a TLS-terminating service.
- **Maintained continuously.** A full-time AWS team funds AWS-LC with
  continuous fuzzing and audits.

## Opt-in: ring

`crypto-ring` is available for environments where the C/CMake toolchain that
`aws-lc-rs` requires at build time is impractical — minimal Alpine images,
some cross-compilation targets, or build pipelines that cannot install a C
compiler.

Enable it explicitly:

```toml
[dependencies]
acton-service = { version = "{% $version.acton %}", default-features = false, features = [
    "http",
    "observability",
    "crypto-ring",
] }
```

## Mutual exclusivity

Exactly one of `crypto-aws-lc-rs` or `crypto-ring` must be enabled. The
framework guards the invariant with a `compile_error!`:

```text
error: Features `crypto-aws-lc-rs` and `crypto-ring` are mutually exclusive.
       Enable exactly one rustls crypto provider.
```

If you disable default features without selecting a provider, you'll see:

```text
error: A rustls crypto provider is required.
       Enable `crypto-aws-lc-rs` (default) or `crypto-ring`.
```

## Bootstrapping at runtime

`rustls 0.23+` panics on `ServerConfig::builder()` or `ClientConfig::builder()`
when more than one provider is compiled into the binary and no default is
installed — which happens routinely because dependencies such as
[`quinn-proto`](https://crates.io/crates/quinn-proto) and
[`jsonwebtoken`](https://crates.io/crates/jsonwebtoken) link `aws-lc-rs`
transitively regardless of your choice.

acton-service installs the chosen provider automatically before the TLS
listener starts. If your binary uses `reqwest`, `sqlx`, or `tonic` TLS
clients *without* mounting the framework's TLS listener, call the bootstrap
once from `main`:

```rust
use acton_service::crypto::ensure_default_crypto_provider;

#[tokio::main]
async fn main() -> Result<()> {
    ensure_default_crypto_provider();
    // ... your service setup ...
    Ok(())
}
```

The call is idempotent — repeat calls are no-ops.

## FIPS 140-3

To use the FIPS-validated AWS-LC module, build with the `aws-lc-rs` crate's
`fips` feature enabled in your downstream binary. See the
[aws-lc-rs FIPS documentation](https://github.com/aws/aws-lc-rs/blob/main/aws-lc-rs/README.md#fips)
for build prerequisites (FIPS-validated module package and a supported host
triple). acton-service does not gate this behind its own feature; it simply
delegates to whichever build of `aws-lc-rs` Cargo resolves.

## A note on `cargo tree`

`crypto-ring` builds will still show `aws-lc-rs` in `cargo tree` because
`quinn-proto` (pulled in by `reqwest`'s HTTP/3 path) and `jsonwebtoken`'s
`rust_crypto` feature link it unconditionally. The *installed* provider —
the one rustls actually uses for handshakes and encryption — is whichever
feature you enabled, not whatever happens to be in the binary.

## Related

- [Feature Flags](/docs/feature-flags) — the full feature catalog
- [Production Checklist](/docs/production) — TLS setup at deployment time
- [Token Generation](/docs/token-generation) — PASETO/JWT crypto context
