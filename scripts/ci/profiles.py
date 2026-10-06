"""One catalog of supported configurations for development and releases."""

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

    def cargo_options(self) -> list[str]:
        options = ["--locked", "-p", self.package]
        if not self.defaults:
            options.append("--no-default-features")
        if self.features:
            options.extend(["--features", self.features])
        return options


def narrow(features: str, **options: bool) -> Profile:
    return Profile(
        features=f"http,{features},crypto-aws-lc-rs",
        defaults=False,
        **options,
    )


PROFILES = {
    "default": Profile(doctests=True),
    "minimal": Profile(features="http,crypto-aws-lc-rs", defaults=False),
    "full": Profile(features="full", doctests=True, protoc=True),
    "ring": Profile(features="full,crypto-ring", defaults=False, protoc=True),
    "windows": Profile(runner="windows", protoc=True),
    "audit-turso": narrow("audit,turso,observability"),
    "audit-surrealdb": narrow("audit,surrealdb,auth,jwt,observability"),
    "audit-clickhouse": Profile(features="audit,clickhouse"),
    "audit-otel": narrow("audit,observability"),
    "mssql": narrow(
        "mssql,grpc,auth,accounts,audit,observability", protoc=True, kerberos=True
    ),
    "tls-no-grpc": narrow("tls"),
    "windows-auth": narrow("tls,windows-auth"),
    "saml": narrow("saml"),
    "grpc-no-tls": narrow("grpc", protoc=True),
    "grpc-tls": narrow("grpc,tls,auth,cedar-authz", protoc=True),
    "oauth-no-cache": narrow("oauth"),
    "oauth-with-cache": narrow("oauth,cache,jwt"),
    "otel-only": narrow("otel-metrics"),
    "metrics": narrow("otel-metrics,prometheus-metrics,tls"),
    "tokens": narrow("jwt,auth"),
    "frontend": narrow("htmx,askama,sse,session-memory"),
    "graphql": narrow("graphql,graphql-cedar,cedar-authz"),
    "audit-nodb": narrow("audit"),
    "core": Profile(package="acton-service-core", doctests=True),
    "core-mssql": Profile(package="acton-service-core", features="mssql"),
    "audit": Profile(package="acton-service-audit", doctests=True),
    "audit-events": Profile(
        package="acton-service-audit", features="accounts,login-lockout"
    ),
    "postgres": Profile(package="acton-service-postgres", doctests=True),
    "postgres-ring": Profile(
        package="acton-service-postgres",
        features="crypto-ring",
        defaults=False,
    ),
    "mssql-adapter": Profile(
        package="acton-service-mssql", doctests=True, kerberos=True
    ),
    "turso-adapter": Profile(package="acton-service-turso", doctests=True),
    "surrealdb-adapter": Profile(package="acton-service-surrealdb", doctests=True),
    "clickhouse-adapter": Profile(package="acton-service-clickhouse", doctests=True),
    "cli": Profile(package="acton-cli"),
    "mssql-integration": Profile(
        package="acton-service-integration-tests",
        features="mssql",
        kerberos=True,
    ),
    "grpc-examples": Profile(
        package="acton-service-integration-tests",
        features="grpc,cedar-authz",
        tests=False,
        protoc=True,
    ),
    "grpc-integration": Profile(
        package="acton-service-integration-tests",
        features="grpc,tls",
        protoc=True,
    ),
    "grpc-integration-ring": Profile(
        package="acton-service-integration-tests",
        features="grpc,tls,crypto-ring",
        defaults=False,
        protoc=True,
    ),
}


def matrix(names: set[str]) -> dict[str, list[dict[str, object]]]:
    unknown = names - PROFILES.keys()
    if unknown:
        raise ValueError(f"Unknown validation profiles: {sorted(unknown)}")
    return {
        "include": [
            {
                "profile": name,
                "runner": PROFILES[name].runner,
                "protoc": PROFILES[name].protoc,
                "kerberos": PROFILES[name].kerberos,
            }
            for name in sorted(names)
        ]
    }
