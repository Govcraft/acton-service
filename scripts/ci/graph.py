"""Enforce backend and SQLx provider boundaries before expensive compilation."""

from profiles import PROFILES

DRIVERS = {
    "postgres": "sqlx-postgres",
    "mssql": "tiberius",
    "turso": "libsql",
    "surrealdb": "surrealdb-core",
    "clickhouse": "clickhouse",
}


def backend_for(name: str) -> str | None:
    profile = PROFILES[name]
    for package in (profile.package, profile.companion):
        backend = package.removeprefix("acton-service-")
        if backend in DRIVERS:
            return backend
    features = set(profile.features.split(","))
    if features & {"full", "database", "postgres"}:
        return "postgres"
    return next((backend for backend in DRIVERS if backend in features), None)


def verify_graph(text: str, backend: str | None, ring: bool = False) -> None:
    packages: dict[str, set[str]] = {}
    for line in text.splitlines():
        if not line or line in {"[build-dependencies]", "[dev-dependencies]"}:
            continue
        if "|" not in line:
            raise ValueError(f"Malformed dependency graph record: {line}")
        package, features = line.split("|", 1)
        packages.setdefault(package.split()[0], set()).update(
            filter(None, features.removesuffix(" (*)").split(","))
        )
    if not packages:
        raise ValueError("Missing resolved dependency graph")
    actual = {name for name, driver in DRIVERS.items() if driver in packages}
    unexpected = {"sqlx-mysql", "sqlx-sqlite"} & packages.keys()
    if unexpected:
        raise ValueError(f"Unsupported SQLx drivers resolved: {unexpected}")
    expected = {backend} if backend else set()
    if actual != expected:
        raise ValueError(f"Expected isolated backend {expected}, resolved {actual}")
    if backend == "postgres":
        provider = "_tls-rustls-ring-webpki" if ring else "_tls-rustls-aws-lc-rs"
        configured = packages.get("sqlx-core", set()) & {
            "_tls-rustls-ring-webpki",
            "_tls-rustls-aws-lc-rs",
        }
        if configured != {provider}:
            raise ValueError(
                f"Expected SQLx provider {provider}, resolved {configured}"
            )
