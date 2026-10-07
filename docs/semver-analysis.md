# Semver analysis for acton-service 0.47.0

Reviewed on 2026-10-07 against published `acton-service-v0.46.0`
(`7fdacbac0b3781e08bc3737eb2ff32d38d3dbca7`). The review covers the component
architecture, CI changes, and authentication fixes for #175 and #176.

## Recommendation

Publish **0.47.0**, using the established tag `acton-service-v0.47.0`.
This is a compatibility boundary for a pre-1.0 crate. Cargo classifies several
changes below as major compatibility changes; incrementing `0.y` supplies that
boundary. One bump covers all changes in the release. The workspace is already
at 0.47.0, so no additional minor increment is required.

## Confirmed breaking changes

| Change | Required migration |
| --- | --- |
| `Config<T>` adds the public `revocation` field | Update complete struct literals, or use defaults. Omitted serialized configuration remains disabled. |
| `ApiKeyConfig` adds the public `pepper_path` field | Update complete literals and supply a persistent secret before initializing a generator. |
| `ApiKeyGenerator::new` requires `ApiKeyPepper` | Pass an explicit secret shared by all issuers and verifiers. |
| `ApiKeyGenerator::generate` returns `Result<(String, String), Error>` | Handle or propagate entropy errors before destructuring. |
| All five API-key storage constructors accept a generator instead of a prefix | Pass a clone of the configured generator. SQL Server construction remains async and fallible. |
| Legacy Argon2id API-key digests are rejected immediately | Reissue existing keys, or explicitly rehash securely held plaintext offline. A stored hash alone cannot be converted. |
| `GrpcTokenAuthService` requires a `'static` validator for its `Service` implementation | Borrowing validator implementations may need owned state. The existing interceptor bound is unchanged. |
| Embedded SurrealDB `mem://` and `memory://` connections are rejected | Connect to a remote HTTP or WebSocket server, including a server using memory storage during development. |

These classifications follow the official [Cargo SemVer guidelines](https://doc.rust-lang.org/cargo/reference/semver.html)
for adding fields to public structs, changing function signatures, tightening
bounds, and changing behavior.

## Additions and compatibility qualifications

Seven standalone core, audit, and database packages supply additional public
entry points while the facade retains its storage wrappers. Explicit revocation
configuration, backend providers, builder injection, and the state getter add
persistent token-ID and subject-cutoff checks to HTTP and builder-managed gRPC.

New methods on `TokenValidator` and `TokenRevocation` have default implementations;
existing required methods retain their signatures. Cargo treats adding defaulted
trait methods as potentially breaking, rather than adding required trait items.
Existing jti-only implementations remain supported.

`AuditEventKind` is exhaustive and shared by facade and component dependencies.
Enabling account or lockout features through a component can add variants visible
through the facade. Align features and update exhaustive matches. Redis reserves
the token ID `subject-cutoffs` to protect its persistent subject-cutoff hash.

CI selection, caching, same-tree merge reuse, validated artifact deployment, and
private integration-harness organization do not independently require another
compatibility bump. Release qualification still validates the exact tagged SHA.

## Authentication migration

Provision exactly 32 raw bytes for the API-key pepper, outside API-key storage.
Configure `auth.api_keys.pepper_path`, or construct `ApiKeyPepper` explicitly.
Keep the same secret across issuers, verifiers, replicas, and restarts. Pass the
configured generator into storage and handle `generate()?`.

API-key digest columns and lookup-prefix indexes need no schema change. Reissue
legacy keys before upgrading, or call `generator.hash(&plaintext)` during a
trusted offline migration when plaintext is already securely available. Pepper
replacement invalidates existing digests and requires coordinated reissuance or
plaintext migration. Verification has no legacy or old-pepper fallback.

Select `[revocation]` explicitly and initialize its schema before serving.
Persist revocation successfully before reporting deactivation as complete.
Subject cutoffs never decrease or expire; they deny tokens issued at or before
the cutoff and tokens without an issued-at claim when a cutoff exists. Prevent
new token issuance for deactivated principals as well.

Synchronous gRPC interceptors validate cryptography only. Use builder-managed
authentication or `GrpcTokenAuthLayer` for asynchronous revocation checks.
See the [API-key guide](../acton-docs/src/app/docs/api-keys/page.md) and
[token-authentication guide](../acton-docs/src/app/docs/token-auth/page.md).

## Review limits and release evidence

The expected local semver reference was absent, so review used the official
Cargo reference linked above. This report classifies source/API changes;
Clippy, Nextest, hosted PR evidence, and exhaustive release qualification provide
separate correctness evidence. Publication must use the qualified commit and
validate the checksums and clean source revision of every published archive.
