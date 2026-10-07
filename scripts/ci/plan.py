"""Select validation profiles conservatively from a reliable Git change set."""

import argparse
import json
import os
import re
import subprocess
from pathlib import Path

from profiles import PROFILES, QUALIFICATION, matrix

BACKENDS = {"postgres", "mssql", "turso", "surrealdb", "clickhouse"}
GRPC_INTEGRATION = {"grpc-integration", "grpc-integration-ring"}
BACKEND_PROFILES = {
    "postgres": {
        "postgres",
        "postgres-ring",
        "postgres-integration",
        "postgres-integration-ring",
        "full",
    },
    "mssql": {"mssql", "windows-mssql"},
    "turso": {"audit-turso"},
    "surrealdb": {"audit-surrealdb"},
    "clickhouse": {"audit-clickhouse"},
}
FEATURE_PATHS = (
    ("auth/oauth/", {"oauth-no-cache", "oauth-with-cache"}),
    ("auth/saml/", {"saml", "full", "windows-saml", "windows-full"}),
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
    (
        "grpc/",
        {"grpc-no-tls", "grpc-tls", "grpc-examples", "windows-grpc", *GRPC_INTEGRATION},
    ),
    ("build_utils.rs", {"grpc-integration", "grpc-integration-ring"}),
    (
        "crypto.rs",
        {
            "default",
            "minimal",
            "full",
            "ring",
            "tls-ring",
            *GRPC_INTEGRATION,
            "postgres-integration",
            "postgres-integration-ring",
        },
    ),
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
    (
        "tls.rs",
        {
            "tls-no-grpc",
            "grpc-tls",
            *GRPC_INTEGRATION,
            "ring",
            "tls-ring",
            "windows",
            "windows-auth-native",
            "windows-grpc",
        },
    ),
    (
        "client_tls.rs",
        {
            "tls-no-grpc",
            "grpc-tls",
            *GRPC_INTEGRATION,
            "ring",
            "tls-ring",
            "windows",
            "windows-auth-native",
            "windows-grpc",
        },
    ),
    (
        "caller_auth.rs",
        {
            "tls-no-grpc",
            "grpc-tls",
            *GRPC_INTEGRATION,
            "ring",
            "tls-ring",
            "windows",
            "windows-auth-native",
            "windows-grpc",
        },
    ),
    ("windows_auth.rs", {"windows-auth", "windows-auth-native"}),
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


HARNESS_TESTS = {
    "mssql_integration": {"mssql-integration"},
    "postgres_integration": {"postgres-integration", "postgres-integration-ring"},
    "surrealdb_integration": {"surrealdb-integration"},
    "clickhouse_integration": {"clickhouse-integration"},
}
EXAMPLE_PROFILES = {
    "examples/basic/simple-api.rs": {"default", "minimal"},
    "examples/basic/users-api.rs": {"default", "minimal"},
    "examples/authorization/cedar-authz.rs": {"full"},
    "examples/observability/test-metrics.rs": {"otel-only", "metrics"},
    "examples/observability/test-observability.rs": {"default"},
    "examples/observability/test-prometheus-metrics.rs": {"metrics"},
    "examples/database/database-api.rs": {"full"},
    "examples/websocket/chat-server.rs": {"full"},
    "examples/htmx/task-manager.rs": {"frontend"},
    "examples/graphql/graphql-basic.rs": {"graphql"},
}


def docs_only(path: str) -> bool:
    return (
        path.startswith(
            ("acton-docs/", "docs/", ".remember/", ".github/ISSUE_TEMPLATE/")
        )
        or re.fullmatch(r"[^/]+\.md", path) is not None
        or path
        in {
            f"{name}/README.md"
            for profile in PROFILES.values()
            for name in (profile.package, profile.companion)
            if name
        }
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
    tooling = any(p.startswith((".github/workflows/", "scripts/ci/")) for p in paths)
    docs = docs or ".github/workflows/deploy-docs.yml" in paths
    selected: set[str] = set()
    reasons: list[str] = []
    if full:
        selected.update(QUALIFICATION)
        return {
            "matrix": matrix(selected),
            "code": True,
            "docs": True,
            "security": True,
            "tooling": True,
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
                relative = path.removeprefix("acton-service-integration-tests/")
                if relative.startswith(("proto/", "examples/")) or relative in {
                    "build.rs",
                    "tests/grpc_tls_integration.rs",
                    "tests/protobuf_helper.rs",
                }:
                    selected.update(
                        {"grpc-examples", *GRPC_INTEGRATION, "windows-grpc"}
                    )
                elif (
                    relative.startswith("tests/")
                    and Path(relative).stem in HARNESS_TESTS
                ):
                    selected.update(HARNESS_TESTS[Path(relative).stem])
                else:
                    selected.update(
                        {
                            name
                            for name, profile in PROFILES.items()
                            if profile.package == "acton-service-integration-tests"
                        }
                    )
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
            if path.startswith("acton-service/examples/"):
                relative = path.removeprefix("acton-service/")
                selected.update(EXAMPLE_PROFILES.get(relative, PROFILES.keys()))
                reasons.append(f"Example target and conditional features: {path}")
                continue
            if path.startswith("acton-service/tests/"):
                test = Path(path).name
                test_profiles = (
                    ("oauth", {"oauth-no-cache", "oauth-with-cache"}),
                    ("saml", {"saml", "windows-saml"}),
                    ("graphql", {"graphql"}),
                    ("grpc", {"grpc-no-tls", "grpc-tls", *GRPC_INTEGRATION}),
                    ("metrics", {"minimal", "otel-only", "metrics", "full"}),
                    (
                        "tls",
                        {
                            "tls-no-grpc",
                            "grpc-tls",
                            *GRPC_INTEGRATION,
                            "ring",
                            "tls-ring",
                        },
                    ),
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

    # Composite jobs already execute these exact fixtures; retain standalone jobs
    # only for isolated fixture edits whose facade integration is not selected.
    for composite, fixture in (
        ("mssql", "mssql-integration"),
        ("audit-surrealdb", "surrealdb-integration"),
        ("audit-clickhouse", "clickhouse-integration"),
    ):
        if composite in selected:
            selected.discard(fixture)
    return {
        "matrix": matrix(selected),
        "code": bool(selected),
        "docs": docs,
        "security": security,
        "tooling": tooling,
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
    Path("ci-plan.json").write_text(json.dumps(plan, indent=2) + "\n")
    print(json.dumps(plan, indent=2))
    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with open(output, "a", encoding="utf-8") as stream:
            for name in ("matrix", "code", "docs", "security", "tooling"):
                stream.write(
                    f"{name}={json.dumps(plan[name], separators=(',', ':'))}\n"
                )


if __name__ == "__main__":
    main()
