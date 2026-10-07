"""Bound compiler uploads and retain a deliberate repository cache working set."""

import argparse
import json
import os
import shutil
import urllib.request
from pathlib import Path

PREFIX = "ci-v2-"
BUDGET = 8 * 1024**3
UPLOAD_LIMIT = 4096 * 1024**2


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
    """Keep newest family/ref entries within budget; migrate away from old Rust keys."""
    selected = []
    seen = set()
    retained = 0
    for entry in sorted(
        entries,
        key=lambda e: (e["ref"] == "refs/heads/main", e["last_accessed_at"]),
        reverse=True,
    ):
        key = entry["key"]
        if key.startswith("v0-rust-"):
            selected.append(entry["id"])
        elif key.startswith(PREFIX):
            # Lock/toolchain suffixes are versioned; family and ref identify ownership.
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
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response) if method == "GET" else None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["prepare", "retain"])
    args = parser.parse_args()
    if args.command == "prepare":
        allowed = trim_target(Path(os.environ.get("CARGO_TARGET_DIR", "target")))
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as stream:
            stream.write(f"save={str(allowed).lower()}\n")
        return
    entries = []
    for page in range(1, 11):
        batch = api(f"actions/caches?per_page=100&page={page}")["actions_caches"]
        entries.extend(batch)
        if len(batch) < 100:
            break
    for cache_id in obsolete(entries):
        api(f"actions/caches/{cache_id}", "DELETE")
        print(f"Removed obsolete cache {cache_id}")


if __name__ == "__main__":
    main()
