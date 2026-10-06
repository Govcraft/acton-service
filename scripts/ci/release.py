"""Validate a release and publish the workspace graph in dependency order."""

import argparse
import hashlib
import io
import json
import os
import re
import subprocess
import tarfile
import tomllib
import urllib.error
import urllib.request
from pathlib import Path

from run_profile import verify_revision


def publication_order(metadata: dict) -> list[str]:
    members = set(metadata["workspace_members"])
    workspace = {p["name"]: p for p in metadata["packages"] if p["id"] in members}
    publishable = {name: p for name, p in workspace.items() if p.get("publish") != []}
    if "acton-service" not in publishable:
        raise ValueError("The acton-service facade must be publishable")
    edges: dict[str, set[str]] = {}
    for name, package in publishable.items():
        deps = set()
        for dep in package["dependencies"]:
            if dep.get("path") is None:
                continue
            dependency = dep["name"]
            if dependency not in publishable:
                if dep.get("kind") == "dev":
                    continue
                raise ValueError(
                    f"{name} depends on unpublished local package {dependency}"
                )
            requirement = dep.get("req", "*")
            if requirement == "*":
                raise ValueError(
                    f"{name} has no publishable version requirement for {dependency}"
                )
            if dep.get("kind") != "dev":
                deps.add(dependency)
        edges[name] = deps
    ordered: list[str] = []
    while edges:
        ready = sorted(name for name, deps in edges.items() if not deps)
        if not ready:
            raise ValueError(f"Publication dependency cycle: {sorted(edges)}")
        ordered.extend(ready)
        edges = {
            name: deps - set(ready) for name, deps in edges.items() if name not in ready
        }
    if ordered[-1] != "acton-service":
        # Independent publishable tools belong before the facade, too.
        ordered.remove("acton-service")
        ordered.append("acton-service")
        for name, package in publishable.items():
            if name != "acton-service" and any(
                d["name"] == "acton-service" and d.get("kind") != "dev"
                for d in package["dependencies"]
            ):
                raise ValueError(
                    f"Publishable {name} depends on facade; review the release graph"
                )
    return ordered


def publish_command(
    order: list[str], dry_run: bool, allow_dirty: bool = False
) -> list[str]:
    if allow_dirty and not dry_run:
        raise ValueError("Dirty source is permitted only for a local dry run")
    command = ["cargo", "publish", "--locked", "--registry", "crates-io"]
    if dry_run:
        command.append("--dry-run")
    else:
        # Qualification already built every packaged crate at EXPECTED_SHA.
        # Avoid rebuilding heavy adapters after obtaining the short-lived token.
        command.append("--no-verify")
    if allow_dirty:
        command.append("--allow-dirty")
    for package in order:
        command.extend(["-p", package])
    return command


def validate_published_archive(
    archive: bytes, checksum: str, name: str, version: str, sha: str
) -> None:
    if hashlib.sha256(archive).hexdigest() != checksum:
        raise ValueError(f"Registry archive checksum mismatch for {name} {version}")
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as package:
        entry = package.extractfile(f"{name}-{version}/.cargo_vcs_info.json")
        if entry is None:
            raise ValueError(
                f"Published {name} {version} has no source revision record"
            )
        source = json.load(entry)["git"]
    if source.get("sha1") != sha or source.get("dirty", False):
        raise ValueError(
            f"Published {name} {version} was not built from qualified commit {sha}"
        )


def remaining_packages(
    order: list[str], versions: dict[str, str], sha: str, fetch
) -> list[str]:
    remaining = []
    for name in order:
        version = versions[name]
        url = f"https://crates.io/api/v1/crates/{name}/{version}"
        try:
            record = json.loads(fetch(url))
        except urllib.error.HTTPError as error:
            error.close()
            if error.code != 404:
                raise
            remaining.append(name)
            continue
        archive = fetch(url + "/download")
        validate_published_archive(
            archive, record["version"]["checksum"], name, version, sha
        )
        print(
            f"Already published from this qualified commit: {name} {version}",
            flush=True,
        )
    return remaining


def fetch_registry(url: str) -> bytes:
    request = urllib.request.Request(
        url, headers={"User-Agent": "acton-service-release (roland@govcraft.ai)"}
    )
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def validate_tag(tag: str, version: str) -> None:
    if not re.fullmatch(r"acton-service-v[0-9]+\.[0-9]+\.[0-9]+", tag):
        raise ValueError("Expected an acton-service-vMAJOR.MINOR.PATCH release tag")
    if tag != f"acton-service-v{version}":
        raise ValueError(f"Release tag must match workspace version {version}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "action",
        choices=["resolve", "dry-run", "publish", "bootstrap", "order", "preflight"],
    )
    parser.add_argument("--tag", default=os.environ.get("RELEASE_TAG", ""))
    parser.add_argument("--allow-dirty", action="store_true")
    args = parser.parse_args()
    if args.action == "resolve":
        version = tomllib.loads(Path("Cargo.toml").read_text())["workspace"]["package"][
            "version"
        ]
        validate_tag(args.tag, version)
        sha = subprocess.check_output(
            ["git", "rev-parse", "--verify", f"refs/tags/{args.tag}^{{commit}}"],
            text=True,
        ).strip()
        verify_revision(sha)
        subprocess.run(
            ["git", "merge-base", "--is-ancestor", sha, "origin/main"], check=True
        )
        output = os.environ.get("GITHUB_OUTPUT")
        if output:
            with open(output, "a", encoding="utf-8") as stream:
                stream.write(f"sha={sha}\n")
        print(f"Qualified release candidate: {args.tag} at {sha}")
        return
    expected = os.environ.get("EXPECTED_SHA")
    if expected:
        verify_revision(expected)
    if args.action in {"publish", "bootstrap", "preflight"} and not expected:
        raise ValueError(
            "Publishing requires the SHA that passed release qualification"
        )
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"],
            text=True,
        )
    )
    order = publication_order(metadata)
    print("Publication order: " + " -> ".join(order), flush=True)
    if args.action in {"publish", "bootstrap", "preflight"}:
        versions = {p["name"]: p["version"] for p in metadata["packages"]}
        order = remaining_packages(order, versions, expected, fetch_registry)
        if args.action == "bootstrap":
            order = [name for name in order if name != "acton-service"]
        if not order:
            print("All selected packages are already published from this commit.")
            return
    if args.action not in {"order", "preflight"}:
        subprocess.run(
            publish_command(order, args.action == "dry-run", args.allow_dirty),
            check=True,
        )


if __name__ == "__main__":
    main()
