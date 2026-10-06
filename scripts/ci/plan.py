"""Select validation profiles conservatively from a reliable Git change set."""

import argparse
import json
import os
import re
import subprocess
from pathlib import Path

from profiles import PROFILES, matrix

BACKENDS = {"postgres", "mssql", "turso", "surrealdb", "clickhouse"}
BACKEND_PROFILES = {
    "postgres": {"postgres", "full"},
    "mssql": {"mssql-adapter", "mssql", "mssql-integration", "windows"},
    "turso": {"turso-adapter", "audit-turso"},
    "surrealdb": {"surrealdb-adapter", "audit-surrealdb"},
    "clickhouse": {"clickhouse-adapter", "audit-clickhouse"},
}
FEATURE_PATHS = (
    ("auth/oauth/", {"oauth-no-cache", "oauth-with-cache"}),
    ("auth/saml/", {"saml", "full", "windows"}),
    ("auth/tokens/", {"tokens", "full"}),
    ("auth/", {"tokens", "oauth-no-cache", "saml", "full"}),
    ("accounts/", {"full", "mssql", "audit-turso", "audit-surrealdb"}),
    ("lockout/", {"full"}),
    (
        "audit/storage/",
        {
            "audit-nodb",
            "full",
            "mssql",
            "audit-turso",
            "audit-surrealdb",
            "audit-clickhouse",
        },
    ),
    ("audit/", {"audit-nodb", "audit-otel", "full"}),
    ("grpc/", {"grpc-no-tls", "grpc-tls", "grpc-examples"}),
    ("graphql/", {"graphql", "full"}),
    ("session/", {"frontend", "full"}),
    ("htmx/", {"frontend"}),
    ("templates/", {"frontend"}),
    ("sse/", {"frontend"}),
    ("websocket/", {"full"}),
    ("repository/", {"postgres", "full"}),
    ("handlers/", {"full"}),
    ("middleware/metrics.rs", {"otel-only", "metrics", "full"}),
    ("middleware/", {"full", "tokens", "tls-no-grpc", "graphql"}),
    ("tls.rs", {"tls-no-grpc", "grpc-tls", "ring", "windows"}),
    ("client_tls.rs", {"tls-no-grpc", "grpc-tls", "ring", "windows"}),
    ("caller_auth.rs", {"tls-no-grpc", "grpc-tls", "ring", "windows"}),
    ("windows_auth.rs", {"windows-auth", "windows"}),
    ("metrics_exporter.rs", {"otel-only", "metrics", "full"}),
    ("observability.rs", {"otel-only", "metrics", "audit-otel", "full"}),
    ("database.rs", BACKEND_PROFILES["postgres"]),
    ("mssql.rs", BACKEND_PROFILES["mssql"]),
    ("turso.rs", BACKEND_PROFILES["turso"]),
    ("surrealdb_backend.rs", BACKEND_PROFILES["surrealdb"]),
    ("clickhouse_backend.rs", BACKEND_PROFILES["clickhouse"]),
    ("cache.rs", {"oauth-with-cache", "full"}),
    ("events.rs", {"full"}),
    ("openapi.rs", {"full"}),
    ("pagination.rs", {"frontend", "full"}),
    ("checks.rs", {"full"}),
    ("health.rs", {"full"}),
    ("versioning.rs", {"full", "graphql"}),
    ("responses.rs", {"full"}),
    ("ids.rs", {"full"}),
)


def docs_only(path: str) -> bool:
    return (
        path.startswith(
            ("acton-docs/", "docs/", ".remember/", ".github/ISSUE_TEMPLATE/")
        )
        or re.fullmatch(r"[^/]+\.md", path) is not None
        or path in {f"{profile.package}/README.md" for profile in PROFILES.values()}
        or path.startswith("acton-service/examples/")
        and path.endswith("/README.md")
        or path.startswith("LICENSE")
        or path in {".gitignore", ".github/release.yml", ".github/dependabot.yml"}
        or path.startswith(".github/PULL_REQUEST_TEMPLATE")
        or path
        in {
            f".github/workflows/{name}.yml"
            for name in (
                "claude",
                "claude-code-review",
                "deploy-docs",
                "docs-sync-on-merge",
            )
        }
    )


def select(paths: list[str], full: bool = False) -> dict[str, object]:
    docs = any(p.startswith("acton-docs/") or p == "Cargo.toml" for p in paths)
    security = any(
        Path(p).name in {"Cargo.toml", "Cargo.lock"}
        or p in {"deny.toml", ".cargo/audit.toml"}
        for p in paths
    )
    selected: set[str] = set()
    reasons: list[str] = []
    if full:
        selected.update(PROFILES)
        return {
            "matrix": matrix(selected),
            "code": True,
            "docs": True,
            "security": True,
            "reasons": ["Exhaustive validation requested"],
        }

    for path in paths:
        if docs_only(path):
            continue
        if (
            Path(path).name in {"Cargo.toml", "Cargo.lock"}
            or path.startswith((".cargo/", "scripts/ci/", ".github/workflows/"))
            or path in {"deny.toml", "Taskfile.yml"}
            or path.startswith("rust-toolchain")
        ):
            selected.update(PROFILES)
            reasons.append(f"Shared build/dependency change: {path}")
            continue
        if path.startswith("acton-cli/"):
            selected.add("cli")
            reasons.append(f"CLI change: {path}")
            continue
        for backend, profiles in BACKEND_PROFILES.items():
            if path.startswith(f"acton-service-{backend}/"):
                selected.update(profiles)
                reasons.append(f"{backend} package and facade integration: {path}")
                break
        else:
            if path.startswith(("acton-service-core/", "acton-service-audit/")):
                selected.update(PROFILES)
                reasons.append(f"Shared contracts affect every adapter: {path}")
                continue
            if path.startswith("acton-service-integration-tests/"):
                selected.update({"grpc-examples", "mssql-integration"})
                reasons.append(f"Integration harness: {path}")
                continue
            if path.startswith("acton-service/src/"):
                relative = path.removeprefix("acton-service/src/")
                for prefix, profiles in FEATURE_PATHS:
                    if (
                        relative == prefix
                        or relative.startswith(prefix)
                        and prefix.endswith("/")
                    ):
                        selected.update(profiles)
                        reasons.append(f"Affected feature: {path}")
                        break
                else:
                    selected.update(PROFILES)
                    reasons.append(f"Shared or unclassified source: {path}")
                continue
            if path.startswith("acton-service/tests/"):
                test = Path(path).name
                test_profiles = (
                    ("oauth", {"oauth-no-cache", "oauth-with-cache"}),
                    ("saml", {"saml", "windows"}),
                    ("graphql", {"graphql"}),
                    ("grpc", {"grpc-no-tls", "grpc-tls"}),
                    ("metrics", {"minimal", "otel-only", "metrics", "full"}),
                    ("tls", {"tls-no-grpc", "grpc-tls", "ring"}),
                    ("mssql", BACKEND_PROFILES["mssql"]),
                    ("audit", {"audit-nodb", "audit-otel", "full"}),
                    ("governor", {"full"}),
                    ("resilience", {"full"}),
                    ("tracing", {"otel-only", "full"}),
                    ("basic_tracing", {"minimal", "default"}),
                )
                for prefix, profiles in test_profiles:
                    if test.startswith(prefix) or f"/fixtures/{prefix}/" in path:
                        selected.update(profiles)
                        break
                else:
                    selected.update(PROFILES)
                reasons.append(f"Test coverage: {path}")
                continue
            selected.update(PROFILES)
            reasons.append(f"Unclassified path, validating everything: {path}")

    if selected:
        selected.update({"default", "minimal"})
    return {
        "matrix": matrix(selected),
        "code": bool(selected),
        "docs": docs,
        "security": security,
        "reasons": reasons,
    }


def changed_paths(event: str, base: str, head: str, forced: bool) -> list[str]:
    if event not in {"pull_request", "push"} or forced:
        raise ValueError("No reliable incremental diff for this event")
    if not re.fullmatch(r"[0-9a-f]{40}", base) or base == "0" * 40:
        raise ValueError("Missing or invalid base commit")
    if not re.fullmatch(r"[0-9a-f]{40}", head):
        raise ValueError("Missing or invalid head commit")
    if event == "pull_request":
        base = subprocess.check_output(
            ["git", "merge-base", base, head], text=True
        ).strip()
    output = subprocess.check_output(
        ["git", "diff", "--name-only", "--no-renames", "-z", base, head],
    )
    return [
        p.decode("utf-8", errors="surrogateescape") for p in output.split(b"\0") if p
    ]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--full", action="store_true")
    parser.add_argument("--paths", nargs="*")
    args = parser.parse_args()
    full = args.full or "[full-ci]" in os.environ.get(
        "PR_TITLE", ""
    ) + "\n" + os.environ.get("PR_BODY", "")
    if args.paths is not None:
        paths = args.paths
    elif full:
        paths = []
    else:
        try:
            paths = changed_paths(
                os.environ.get("EVENT", ""),
                os.environ.get("BASE_SHA", ""),
                os.environ.get("HEAD_SHA", ""),
                os.environ.get("PUSH_FORCED") == "true",
            )
        except (ValueError, subprocess.CalledProcessError) as error:
            print(f"Incremental selection unavailable ({error}); selecting everything.")
            paths, full = [], True
    plan = select(paths, full)
    print(json.dumps(plan, indent=2))
    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with open(output, "a", encoding="utf-8") as stream:
            for name in ("matrix", "code", "docs", "security"):
                stream.write(
                    f"{name}={json.dumps(plan[name], separators=(',', ':'))}\n"
                )


if __name__ == "__main__":
    main()
