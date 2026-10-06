# Development validation and release qualification

Ordinary pull requests should compile only the configurations they can affect.
Releases must qualify the exact commit to be published against every supported
configuration, including Windows and alternate storage/crypto providers.

## Implementation

1. Define the supported profiles once in `scripts/ci/profiles.py`. Execute their
   commands without shell interpolation through `run_profile.py`.
2. Select default/minimal Linux profiles and affected feature/package profiles
   using a pure path classifier. Unknown paths, dependency changes, unreliable
   diffs, and `[full-ci]` select exhaustive coverage. Diff pull requests from
   their merge base and test GitHub's merge commit. Check combined code on main.
3. Preserve the required `ci-gate` check. Validate selected job results explicitly;
   unexpected skips, failed selection, cancellation, and failure are errors.
4. Cancel superseded pull-request runs only. Keep main and release validation
   uncancelled. Isolate persistent self-hosted artifacts by profile.
5. Reuse Rust, documentation, and security workflows. A nightly/manual/full
   qualification workflow runs all profiles, doctests, policy checks, SBOM,
   and a workspace publication dry run at one immutable SHA.
6. Resolve release tags once; validate workspace version and main ancestry;
   qualify that SHA; then authenticate and publish those same packages from that
   SHA in dependency order. Never treat a green nightly at another SHA as proof.
7. Replace the local release task's direct publication route with preparation
   and dispatch of the gated GitHub workflow.
8. Verify selection, strict gates, workflow wiring, publication graph, and exact
   commit checks using Python unit tests and actionlint. Validate the extracted
   Rust packages and all supported Linux facade feature configurations with
   Clippy, Nextest, doctests, and a publication dry run.

## Architectural compatibility

Standalone core/audit contracts and database packages are independently tested.
`acton-service` remains the public facade. Dependency modifications use Cargo
commands. The additive package extraction targets workspace version 0.47.0;
the final API assessment will confirm that release choice.

The exhaustive qualification run intentionally remains expensive. Its purpose
is to establish release readiness, while the development gate gives timely
feedback on the paths a change affects.
