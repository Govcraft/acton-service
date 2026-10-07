"""Bound compiler uploads and retain a deliberate repository cache working set."""

import argparse
import hashlib
import json
import os
import re
import shutil
import tomllib
import urllib.error
import urllib.request
from pathlib import Path

PREFIX = "ci-v2-"
BUDGET = 8 * 1024**3
UPLOAD_LIMIT = 4096 * 1024**2
ROOT = Path(__file__).resolve().parents[2]


def compiler_identity(family: str, platform: str, architecture: str, root=ROOT):
    """Include feature declarations and recipes, without hashing source edits."""
    if not all(
        re.fullmatch(r"[A-Za-z0-9_-]+", v) for v in (family, platform, architecture)
    ):
        raise ValueError("Invalid compiler cache identity")
    manifest = tomllib.loads((root / "Cargo.toml").read_text())
    paths = {
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        ".cargo/config.toml",
        ".cargo/config",
    }
    paths.update(
        path.relative_to(root).as_posix()
        for member in manifest["workspace"]["members"]
        for path in root.glob(f"{member}/Cargo.toml")
        if path.is_file()
    )
    paths.update(
        {
            "scripts/ci/profiles.py",
            "scripts/ci/run_profile.py",
            "scripts/ci/cache.py",
            "scripts/ci/registry_cache.py",
            "scripts/ci/warm_caches.py",
            ".github/actions/compiler-cache/action.yml",
            ".github/actions/registry-cache/action.yml",
            ".github/workflows/rust-validation.yml",
            ".github/workflows/cache-maintenance.yml",
        }
    )
    digest = hashlib.sha256()
    for name in sorted(paths):
        path = root / name
        if path.is_file():
            contents = path.read_bytes().replace(b"\r\n", b"\n")
            digest.update(name.encode() + b"\0" + contents + b"\0")
    compiler = tomllib.loads((root / "rust-toolchain.toml").read_text())["toolchain"][
        "channel"
    ]
    prefix = f"{PREFIX}{family}--{platform}-{architecture}-{compiler}-debug0--"
    # The second prefix permits migration from the previous explicit compiler keys.
    fallback = f"{PREFIX}{family}--{platform}-{compiler}-debug0--"
    prefixes = [prefix, fallback]
    if family in {"mssql", "turso"}:
        # New native-driver families can reuse common libraries from old full caches.
        prefixes.extend(
            [
                f"{PREFIX}full--{platform}-{architecture}-{compiler}-debug0--",
                f"{PREFIX}full--{platform}-{compiler}-debug0--",
            ]
        )
    return prefix + digest.hexdigest(), prefixes


def trim_target(target: Path) -> bool:
    """Keep reusable libraries/build outputs; omit docs, tests, and incremental data."""
    for relative in ("debug/incremental", "debug/examples", "doc"):
        shutil.rmtree(target / relative, ignore_errors=True)
    deps = target / "debug/deps"
    if deps.exists():
        for path in deps.iterdir():
            if path.is_file() and path.suffix in {"", ".exe"}:
                path.unlink()
    size = sum(p.stat().st_size for p in target.rglob("*") if p.is_file())
    print(f"Compiler cache upload footprint: {size / 1024**2:.1f} MiB")
    return size <= UPLOAD_LIMIT


def obsolete(entries: list[dict], budget: int = BUDGET) -> list[int]:
    """Bound main caches after reserving storage for other workflows and open PRs."""
    selected = []
    seen = set()
    retained = sum(
        entry["size_in_bytes"]
        for entry in entries
        if entry["ref"] != "refs/heads/main"
        or not entry["key"].startswith((PREFIX, "v0-rust-"))
    )
    for entry in sorted(
        entries,
        key=lambda e: (e["ref"] == "refs/heads/main", e["last_accessed_at"]),
        reverse=True,
    ):
        key = entry["key"]
        if entry["ref"] != "refs/heads/main":
            continue
        if key.startswith("v0-rust-"):
            selected.append(entry["id"])
        elif key.startswith(PREFIX):
            # Lock/toolchain suffixes are versioned; the family identifies ownership.
            family = key.removeprefix(PREFIX).split("--", 1)[0]
            identity = (family, entry["ref"])
            if identity in seen or retained + entry["size_in_bytes"] > budget:
                selected.append(entry["id"])
            else:
                seen.add(identity)
                retained += entry["size_in_bytes"]
    return selected


def api(path: str, method: str = "GET"):
    url = (
        f"{os.environ['GITHUB_API_URL']}/repos/{os.environ['GITHUB_REPOSITORY']}/{path}"
    )
    request = urllib.request.Request(
        url,
        method=method,
        headers={
            "Authorization": f"Bearer {os.environ['GH_TOKEN']}",
            "Accept": "application/vnd.github+json",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response) if method == "GET" else None
    except urllib.error.HTTPError as error:
        if method == "DELETE" and error.code == 404:
            return None
        raise


def inventory(request=api):
    entries = []
    page = 1
    while True:
        batch = request(f"actions/caches?per_page=100&page={page}")["actions_caches"]
        entries.extend(batch)
        if len(batch) < 100:
            return entries
        page += 1


def closed_pr_caches(entries: list[dict], request=api, number=None) -> list[int]:
    """Confirm PR states before deletion; never trust payload state."""
    numbers = {
        int(match[1])
        for entry in entries
        if (
            match := re.fullmatch(
                r"refs/pull/([1-9][0-9]*)/merge", entry.get("ref") or ""
            )
        )
        and (number is None or int(match[1]) == number)
    }
    states = {n: request(f"pulls/{n}")["state"] for n in sorted(numbers)}
    return [
        entry["id"]
        for entry in entries
        if (
            match := re.fullmatch(
                r"refs/pull/([1-9][0-9]*)/merge", entry.get("ref") or ""
            )
        )
        and states.get(int(match[1])) == "closed"
        and type(entry["id"]) is int
        and entry["id"] > 0
    ]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "command", choices=["prepare", "retain", "identity", "closed-prs"]
    )
    parser.add_argument("--family")
    parser.add_argument("--pr", type=int)
    args = parser.parse_args()
    if args.command == "identity":
        key, prefixes = compiler_identity(
            args.family or "", os.environ["RUNNER_OS"], os.environ["RUNNER_ARCH"]
        )
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as stream:
            stream.write(f"key={key}\nrestore-keys<<CACHE_PREFIXES\n")
            stream.write("\n".join(prefixes) + "\nCACHE_PREFIXES\n")
        return
    if args.command == "prepare":
        allowed = trim_target(Path(os.environ.get("CARGO_TARGET_DIR", "target")))
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as stream:
            stream.write(f"save={str(allowed).lower()}\n")
        return
    if args.pr is not None and args.pr <= 0:
        parser.error("PR number must be positive")
    entries = inventory()
    selected = (
        closed_pr_caches(entries, number=args.pr)
        if args.command == "closed-prs"
        else obsolete(entries)
    )
    for cache_id in selected:
        api(f"actions/caches/{cache_id}", "DELETE")
        print(f"Removed obsolete cache {cache_id}")


if __name__ == "__main__":
    main()
