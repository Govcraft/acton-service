"""Recognize stable workspace version bumps without hiding dependency changes.

Adapted from SchemaForge's conservative semantic manifest/lock comparison.
Cargo still validates the resulting metadata before a version-only gate can pass.
"""

import copy
import os
import re
import subprocess
import tomllib
from pathlib import PurePosixPath


def snapshot(revision: str) -> dict[str, str]:
    paths = subprocess.check_output(
        ["git", "ls-tree", "-r", "--name-only", "-z", revision]
    )
    names = [
        path.decode()
        for path in paths.split(b"\0")
        if path.endswith(b"Cargo.toml") or path == b"Cargo.lock"
    ]
    return {
        name: subprocess.check_output(["git", "show", f"{revision}:{name}"], text=True)
        for name in names
    }


def packages(files: dict[str, str]) -> dict[str, tuple[str, str]]:
    root = tomllib.loads(files["Cargo.toml"])
    version = root["workspace"]["package"]["version"]
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise ValueError("Version fast path supports stable workspace versions only")
    result = {}
    for member in root["workspace"]["members"]:
        manifest = member + "/Cargo.toml"
        package = tomllib.loads(files[manifest])["package"]
        if package["version"] != {"workspace": True} or package["name"] in result:
            raise ValueError("Expected unique workspace-inherited packages")
        result[package["name"]] = (manifest, version)
    return result


def dependency_tables(manifest: dict):
    for kind in ("dependencies", "dev-dependencies", "build-dependencies"):
        yield manifest.get(kind, {})
        yield manifest.get("workspace", {}).get(kind, {})
        for target in manifest.get("target", {}).values():
            yield target.get(kind, {})


def satisfies(version: str, requirement: str) -> bool:
    match = re.fullmatch(r"(\^|=|~)?([0-9]+\.[0-9]+\.[0-9]+)", requirement)
    if not match:
        return False
    actual = tuple(map(int, version.split(".")))
    wanted = tuple(map(int, match[2].split(".")))
    if match[1] == "=":
        return actual == wanted
    count = (
        2
        if match[1] == "~"
        else next((i + 1 for i, value in enumerate(wanted) if value), 3)
    )
    return actual >= wanted and actual[:count] == wanted[:count]


def normalized_manifest(path: str, text: str, members: dict) -> dict:
    manifest = copy.deepcopy(tomllib.loads(text))
    if path == "Cargo.toml":
        manifest["workspace"]["package"].pop("version")
    directories = {
        str(PurePosixPath(p).parent): (name, version)
        for name, (p, version) in members.items()
    }
    for table in dependency_tables(manifest):
        for alias, dependency in table.items():
            if isinstance(dependency, dict) and "path" in dependency:
                directory = os.path.normpath(
                    str(PurePosixPath(path).parent / dependency["path"])
                )
                if directory not in directories:
                    raise ValueError("Unknown local dependency")
                name, version = directories[directory]
                requirement = dependency.get("version")
                if (
                    dependency.get("package", alias) != name
                    or not isinstance(requirement, str)
                    or not satisfies(version, requirement)
                ):
                    raise ValueError("Invalid local registry version requirement")
                dependency.pop("version")
    return manifest


def normalized_lock(text: str, members: dict, versions: dict) -> dict:
    lock = tomllib.loads(text)
    registry_names = {p["name"] for p in lock["package"] if "source" in p}
    seen = set()
    for package in lock["package"]:
        name = package["name"]
        if name in members and "source" not in package:
            if package["version"] != members[name][1] or name in seen:
                raise ValueError("Stale or duplicate workspace lock entry")
            seen.add(name)
            package.pop("version")
        for i, dependency in enumerate(package.get("dependencies", [])):
            words = dependency.split()
            if (
                len(words) == 2
                and words[0] in versions
                and words[0] not in registry_names
                and words[1] in versions[words[0]]
            ):
                package["dependencies"][i] = words[0]
        if "dependencies" in package:
            package["dependencies"].sort()
    if seen != members.keys():
        raise ValueError("Missing workspace lock entries")
    lock["package"].sort(
        key=lambda p: (p["name"], p.get("version", ""), p.get("source", ""))
    )
    return lock


def version_only(
    paths: list[str], before: dict[str, str], after: dict[str, str]
) -> bool:
    try:
        old, new = packages(before), packages(after)
        if old.keys() != new.keys() or {
            name: path for name, (path, _) in old.items()
        } != {name: path for name, (path, _) in new.items()}:
            return False
        allowed = {name: {old[name][1], new[name][1]} for name in old}
        manifests = {"Cargo.toml", *(path for path, _ in old.values())}
        if not paths or not set(paths) <= manifests | {"Cargo.lock"}:
            return False
        for path in manifests:
            if normalized_manifest(path, before[path], old) != normalized_manifest(
                path, after[path], new
            ):
                return False
        return normalized_lock(before["Cargo.lock"], old, allowed) == normalized_lock(
            after["Cargo.lock"], new, allowed
        )
    except (KeyError, TypeError, ValueError):
        return False
