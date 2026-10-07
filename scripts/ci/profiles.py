"""Supported configurations and their independent behavioral contracts."""

from dataclasses import dataclass


@dataclass(frozen=True)
class Profile:
    package: str = "acton-service"
    features: str = ""
    defaults: bool = True
    tests: bool = True
    doctests: bool = False
    runner: str = "linux"
    protoc: bool = False
    kerberos: bool = False
    companion: str = ""
    compile_only: bool = False
    harness: str = ""

    def cargo_options(self) -> list[str]:
        options = ["--locked", "-p", self.package]
        if not self.defaults:
            options.append("--no-default-features")
        if self.features:
            options.extend(["--features", self.features])
        return options


def narrow(features: str, **options) -> Profile:
    return Profile(
        features=f"http,{features},crypto-aws-lc-rs", defaults=False, **options
    )


PROFILES = {
    "default": Profile(doctests=True),
    "minimal": Profile(features="http,crypto-aws-lc-rs", defaults=False),
    "full": Profile(features="full", doctests=True),
    # Broad compilation stays isolated; provider runtime uses smaller test binaries.
    "ring": Profile(features="full,crypto-ring", defaults=False, tests=False),
    "tls-ring": Profile(
        features="http,tls,otel-metrics,prometheus-metrics,crypto-ring", defaults=False
    ),
    "windows": Profile(runner="windows"),
    "windows-full": Profile(
        features="full", runner="windows", tests=False, compile_only=True
    ),
    "windows-mssql": Profile(
        features="http,mssql,grpc,auth,accounts,audit,observability,crypto-aws-lc-rs",
        defaults=False,
        runner="windows",
        tests=False,
        compile_only=True,
    ),
    "windows-auth-native": Profile(
        features="http,tls,windows-auth,crypto-aws-lc-rs",
        defaults=False,
        runner="windows",
    ),
    "windows-grpc": Profile(
        package="acton-service-integration-tests",
        features="grpc,tls,cedar-authz",
        runner="windows",
        protoc=True,
        tests=False,
        compile_only=True,
    ),
    "windows-saml": Profile(
        features="http,saml,crypto-aws-lc-rs", defaults=False, runner="windows"
    ),
    "audit-turso": narrow("audit,turso,observability", companion="acton-service-turso"),
    "audit-surrealdb": narrow(
        "audit,surrealdb,auth,jwt,observability",
        companion="acton-service-surrealdb",
        harness="surrealdb",
    ),
    "audit-clickhouse": Profile(
        features="audit,clickhouse",
        companion="acton-service-clickhouse",
        harness="clickhouse",
    ),
    "audit-otel": narrow("audit,observability"),
    "mssql": narrow(
        "mssql,grpc,auth,accounts,audit,observability",
        kerberos=True,
        companion="acton-service-mssql",
        harness="mssql",
    ),
    "tls-no-grpc": narrow("tls"),
    "windows-auth": narrow("tls,windows-auth"),
    "saml": narrow("saml"),
    "grpc-no-tls": narrow("grpc"),
    "grpc-tls": narrow("grpc,tls,auth,cedar-authz"),
    "oauth-no-cache": narrow("oauth"),
    "oauth-with-cache": narrow("oauth,cache,jwt"),
    "otel-only": narrow("otel-metrics"),
    "metrics": narrow("otel-metrics,prometheus-metrics,tls"),
    "tokens": narrow("jwt,auth"),
    "frontend": narrow("htmx-full"),
    "graphql": narrow("graphql,graphql-cedar,cedar-authz"),
    "audit-nodb": narrow("audit"),
    "core": Profile(package="acton-service-core"),
    "audit": Profile(package="acton-service-audit"),
    "audit-events": Profile(
        package="acton-service-audit", features="accounts,login-lockout"
    ),
    "postgres": Profile(package="acton-service-postgres"),
    "postgres-ring": Profile(
        package="acton-service-postgres",
        features="crypto-ring",
        defaults=False,
        tests=False,
    ),
    # Runtime adapter tests run in the companion jobs after an isolated lint.
    "cli": Profile(package="acton-cli"),
    "mssql-integration": Profile(
        package="acton-service-integration-tests", features="mssql", kerberos=True
    ),
    "postgres-integration": Profile(
        package="acton-service-integration-tests", features="postgres"
    ),
    "postgres-integration-ring": Profile(
        package="acton-service-integration-tests",
        features="postgres,crypto-ring",
        defaults=False,
    ),
    "surrealdb-integration": Profile(
        package="acton-service-integration-tests", features="surrealdb"
    ),
    "clickhouse-integration": Profile(
        package="acton-service-integration-tests", features="clickhouse"
    ),
    "grpc-examples": Profile(
        package="acton-service-integration-tests",
        features="grpc,cedar-authz",
        tests=False,
        protoc=True,
    ),
    "grpc-integration": Profile(
        package="acton-service-integration-tests", features="grpc,tls", protoc=True
    ),
    "grpc-integration-ring": Profile(
        package="acton-service-integration-tests",
        features="grpc,tls,crypto-ring",
        defaults=False,
        protoc=True,
    ),
}
QUALIFICATION = PROFILES.keys() - {
    "cli",
    "mssql-integration",
    "surrealdb-integration",
    "clickhouse-integration",
}
# One deliberate writer per family; rare profiles restore without competing saves.
CACHE_WRITERS = {
    "default",
    "full",
    "tls-ring",
    "audit-surrealdb",
    "windows",
    "frontend",
}


def cache_family(name: str) -> str:
    if PROFILES[name].runner == "windows":
        return "windows"
    if name in {"audit-surrealdb", "surrealdb-integration"}:
        return "audit-surrealdb"
    if name in {
        "ring",
        "tls-ring",
        "postgres-ring",
        "postgres-integration-ring",
        "grpc-integration-ring",
    }:
        return "ring"
    if name in {"default", "minimal", "core", "audit", "audit-events"}:
        return "default"
    if name == "frontend":
        return "frontend"
    return "full"


def matrix_entry(name: str) -> dict[str, object]:
    profile = PROFILES[name]
    return {
        "profile": name,
        "runner": profile.runner,
        "protoc": profile.protoc,
        "kerberos": profile.kerberos,
        "tests": profile.tests,
        "cache": cache_family(name),
        "cache_writer": name in CACHE_WRITERS,
    }


def matrix(names: set[str]) -> dict[str, list[dict[str, object]]]:
    unknown = names - PROFILES.keys()
    if unknown:
        raise ValueError(f"Unknown validation profiles: {sorted(unknown)}")
    return {"include": [matrix_entry(name) for name in sorted(names)]}
